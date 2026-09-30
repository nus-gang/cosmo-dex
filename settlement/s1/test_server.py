import base64
import copy
import hashlib
import tempfile
import unittest
from server import Gateway, Unavailable, decode_json_response, field

GH = 'ab' * 32

class GatewayTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.db = self.temp.name + '/journal.db'
        self.g = Gateway('http://127.0.0.1:1', GH, self.db)
        self.meta = dict(chain_id='nus-s1-dev-1', observed_height='8', cursor_height='8', query_latency_ms='0',
                         indexer_mode='DIRECT_COMMITTED_QUERY', block_time='2026-01-01T00:00:00Z', freshness_ms='0')
        self.g.snapshot = lambda: self.meta
        self.raw = b'original signed TX bytes'
        self.encoded = base64.b64encode(self.raw).decode()
        self.digest = hashlib.sha256(self.raw).hexdigest().upper()

    def tearDown(self):
        self.temp.cleanup()

    def test_lost_response_persisted_and_restart(self):
        self.g.rpc = lambda *args: (_ for _ in ()).throw(Unavailable())
        for _ in range(2):
            status, data = self.g.handle('POST', '/s1/txs', {'tx_bytes': self.encoded})
            self.assertEqual((status, data['state'], data['tx_hash']), (202, 'SUBMISSION_UNKNOWN', self.digest))
        restarted = Gateway('http://127.0.0.1:1', GH, self.db)
        with restarted.connect() as c:
            self.assertEqual(c.execute('SELECT hash, raw FROM txs').fetchall(), [(self.digest, self.raw)])
        with self.assertRaises(ValueError):
            Gateway('http://127.0.0.1:1', 'cd' * 32, self.db)

    def test_checktx_never_committed(self):
        for code in [0, 32]:
            self.g.rpc = lambda *args: dict(hash=self.digest, code=code)
            self.assertEqual(self.g.handle('POST', '/s1/txs', {'tx_bytes': self.encoded})[1]['state'], 'SUBMISSION_UNKNOWN')

    def test_inclusion_and_failed_delivery(self):
        for code, height, state in [(0, '7', 'COMMITTED'), (5, '7', 'REJECTED_FINAL'), (0, '9', 'SUBMISSION_UNKNOWN')]:
            result = dict(code=code, gas_wanted='500000', gas_used='200000')
            def rpc(method, params):
                if method == 'block':
                    return dict(block=dict(header=dict(height=height, chain_id='nus-s1-dev-1'), data=dict(txs=[self.encoded])))
                if method == 'block_results':
                    return dict(height=height, txs_results=[result])
                return dict(tx=self.encoded, hash=self.digest, height=height, index=0, tx_result=result)
            self.g.rpc = rpc
            self.assertEqual(self.g.handle('GET', '/s1/txs/' + self.digest)[1]['state'], state)
        self.g.rpc = lambda *args: dict(tx=self.encoded, hash='00' * 32, height='7')
        with self.assertRaises(Unavailable):
            self.g.handle('GET', '/s1/txs/' + self.digest)

    def test_indexer_unavailable_stays_unknown(self):
        self.g.rpc = lambda *args: (_ for _ in ()).throw(Unavailable())
        self.assertEqual(self.g.handle('GET', '/s1/txs/' + self.digest)[1]['state'], 'SUBMISSION_UNKNOWN')

    def test_reject_envelope_and_noncanonical_base64(self):
        for body in [{'tx_bytes': self.encoded, 'sequence': '1'}, {'tx_bytes': ''}, {'tx_bytes': 'Zh=='}, {'tx_bytes': 3}, {'tx_bytes': base64.b64encode(b'a' * 16385).decode()}]:
            self.assertEqual(self.g.handle('POST', '/s1/txs', body)[0], 400)

    def test_snapshot_height_and_conservation_fail_closed(self):
        g = Gateway('http://127.0.0.1:1', GH, self.db)
        user = dict(owner='alice', account_number='0', sequence='0', epoch='0', bank_atoms='1000000000000', exchange_atoms='0', gas_atoms='1000000000')
        snapshot = dict(genesis_hash=GH, chain_id='nus-s1-dev-1', observed_height='8', state='COMMITTED',
            accounts=[user, dict(user, owner='bob')], module_atoms='0', quote_supply='2000000000000',
            operator_accounts=[], gas_collector_atoms='0', gas_supply='2000000000', genesis_gas_supply='2000000000')
        g.rpc = lambda *args: dict(block=dict(header=dict(height='8', chain_id='nus-s1-dev-1', time='2026-01-01T00:00:00Z')))
        g.query = lambda *args: (snapshot, '8')
        self.assertEqual(g.snapshot()['cursor_height'], '8')
        for key, value in [('observed_height', '7'), ('genesis_hash', 'cd' * 32), ('module_atoms', '1'), ('gas_supply', '2000000001')]:
            bad = copy.deepcopy(snapshot)
            bad[key] = value
            g.query = lambda *args: (bad, '8')
            with self.assertRaises(Unavailable):
                g.snapshot()

    def test_index_entry_without_block_inclusion_rejected(self):
        def rpc(method, params):
            if method == 'block':
                return dict(block=dict(header=dict(height='7', chain_id='nus-s1-dev-1'), data=dict(txs=[])))
            return dict(tx=self.encoded, hash=self.digest, height='7', index=0, tx_result=dict(code=0))
        self.g.rpc = rpc
        with self.assertRaises(Unavailable):
            self.g.handle('GET', '/s1/txs/' + self.digest)

    def test_query_response_codec(self):
        self.assertEqual(decode_json_response(field(1, '{"amount":"9007199254740993"}')),
                         {'amount': '9007199254740993'})
        with self.assertRaises(Unavailable):
            decode_json_response(b'\x0a\x02{}extra')

if __name__ == '__main__':
    unittest.main()
