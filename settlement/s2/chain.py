"""Sequential committed RPC observations. The engine owns the durable cursor."""
import base64
import datetime
import hashlib
import ipaddress
import os
from pathlib import Path
import re
import struct
import time
import urllib.parse
import urllib.request

from transport import Unavailable, decode, encode


def integer(value):
    if not isinstance(value, str) or not re.fullmatch(r'0|[1-9][0-9]*', value):
        raise Unavailable('NON_CANONICAL_INTEGER')
    result = int(value)
    if result > 2**64 - 1:
        raise Unavailable('INTEGER_OVERFLOW')
    return result


def loopback_url(value):
    url = urllib.parse.urlsplit(value)
    try:
        valid = (url.scheme == 'http' and ipaddress.ip_address(url.hostname).is_loopback
                 and url.port is not None and not url.username and not url.password
                 and not url.query and not url.fragment and url.path in ('', '/'))
    except (ValueError, TypeError):
        valid = False
    if not valid:
        raise ValueError('literal loopback HTTP RPC URL required')
    return value


class NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, *args, **kwargs):
        raise Unavailable('RPC_REDIRECT')


class RPC:
    def __init__(self, url):
        self.url = loopback_url(url)
        self.opener = urllib.request.build_opener(urllib.request.ProxyHandler({}), NoRedirect())

    def call(self, method, params):
        data = encode({'jsonrpc': '2.0', 'id': 1, 'method': method, 'params': params})
        request = urllib.request.Request(self.url, data, {'Content-Type': 'application/json'})
        try:
            started = time.monotonic()
            with self.opener.open(request, timeout=2) as response:
                raw = response.read(2 * 1024 * 1024 + 1)
            if len(raw) > 2 * 1024 * 1024 or time.monotonic() - started > 2:
                raise Unavailable('RPC_LIMIT')
            value = decode(raw)
            if value.get('id') != 1 or value.get('jsonrpc') != '2.0' or 'error' in value:
                raise Unavailable('RPC_ERROR')
            return value['result'], raw
        except (OSError, ValueError, KeyError, TypeError) as exc:
            raise Unavailable('RPC_UNAVAILABLE') from exc


def json_response(raw):
    """Decode exactly QueryJSONResponse field 1; reject trailing/nonminimal wire."""
    if not raw or raw[0] != 10:
        raise Unavailable('ABCI_WIRE')
    size = 0
    for i in range(1, min(len(raw), 6)):
        byte = raw[i]
        size |= (byte & 127) << (7 * (i - 1))
        if byte < 128:
            if (i > 1 and byte == 0) or i + 1 + size != len(raw):
                break
            return decode(raw[i + 1:])
    raise Unavailable('ABCI_WIRE')


def unix_ms(value):
    if not re.fullmatch(r'\d{4}-\d\d-\d\dT\d\d:\d\d:\d\d(?:\.\d{1,9})?Z', value):
        raise Unavailable('BLOCK_TIME')
    stamp = datetime.datetime.fromisoformat(value.replace('Z', '+00:00'))
    delta = stamp - datetime.datetime(1970, 1, 1, tzinfo=datetime.timezone.utc)
    return (delta.days * 86400 + delta.seconds) * 1000 + delta.microseconds // 1000


def durable_file(path, data):
    """Content addressed immutable evidence, fsynced before engine observation."""
    path = Path(path)
    try:
        with path.open('xb') as file:
            file.write(data)
            file.flush()
            os.fsync(file.fileno())
    except FileExistsError:
        if path.read_bytes() != data:
            raise Unavailable('EVIDENCE_CONFLICT')
    fd = os.open(path.parent, os.O_RDONLY)
    try:
        os.fsync(fd)
    finally:
        os.close(fd)


class Collector:
    def __init__(self, rpc, engine, manifest, evidence):
        self.rpc, self.engine, self.manifest = rpc, engine, manifest
        self.evidence = Path(evidence)
        self.evidence.mkdir(parents=True, exist_ok=True)
        self.fatal = None
        self.last_error = None

    def fetch(self, height):
        """Verify raw ABCI/header identity; Rust additionally verifies economics."""
        query, raw_query = self.rpc.call('abci_query', {
            'path': '/nus.exchange.v1.Query/Snapshot', 'data': '',
            'height': str(height), 'prove': False})
        response = query['response']
        if (response['height'] != str(height) or response.get('code', 0) != 0):
            raise Unavailable('SNAPSHOT_UNAVAILABLE')
        snapshot = json_response(base64.b64decode(response['value'], validate=True))
        body = snapshot['body']
        raw = encode(body)
        domain = b'NUS/S2/SNAPSHOT/V1'
        digest = hashlib.sha256(struct.pack('>I', len(domain)) + domain
                                + struct.pack('>Q', len(raw)) + raw).hexdigest()
        if (snapshot['snapshot_id'] != digest or body['context'] != self.manifest['context']
                or body['market'] != self.manifest['market']
                or body['observed_height'] != str(height)
                or [a['owner'] for a in body['accounts']] != self.manifest['owners']
                or [s['genesis_supply_atoms'] for s in body['supplies']] != self.manifest['supplies']):
            raise Unavailable('SNAPSHOT_BINDING')
        block, raw_block = self.rpc.call('block', {'height': str(height)})
        header = block['block']['header']
        if (header['height'] != str(height)
                or header['chain_id'] != self.manifest['context']['chain_id']
                or block['block_id']['hash'].lower() != body['block_hash']
                or str(unix_ms(header['time'])) != body['block_time_unix_ms']):
            raise Unavailable('BLOCK_BINDING')
        evidence = encode({'height': str(height), 'snapshot_id': digest,
                           'abci_response_base64': base64.b64encode(raw_query).decode(),
                           'block_response_base64': base64.b64encode(raw_block).decode()})
        evidence_hash = hashlib.sha256(evidence).hexdigest()
        durable_file(self.evidence / f'{height}-{evidence_hash}.json', evidence)
        return snapshot

    def tick(self):
        """Bounded pass. Reobserve persisted H, then consume every H+1 in order."""
        try:
            if self.fatal:
                raise Unavailable(self.fatal)
            started = time.monotonic()
            status, body = self.engine.request('GET', '/s2/status')
            if status != 200:
                raise Unavailable('ENGINE_STATUS')
            cursor = integer(body['observation']['observed_height'])
            network, _ = self.rpc.call('status', {})
            if network['node_info']['network'] != self.manifest['context']['chain_id']:
                raise Unavailable('RPC_CHAIN_MISMATCH')
            tip = integer(network['sync_info']['latest_block_height'])
            catching_up = network['sync_info']['catching_up']
            if not isinstance(catching_up, bool):
                raise Unavailable('RPC_STATUS_SCHEMA')
            if tip < cursor:
                self.fatal = 'HEIGHT_REGRESSION'
                self.engine.rpc_failed()
                # A verified historical snapshot lets the sequencer persist its
                # own recovery gate/reason; never fabricate a lower snapshot.
                if tip > 0:
                    snapshot = self.fetch(tip)
                    self.engine.call({
                        'op': 'observe', 'snapshot': snapshot, 'cursor_height': str(tip),
                        'received_at_unix_ms': str(time.time_ns() // 1_000_000),
                        'query_latency_ms': '0', 'catching_up': True})
                raise Unavailable(self.fatal)
            # Close before fetching a known backlog, not after the final response.
            if tip > cursor or catching_up:
                self.engine.rpc_failed()
            # At most eight heights per pass, so shutdown/HTTP remain responsive.
            for height in range(cursor, min(tip, cursor + 7) + 1):
                if height != cursor:
                    started = time.monotonic()
                snapshot = self.fetch(height)
                received = time.time_ns() // 1_000_000
                latency = (time.monotonic_ns() // 1_000_000) - int(started * 1000)
                status, result = self.engine.call({
                    'op': 'observe', 'snapshot': snapshot, 'cursor_height': str(height),
                    'received_at_unix_ms': str(received), 'query_latency_ms': str(latency),
                    'catching_up': catching_up or height < tip})
                if status != 200:
                    raise Unavailable(result.get('code', 'OBSERVE_FAILED'))
            self.last_error = None
            return True
        except (Unavailable, OSError, ValueError, TypeError, KeyError) as exc:
            self.last_error = str(exc) if isinstance(exc, Unavailable) else 'SNAPSHOT_UNAVAILABLE'
            self.engine.rpc_failed()
            return False
