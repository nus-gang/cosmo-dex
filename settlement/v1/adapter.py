"""S0 in-process REST/WS contract adapter. No chain trust or persistence."""
import base64
import copy
import json
import re
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
SCHEMA = json.loads((ROOT / 'protocol/v1/schema.json').read_text())

# Shared lexical rules for the validator and generated JSON Schema.
IDENTIFIER_PATTERN = r'[A-Za-z0-9._:/-]{1,128}'
ORIGIN_PATTERN = r'https://[a-z0-9]+(?:[.-][a-z0-9]+)*(?::(?!443$)(?:[1-9][0-9]{0,3}|[1-5][0-9]{4}|6[0-4][0-9]{3}|65[0-4][0-9]{2}|655[0-2][0-9]|6553[0-5]))?'

def string_schema(field):
    if field == 'audience':
        return {'type':'string', 'enum':['exchange-api','private-ws']}
    pattern = ORIGIN_PATTERN if field == 'server_origin' else IDENTIFIER_PATTERN
    rule = {'type':'string', 'pattern':'^' + pattern + r'$(?![\s\S])'}
    if field == 'server_origin':
        rule['x-origin-policy'] = 'nondefault port 1..65535; exact deployment allowlist required'
    return rule

def string_value(field, value):
    if not isinstance(value, str):
        raise ValueError('STRING')
    if field == 'audience':
        if value not in ('exchange-api','private-ws'):
            raise ValueError('AUDIENCE')
    elif field == 'server_origin':
        if not re.fullmatch(ORIGIN_PATTERN, value):
            raise ValueError('ORIGIN')
        authority = value[len('https://'):]
        if ':' in authority:
            port = int(authority.rsplit(':',1)[1])
            if port == 443 or port > 65535:
                raise ValueError('ORIGIN')
    elif not re.fullmatch(IDENTIFIER_PATTERN, value):
        raise ValueError('STRING')
    return value

def integer(value, bits=64):
    if not isinstance(value, str) or not re.fullmatch(r'0|[1-9][0-9]*', value) or len(value) > 39 or int(value) >= 2**bits:
        raise ValueError('INTEGER_RANGE')
    return int(value)

def strict_json(raw):
    def pairs(items):
        obj = {}
        for k, v in items:
            if k in obj:
                raise ValueError('DUPLICATE_KEY')
            obj[k] = v
        return obj
    return json.loads(raw, object_pairs_hook=pairs)

def validate(name, obj):
    fields = SCHEMA[name]
    if not isinstance(obj, dict) or set(obj) != {f['name'] for f in fields}:
        raise ValueError('FIELDS')
    for f in fields:
        value = obj[f['name']]
        values = value if f['repeated'] else [value]
        if f['repeated'] and not isinstance(value, list):
            raise ValueError('ARRAY')
        for v in values:
            t = f['type']
            if t in ('u32', 'u64', 'atoms'):
                integer(v, 128 if t == 'atoms' else int(t[1:]))
            elif t == 'h':
                if not isinstance(v, str) or not re.fullmatch('[0-9a-f]{64}', v):
                    raise ValueError('HASH')
            elif t in ('a', 'pk', 'sig'):
                if not isinstance(v, str):
                    raise ValueError('BASE64')
                b = base64.b64decode(v, validate=True)
                if len(b) != {'a':20, 'pk':1952, 'sig':3309}[t] or base64.b64encode(b).decode() != v:
                    raise ValueError('BASE64')
            elif t == 's':
                string_value(f['name'], v)
            else:
                validate(t, v)
    if 'protocol_version' in obj and obj['protocol_version'] != '1':
        raise ValueError('UNSUPPORTED_VERSION')

KEYS = ('chain_id', 'genesis_hash', 'market_id', 'batch_seq')
def key(r):
    return tuple(r[k] for k in KEYS)

def decision(last_seq, seq, stored_hash, submitted_hash, stored_id=None, submitted_id=None):
    last, n = integer(last_seq), integer(seq)
    if stored_hash is not None:
        return 'ALREADY_COMMITTED' if (stored_hash, stored_id) == (submitted_hash, submitted_id) else 'BATCH_CONFLICT'
    if n <= last:
        return 'RECEIPT_INCONSISTENCY'
    return 'CHECK_NEW_BATCH' if n == last + 1 else 'BATCH_SEQUENCE_GAP'

class MockAPI:
    """Seeded receipts are trusted synthetic finalized chain observations only."""
    def __init__(self, receipts, observed_height='100', indexer_height='100'):
        self.receipts = {}
        self.inconsistencies = set()
        for r in receipts:
            validate('BatchReceiptV1', r)
            k = key(r)
            if k in self.receipts and self.receipts[k] != r:
                raise ValueError('BATCH_CONFLICT')
            self.receipts[k] = copy.deepcopy(r)
        self.height = str(integer(observed_height))
        self.indexer = str(integer(indexer_height))
        if int(self.indexer) > int(self.height):
            raise ValueError('HEIGHT')

    def lookup(self, chain_id, genesis_hash, market_id, batch_seq, *, available=True, last_seq=None):
        string_value("chain_id", chain_id)
        string_value("market_id", market_id)
        integer(batch_seq)
        if not re.fullmatch('[0-9a-f]{64}', genesis_hash):
            raise ValueError('HASH')
        r = self.receipts.get((chain_id, genesis_hash, market_id, batch_seq)) if available else None
        k = (chain_id, genesis_hash, market_id, batch_seq)
        if last_seq is not None:
            last = integer(last_seq)
            if available and r is None and int(batch_seq) <= last:
                self.inconsistencies.add(k)
        code = ('RECEIPT_INCONSISTENCY' if k in self.inconsistencies else
                'COMMITTED' if r else ('NOT_FOUND_AT_HEIGHT' if available else 'LOOKUP_UNAVAILABLE'))
        if code == 'RECEIPT_INCONSISTENCY':
            r = None
        return {'code':code, 'retryable':code not in ('COMMITTED','RECEIPT_INCONSISTENCY'), 'state':'COMMITTED' if r else 'SUBMISSION_UNKNOWN',
                'height':self.height, 'observed_height':self.height, 'indexer_height':self.indexer,
                'stale':self.indexer != self.height, 'receipt':copy.deepcopy(r)}

    def retry(self, submitted, last_seq, *, available=True, authorized=True, previous_matches=True, duplicate_fill=False):
        # Chain codec/context/current TX authorization MUST precede this adapter in production.
        validate('BatchReceiptV1', submitted)
        if not authorized:
            return 'UNAUTHORIZED'
        if key(submitted) in self.inconsistencies:
            return 'RECEIPT_INCONSISTENCY'
        if not available:
            return 'SUBMISSION_UNKNOWN'
        r = self.receipts.get(key(submitted))
        result = decision(last_seq, submitted['batch_seq'], r['batch_hash'] if r else None,
                          submitted['batch_hash'], r['batch_id'] if r else None, submitted['batch_id'])
        if result == 'RECEIPT_INCONSISTENCY':
            self.inconsistencies.add(key(submitted))
        if result != 'CHECK_NEW_BATCH':
            return result
        if not previous_matches:
            return 'PREVIOUS_BATCH_HASH_MISMATCH'
        if duplicate_fill:
            return 'DUPLICATE_FILL'
        return 'CHECK_NEW_BATCH'

def reconcile(lookup, *, rejected_final=False, inflight_resolved=False, replay_complete=False):
    if lookup['code'] == 'RECEIPT_INCONSISTENCY':
        return {'state':'SUBMISSION_UNKNOWN', 'release_D_P':False, 'new_id_allowed':False}
    if lookup['code'] == 'COMMITTED':
        return {'state':'COMMITTED', 'release_D_P':True, 'new_id_allowed':False}
    corrected = rejected_final and inflight_resolved and replay_complete
    return {'state':'CORRECTED' if corrected else 'SUBMISSION_UNKNOWN',
            'release_D_P':corrected, 'new_id_allowed':False}

def available(confirmed, reserved, debit, provisional):
    c,r,d,p = [integer(x,128) for x in (confirmed,reserved,debit,provisional)]
    if c < r+d:
        raise ValueError('INSUFFICIENT_CONFIRMED_BALANCE')
    return str(c-r-d)  # P is never spendable.

class EventConsumer:
    def __init__(self):
        self.entities = {}
    def apply(self, event):
        integer(event['revision']); integer(event['observed_height'])
        if event['state'] not in ('PENDING','COMMITTED','CORRECTED','SUBMISSION_UNKNOWN'):
            raise ValueError('STATE')
        old = self.entities.get(event['entity_id'])
        if old and int(event['revision']) <= int(old['revision']):
            if event['revision'] == old['revision'] and event != old:
                raise ValueError('REVISION_CONFLICT')
            return False
        if old and old['state'] in ('COMMITTED','CORRECTED') and event['state'] != old['state']:
            raise ValueError('TERMINAL_STATE')
        self.entities[event['entity_id']] = copy.deepcopy(event)
        return True

class Attempt:
    """Synthetic immutable payload binding across transport failures; no TX signing."""
    def __init__(self, batch_id, canonical_bytes):
        self.batch_id = batch_id
        self.payload = bytes(canonical_bytes)
        self.state = 'PENDING'
    def timeout(self):
        self.state = 'SUBMISSION_UNKNOWN'
    def retry(self, batch_id, canonical_bytes, *, lookup_completed):
        if not lookup_completed or batch_id != self.batch_id or bytes(canonical_bytes) != self.payload:
            raise ValueError('RETRY_BINDING')
        return self.payload
