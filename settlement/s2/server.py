#!/usr/bin/env python3
"""Loopback REST adapter. No signing, matching, or settlement occurs in Python."""
import argparse
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import ipaddress
from pathlib import Path
import re
import signal
import threading

from bootstrap import genesis_manifest
from chain import Collector, RPC
from transport import Engine, Unavailable, decode, encode, unknown
from direct import Direct, route as direct_route

ORIGINS = {'http://127.0.0.1:5173', 'http://localhost:5173'}
PUBLIC = {'/s2/network', '/s2/status', '/s2/book'}
MAX_BODY = 16384


class Server(ThreadingHTTPServer):
    daemon_threads = True
    request_queue_size = 16

    def __init__(self, address, engine, direct=None):
        if not ipaddress.ip_address(address[0]).is_loopback:
            raise ValueError('loopback bind required')
        self.engine = engine
        self.direct = direct
        self.slots = threading.BoundedSemaphore(16)
        super().__init__(address, Handler)

    def process_request(self, request, address):
        if not self.slots.acquire(blocking=False):
            self.shutdown_request(request)
            return
        try:
            super().process_request(request, address)
        except BaseException:
            self.slots.release()
            raise

    def process_request_thread(self, request, address):
        try:
            super().process_request_thread(request, address)
        finally:
            self.slots.release()


class Handler(BaseHTTPRequestHandler):
    # Close every connection: no pipelined body desynchronization or shared cache.
    protocol_version = 'HTTP/1.0'

    def setup(self):
        self.request.settimeout(2)
        super().setup()

    def log_message(self, *args):
        pass

    def respond(self, status, body):
        raw = b'' if status == 204 else encode(body)
        self.send_response(status)
        self.send_header('Content-Type', 'application/json')
        self.send_header('Content-Length', str(len(raw)))
        self.send_header('Cache-Control', 'no-store')
        self.send_header('Vary', 'Origin, Authorization')
        self.send_header('X-Content-Type-Options', 'nosniff')
        self.send_header('Connection', 'close')
        origins = self.headers.get_all('Origin', [])
        if len(origins) == 1 and origins[0] in ORIGINS:
            self.send_header('Access-Control-Allow-Origin', origins[0])
            if status == 204:
                self.send_header('Access-Control-Allow-Methods', 'GET, POST')
                self.send_header('Access-Control-Allow-Headers', 'Authorization, Content-Type')
        self.end_headers()
        self.wfile.write(raw)

    def reject(self, status, code):
        body = unknown(code)
        body.update(state='REJECTED', retryable=False)
        self.respond(status, body)

    def do_OPTIONS(self):
        self.dispatch(preflight=True)

    def do_GET(self):
        self.dispatch()

    def do_POST(self):
        self.dispatch()

    def dispatch(self, preflight=False):
        try:
            for name in ('Host', 'Origin', 'Authorization', 'Content-Length', 'Content-Type',
                         'Access-Control-Request-Method', 'Access-Control-Request-Headers'):
                if len(self.headers.get_all(name, [])) > 1:
                    return self.reject(400, 'NON_CANONICAL_WIRE')
            # Reject DNS rebinding and proxy absolute URLs; no client-selected host.
            host = self.headers.get('Host')
            port = self.server.server_port
            if host not in {f'127.0.0.1:{port}', f'localhost:{port}'}:
                return self.reject(403, 'FORBIDDEN')
            is_direct = self.server.direct is not None and direct_route(self.path)
            if (len(self.path) > 8192 or not (self.path.startswith('/s2/') or is_direct)
                    or not self.path.isascii() or '#' in self.path):
                return self.reject(404, 'UNSUPPORTED_ROUTE')
            origin = self.headers.get('Origin')
            if (origin is not None and origin not in ORIGINS) or (
                    origin is None and (self.command != 'GET' or self.path not in PUBLIC and not is_direct)):
                return self.reject(403, 'FORBIDDEN')
            if self.headers.get_all('Transfer-Encoding') or self.headers.get_all('Expect'):
                return self.reject(400, 'NON_CANONICAL_WIRE')
            length = self.headers.get('Content-Length', '0')
            if not re.fullmatch(r'0|[1-9][0-9]{0,5}', length):
                return self.reject(400, 'NON_CANONICAL_WIRE')
            size = int(length)
            if size > (22000 if is_direct else MAX_BODY):
                return self.reject(413, 'RESOURCE_LIMIT')
            if self.command in ('GET', 'OPTIONS') and size:
                return self.reject(400, 'NON_CANONICAL_WIRE')
            if preflight:
                method = self.headers.get('Access-Control-Request-Method')
                headers = self.headers.get('Access-Control-Request-Headers', '')
                if method not in ('GET', 'POST') or any(
                        h.strip().lower() not in ('authorization', 'content-type')
                        for h in headers.split(',') if h.strip()):
                    return self.reject(403, 'FORBIDDEN')
                return self.respond(204, {})
            if size and self.headers.get('Content-Type') != 'application/json':
                return self.reject(415, 'NON_CANONICAL_WIRE')
            raw = self.rfile.read(size)
            if len(raw) != size:
                return self.reject(400, 'NON_CANONICAL_WIRE')
            if is_direct:
                status, body = self.server.direct.request(self.command, self.path, raw)
                return self.respond(status, body)
            status, body = self.server.engine.request(
                self.command, self.path, origin, self.headers.get('Authorization'), raw)
            self.respond(status, body)
        except Unavailable:
            self.respond(503, unknown())
        except (OSError, ValueError):
            self.close_connection = True


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--engine', required=True)
    parser.add_argument('--manifest', required=True)
    parser.add_argument('--genesis', required=True)
    parser.add_argument('--journal', required=True)
    parser.add_argument('--evidence', required=True)
    parser.add_argument('--bootstrap', help='explicit new journal only; omit to open existing')
    parser.add_argument('--rpc', default='http://127.0.0.1:26657')
    parser.add_argument('--port', type=int, default=8788)
    args = parser.parse_args()
    manifest = decode(Path(args.manifest).read_bytes())
    genesis = Path(args.genesis).read_bytes()
    expected, _ = genesis_manifest(genesis)
    if any(manifest.get(key) != value for key, value in expected.items()):
        parser.error('S2 genesis binding mismatch')
    rpc = RPC(args.rpc)
    argv = [args.engine, 'create' if args.bootstrap else 'open', args.manifest, args.journal]
    if args.bootstrap:
        initial = decode(Path(args.bootstrap).read_bytes())
        if initial['body']['observed_height'] != '1':
            parser.error('bootstrap must begin at committed height 1')
        verifier = Collector(rpc, None, manifest, args.evidence)
        if verifier.fetch(1) != initial:
            parser.error('bootstrap RPC evidence mismatch')
        argv.append(args.bootstrap)
    engine = Engine(argv)
    def stop_signal(signum, frame):
        raise KeyboardInterrupt
    signal.signal(signal.SIGTERM, stop_signal)
    stopped = threading.Event()
    server = None
    collector_thread = None
    try:
        status, _ = engine.request('GET', '/s2/status')
        if status != 200:
            raise Unavailable('ENGINE_STARTUP')
        collector = Collector(rpc, engine, manifest, args.evidence)
        def observe():
            while not stopped.is_set():
                try:
                    collector.tick()
                except Unavailable:
                    # Pipe failure is permanent until explicit process restart.
                    break
                stopped.wait(1)
        collector_thread = threading.Thread(target=observe, daemon=True)
        collector_thread.start()
        direct = Direct(rpc, manifest, Path(args.evidence) / 'direct')
        server = Server(('127.0.0.1', args.port), engine, direct)
        server.serve_forever(poll_interval=0.25)
    except KeyboardInterrupt:
        pass
    finally:
        stopped.set()
        if server:
            server.server_close()
        if collector_thread:
            collector_thread.join(timeout=15)
        engine.close()


if __name__ == '__main__':
    main()
