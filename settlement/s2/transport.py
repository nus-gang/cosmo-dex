"""Trusted parent transport for the pinned S2 Rust sequencer (stdlib only)."""
import base64
import json
import os
import selectors
import subprocess
import threading
import time


class Unavailable(Exception):
    """No authoritative completion was received; never manufacture rejection."""


def unique(pairs):
    value = {}
    for key, item in pairs:
        if key in value:
            raise ValueError('duplicate key')
        value[key] = item
    return value


def decode(raw):
    return json.loads(raw, object_pairs_hook=unique,
                      parse_constant=lambda _: (_ for _ in ()).throw(ValueError('constant')))


def encode(value):
    return json.dumps(value, sort_keys=True, separators=(',', ':'),
                      ensure_ascii=True, allow_nan=False).encode()


def unknown(code='SUBMISSION_UNKNOWN'):
    return {'code': code, 'state': 'SUBMISSION_UNKNOWN', 'retryable': True,
            'observed_height': None, 'stream_seq': None}


class Engine:
    """One response per request, bounded nonblocking IO and fail-stop ambiguity.

    A timeout kills this child before releasing the lock. Otherwise a late
    response could be attributed to a different owner/request. Recovery requires
    explicit process restart/open; this class never recreates a journal.
    """
    MAX_CONTROL = 100_000
    MAX_RESPONSE = 32 * 1024 * 1024

    def __init__(self, argv, timeout=10):
        self.timeout = timeout
        self.lock = threading.Lock()
        self.failed = False
        self.child = subprocess.Popen(argv, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                      stderr=subprocess.DEVNULL, bufsize=0)
        os.set_blocking(self.child.stdin.fileno(), False)
        os.set_blocking(self.child.stdout.fileno(), False)

    def _stop(self):
        self.failed = True
        if self.child.poll() is None:
            self.child.kill()
        self.child.wait(timeout=5)
        self.child.stdin.close()
        self.child.stdout.close()

    def close(self):
        with self.lock:
            if not self.failed:
                self._stop()

    def call(self, message):
        data = encode(message) + b'\n'
        if len(data) - 1 > self.MAX_CONTROL:
            raise ValueError('control limit')
        with self.lock:
            if self.failed:
                raise Unavailable('ENGINE_UNAVAILABLE')
            deadline = time.monotonic() + self.timeout
            try:
                with selectors.DefaultSelector() as poll:
                    poll.register(self.child.stdin, selectors.EVENT_WRITE)
                    sent = 0
                    result = bytearray()
                    while True:
                        remaining = deadline - time.monotonic()
                        if remaining <= 0 or not poll.select(remaining):
                            raise Unavailable('ENGINE_TIMEOUT')
                        if sent < len(data):
                            sent += os.write(self.child.stdin.fileno(), data[sent:])
                            if sent == len(data):
                                poll.unregister(self.child.stdin)
                                poll.register(self.child.stdout, selectors.EVENT_READ)
                            continue
                        chunk = os.read(self.child.stdout.fileno(), 65536)
                        if not chunk:
                            raise Unavailable('ENGINE_EOF')
                        result.extend(chunk)
                        if len(result) > self.MAX_RESPONSE:
                            raise Unavailable('ENGINE_FRAME_LIMIT')
                        if b'\n' in chunk:
                            if result.count(b'\n') != 1 or not result.endswith(b'\n'):
                                raise Unavailable('ENGINE_FRAME')
                            value = decode(result)
                            if (not isinstance(value, dict) or set(value) != {'http_status', 'body'}
                                    or value['http_status'] not in {str(n) for n in range(200, 600)}
                                    or not isinstance(value['body'], dict)):
                                raise Unavailable('ENGINE_FRAME')
                            return int(value['http_status']), value['body']
            except (OSError, ValueError, TypeError, Unavailable) as exc:
                self._stop()
                raise Unavailable('ENGINE_COMPLETION_UNKNOWN') from exc

    def request(self, method, path, origin=None, authorization=None, body=b''):
        # Never interpret the browser's body as a trusted control message.
        return self.call({'op': 'request', 'method': method, 'path': path,
                          'origin': origin, 'authorization': authorization,
                          'body_base64': base64.b64encode(body).decode()})

    def rpc_failed(self):
        return self.call({'op': 'rpc_failed'})
