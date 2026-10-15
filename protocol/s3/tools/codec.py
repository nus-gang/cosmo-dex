"""Fixture encoder. Strict hostile-input decoding belongs to language adapters."""
import base64
import hashlib
import json
from pathlib import Path

ROOT = Path(__file__).resolve().parents[3]
S3 = ROOT / 'protocol/s3'

def sha(data):
    return hashlib.sha256(data).hexdigest()

def canon(obj):
    return json.dumps(obj, sort_keys=True, ensure_ascii=True, separators=(',', ':')).encode()

def frame(domain, body):
    d = domain.encode('ascii')
    return len(d).to_bytes(4, 'big') + d + len(body).to_bytes(8, 'big') + body

def varint(n):
    assert 0 <= n < 2**64
    out = bytearray()
    while n > 127:
        out.append((n & 127) | 128)
        n >>= 7
    return bytes(out + bytes([n]))

def blob(tag, value):
    if isinstance(value, str):
        value = value.encode('ascii')
    return varint(tag * 8 + 2) + varint(len(value)) + value

def uint(tag, value, sdk=False):
    n = int(value)
    return b'' if sdk and n == 0 else varint(tag * 8) + varint(n)

def encode(fields):
    out = b''
    for tag, kind, value in fields:
        if kind in ('u32', 'u64'):
            assert 0 <= int(value) < 2**int(kind[1:])
            out += uint(tag, value)
        else:
            out += blob(tag, bytes.fromhex(value) if kind == 'hex' else value)
    return out

def b64(data):
    return base64.b64encode(data).decode()

def read(name):
    return json.loads((S3 / name).read_text())

def write(name, obj):
    (S3 / name).write_text(json.dumps(obj, ensure_ascii=False, indent=2) + '\n')

def address(raw):
    chars = 'qpzry9x8gf2tvdw0s3jn54khce6mua7l'
    data, acc, bits = [], 0, 0
    for x in raw:
        acc = (acc << 8) | x
        bits += 8
        while bits >= 5:
            bits -= 5
            data.append((acc >> bits) & 31)
    if bits:
        data.append((acc << (5-bits)) & 31)
    chk = 1
    for x in [ord(c) >> 5 for c in 'nus'] + [0] + [ord(c) & 31 for c in 'nus'] + data + [0]*6:
        top = chk >> 25
        chk = ((chk & 0x1ffffff) << 5) ^ x
        for i, g in enumerate([0x3b6a57b2,0x26508e6d,0x1ea119fa,0x3d4233dd,0x2a1462b3]):
            if (top >> i) & 1:
                chk ^= g
    chk ^= 1
    return 'nus1' + ''.join(chars[x] for x in data + [(chk >> (5*(5-i))) & 31 for i in range(6)])
