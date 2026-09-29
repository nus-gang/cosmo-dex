import copy
import json
from pathlib import Path
import unittest
from adapter import *

HERE=Path(__file__).resolve().parent
class Contract(unittest.TestCase):
    def setUp(self):
        self.f=json.loads((HERE/'fixtures.json').read_text())
        self.r=self.f['receipts'][0]
        self.api=MockAPI([self.r], '100','99')
    def lookup(self, **kw):
        return self.api.lookup(*(self.r[k] for k in KEYS), **kw)
    def test_a_receipt_vectors(self):
        cases=json.loads((ROOT/'protocol/v1/vectors/s0-cases.json').read_text())['cases']
        for c in cases:
            if c['kind']=='receipt':
                with self.subTest(c['id']): self.assertEqual(decision(**c['input']),c['expected'])
    def test_historical_receipt_before_continuity(self):
        self.assertEqual(self.api.retry(self.r,'999'),'ALREADY_COMMITTED')
        self.assertEqual(self.lookup()['receipt'],self.r)
        self.assertTrue(self.lookup()['stale'])
        for field in ('batch_hash','batch_id'):
            bad=copy.deepcopy(self.r); bad[field]='ff'*32
            self.assertEqual(self.api.retry(bad,'999'),'BATCH_CONFLICT')
    def test_missing_history_is_inconsistency(self):
        self.api.receipts.clear()
        self.assertEqual(self.api.retry(self.r,'999'),'RECEIPT_INCONSISTENCY')
        self.assertEqual(self.lookup()['code'],'RECEIPT_INCONSISTENCY')

    def test_inconsistency_blocks_every_correction_combination(self):
        from itertools import product
        for source in ('retry', 'lookup'):
            api = MockAPI([])
            if source == 'retry':
                self.assertEqual(api.retry(self.r, '999'), 'RECEIPT_INCONSISTENCY')
            else:
                self.assertEqual(api.lookup(*(self.r[k] for k in KEYS), last_seq='999')['code'],
                                 'RECEIPT_INCONSISTENCY')
            for available_flag in (True, False):
                response = api.lookup(*(self.r[k] for k in KEYS), available=available_flag)
                self.assertEqual(response['code'], 'RECEIPT_INCONSISTENCY')
                self.assertFalse(response['retryable'])
                for flags in product((False, True), repeat=3):
                    with self.subTest(source=source, available=available_flag, flags=flags):
                        self.assertEqual(reconcile(response, rejected_final=flags[0],
                            inflight_resolved=flags[1], replay_complete=flags[2]),
                            {'state':'SUBMISSION_UNKNOWN','release_D_P':False,'new_id_allowed':False})
            other = dict(self.r, market_id='other')
            self.assertEqual(api.lookup(*(other[k] for k in KEYS))['code'], 'NOT_FOUND_AT_HEIGHT')
            api.receipts[key(self.r)] = self.r
            self.assertEqual(api.retry(self.r, '999'), 'RECEIPT_INCONSISTENCY')
            self.assertFalse(reconcile(api.lookup(*(self.r[k] for k in KEYS)))['release_D_P'])

    def test_string_rules_and_generated_schema(self):
        defs = json.loads((HERE/'api.schema.json').read_text())['$defs']
        cases = {
            'chain_id': (['a','A0._:/-' * 16, 'x'*128], ['', 'a b','x'*129,'é','x\n']),
            'server_origin': (['https://example.com','https://example.com:8443'],
                ['', 'http://example.com','https://EXAMPLE.com','https://example.com/',
                 'https://u@example.com','https://example.com:443','https://example.com:65536',
                 'https://example.com:01','https://example.com?x','https://example.com#x']),
            'audience': (['exchange-api','private-ws'], ['', 'other','exchange-api '])
        }
        for field, (good,bad) in cases.items():
            for value in good + bad:
                expected = value in good
                for name, fields in SCHEMA.items():
                    if not any(f['name']==field for f in fields):
                        continue
                    rule = defs[name]['properties'][field]
                    accepted = (value in rule['enum'] if 'enum' in rule else
                                re.search(rule['pattern'], value) is not None)
                    self.assertEqual(accepted, expected, (name,field,value))
                if expected:
                    self.assertEqual(string_value(field,value),value)
                else:
                    with self.assertRaises(ValueError): string_value(field,value)
        for field in ('chain_id','market_id'):
            for value in ('','a b','x'*129):
                bad = dict(self.r, **{field:value})
                with self.assertRaises(ValueError): validate('BatchReceiptV1',bad)
                with self.assertRaises(ValueError): self.api.lookup(*(bad[k] for k in KEYS))

    def test_context_isolation(self):
        for field in KEYS[:3]:
            bad=copy.deepcopy(self.r); bad[field]='ff'*32 if field=='genesis_hash' else 'other'
            self.assertIsNone(self.api.lookup(*(bad[k] for k in KEYS))['receipt'])
    def test_timeout_lookup_failure_and_not_found(self):
        for response in (self.lookup(available=False),MockAPI([]).lookup(*(self.r[k] for k in KEYS))):
            self.assertEqual(reconcile(response),{'state':'SUBMISSION_UNKNOWN','release_D_P':False,'new_id_allowed':False})
            for flags in ((False,True,True),(True,False,True),(True,True,False)):
                self.assertFalse(reconcile(response,rejected_final=flags[0],inflight_resolved=flags[1],replay_complete=flags[2])['release_D_P'])
            self.assertEqual(reconcile(response,rejected_final=True,inflight_resolved=True,replay_complete=True)['state'],'CORRECTED')
        self.assertEqual(self.api.retry(self.r,'999',available=False),'SUBMISSION_UNKNOWN')
        self.assertEqual(reconcile(self.lookup())['state'],'COMMITTED')
    def test_current_authorization_required(self):
        self.assertEqual(self.api.retry(self.r,'999',authorized=False),'UNAUTHORIZED')
        self.assertEqual(self.lookup()['code'],'COMMITTED')
    def test_new_sequence_and_partial_failure(self):
        self.api.receipts.clear(); r=copy.deepcopy(self.r); r['batch_seq']='10'
        self.assertEqual(self.api.retry(r,'9'),'CHECK_NEW_BATCH')
        self.assertEqual(self.api.retry(r,'8'),'BATCH_SEQUENCE_GAP')
        self.assertEqual(self.api.retry(r,'9',previous_matches=False),'PREVIOUS_BATCH_HASH_MISMATCH')
        self.assertEqual(self.api.retry(r,'9',duplicate_fill=True),'DUPLICATE_FILL')
        self.assertEqual(self.api.receipts,{}) # adapter cannot commit partial batches
    def test_balance(self):
        self.assertEqual(available('100','20','30','999999'),'50')
        with self.assertRaises(ValueError): available('1','0','2','100')
    def test_events(self):
        consumer=EventConsumer()
        for e in self.f['events']:
            self.assertTrue(consumer.apply(e)); self.assertFalse(consumer.apply(e))
        self.assertFalse(consumer.apply(self.f['events'][0]))
        bad=copy.deepcopy(self.f['events'][-1]); bad['state']='CORRECTED'
        with self.assertRaises(ValueError): consumer.apply(bad)
        bad['revision']='2'
        with self.assertRaises(ValueError): consumer.apply(bad)
    def test_integer_boundaries_and_duplicate_keys(self):
        for bits in (32,64,128):
            self.assertEqual(integer(str(2**bits-1),bits),2**bits-1)
            for v in ('01','-1','1e3',' 1',1,True,str(2**bits)):
                with self.assertRaises(ValueError): integer(v,bits)
        with self.assertRaises(ValueError): strict_json('{"amount":"1","amount":"2"}')
    def test_a_json_mapping(self):
        src=json.loads((ROOT/'protocol/v1/vectors/message-codec.json').read_text())
        for v in src['positives']: validate(v['message'],v['api_json'])
        bad=copy.deepcopy(self.r); bad['batch_seq']=1
        with self.assertRaises(ValueError): validate('BatchReceiptV1',bad)
        bad=copy.deepcopy(self.r); bad['unknown']='1'
        with self.assertRaises(ValueError): validate('BatchReceiptV1',bad)
        # API signature shape is checked; cryptographic verification belongs to C/F.
        with self.assertRaises(ValueError): validate('SignedOrderV1',{'order':{},'signature':'bad'})
    def test_rest_route_and_ws_fixture(self):
        from mock import rest
        r=self.r
        url=f"/v1/markets/{r['market_id']}/batches/{r['batch_seq']}?chain_id={r['chain_id']}&genesis_hash={r['genesis_hash']}"
        status,body=rest(self.api,url)
        self.assertEqual(status,200); self.assertEqual(body['receipt'],r)
        self.api.receipts.clear()
        self.assertEqual(rest(self.api,url)[0],404)
        with self.assertRaises(ValueError): rest(self.api,url+'&chain_id=other')

    def test_retry_keeps_a_canonical_bytes(self):
        vectors=json.loads((ROOT/'protocol/v1/vectors/batches.json').read_text())['batches']
        b=vectors[0]; raw=bytes.fromhex(b['canonical_or_candidate_hex'])
        attempt=Attempt(b['batch_id'],raw); attempt.timeout()
        self.assertEqual(attempt.state,'SUBMISSION_UNKNOWN')
        self.assertEqual(attempt.retry(b['batch_id'],raw,lookup_completed=True),raw)
        for bid,data,queried in [('ff'*32,raw,True),(b['batch_id'],raw+b'0',True),(b['batch_id'],raw,False)]:
            with self.assertRaises(ValueError): attempt.retry(bid,data,lookup_completed=queried)

    def test_schema_matches_a(self):
        defs=json.loads((HERE/'api.schema.json').read_text())['$defs']
        for name,fields in SCHEMA.items(): self.assertEqual(set(defs[name]['required']),{f['name'] for f in fields})

if __name__=='__main__': unittest.main(verbosity=2)
