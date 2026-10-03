"""Real loopback HTTP acceptance driver; public test seeds stay in signer process."""
import http.client
import subprocess
import threading
from server import Server
from transport import decode, encode

ORIGIN = 'http://127.0.0.1:5173'

class Orders:
    def __init__(self, engine, manifest, signer, output, observe):
        self.server = Server(('127.0.0.1', 0), engine)
        self.thread = threading.Thread(target=self.server.serve_forever, kwargs={'poll_interval': .05})
        self.context, self.signer, self.output, self.observe = manifest['context'], signer, output, observe
        self.identities = [self.sign(i, 'identity') for i in range(2)]
        self.thread.start()
        try:
            self.tokens = [self.login(i) for i in range(2)]
        except BaseException:
            self.close()
            raise
        self.receipts = []
        self.fields = []
        self.hashes = []

    def close(self):
        self.server.shutdown()
        self.server.server_close()
        self.thread.join(timeout=5)

    def sign(self, user, kind, **kw):
        return decode(subprocess.run([self.signer], input=encode(dict(user=user, type=kind, **kw)),
                      capture_output=True, check=True, timeout=15).stdout)

    def request(self, method, path, body=None, token=None):
        conn = http.client.HTTPConnection('127.0.0.1', self.server.server_port, timeout=10)
        try:
            headers = {'Origin': ORIGIN, 'Content-Type': 'application/json'}
            if token:
                headers['Authorization'] = 'Bearer ' + token
            conn.request(method, path, None if body is None else encode(body), headers)
            response = conn.getresponse()
            return response.status, decode(response.read())
        finally:
            conn.close()

    def login(self, user):
        status, challenge = self.request('POST', '/s2/auth/challenges',
            dict(owner=self.identities[user]['owner'], origin=ORIGIN, audience='exchange-api'))
        assert status == 200, challenge
        signed = self.sign(user, 'WalletChallengeV1', wire_base64=challenge['wire_base64'])
        body = {k: signed[k] for k in ('wire_base64', 'signature_base64')}
        status, session = self.request('POST', '/s2/auth/sessions', body)
        assert status == 200, session
        assert self.request('POST', '/s2/auth/sessions', body)[0] == 401
        return session['token']

    def view(self, user, name):
        status, view = self.request('GET', '/s2/me', token=self.tokens[user])
        assert status == 200, view
        assert view['owner'] == self.identities[user]['owner'], view
        for row in view['ledger']:
            assert int(row['A']) == int(row['C']) - int(row['R']) - int(row['D']) >= 0
        (self.output / f'{name}-{user}.json').write_bytes(encode(view))
        return view

    def pending(self):
        for user, side, qty in ((0, '2', '2000'), (1, '1', '1000')):
            height = int(self.observe()['observation']['observed_height'])
            ident = self.identities[user]
            fields = dict(protocol_version='1', chain_id=self.context['chain_id'],
                genesis_hash=self.context['genesis_hash'], exchange_module_id='x/exchange',
                market_id='DEVBASE/DEVQUOTE', market_config_version='1', owner=ident['owner'],
                owner_pubkey=ident['public_key'], order_id=f'{100+user:064x}', owner_epoch='0',
                side=side, limit_price_ticks='10000', max_qty_lots=qty, max_fee_bps='0',
                fee_asset_policy_id='RECEIVE_ASSET_V1', expiry_height=str(height+100), order_type='1')
            signed = self.sign(user, 'OrderV1', fields=fields)
            body = dict(context=self.context, **{k: signed[k] for k in ('wire_base64', 'signature_base64')})
            status, receipt = self.request('POST', '/s2/orders', body, self.tokens[user])
            assert status == 201 and receipt['state'] == 'LOCAL_ACCEPTED', receipt
            # Treat the first response as lost, retry exact bytes and require the same receipt.
            assert self.request('POST', '/s2/orders', body, self.tokens[user]) == (200, receipt)
            path = '/s2/me/commands/ORDER/' + fields['order_id'] + '?epoch=0'
            assert self.request('GET', path, token=self.tokens[user]) == (200, receipt)
            assert self.request('GET', path, token=self.tokens[1-user])[0] == 404
            self.receipts.append((body, receipt, path))
            self.fields.append(fields)
            self.hashes.append(signed["hash"])
            (self.output / f'order-{user}.json').write_bytes(encode(dict(request=body, receipt=receipt)))
        views = [self.view(i, 'pending') for i in range(2)]
        for view in views:
            assert len(view['fills']) == 1 and view['fills'][0]['state'] == 'PENDING', view
            assert any(int(r['P']) > 0 for r in view['ledger'])
        assert views[0]['fills'][0]['fill_id'] == views[1]['fills'][0]['fill_id']
        self.fill_id = views[0]['fills'][0]['fill_id']
        height = int(self.observe()['observation']['observed_height'])
        fields = dict(protocol_version='1', chain_id=self.context['chain_id'],
            genesis_hash=self.context['genesis_hash'], market_id='DEVBASE/DEVQUOTE',
            owner=self.identities[0]['owner'], owner_epoch='0', order_id=self.fields[0]['order_id'],
            order_hash=self.hashes[0], cancel_nonce=f'{150:064x}', expiry_height=str(height+100),
            exchange_module_id='x/exchange')
        signed = self.sign(0, 'CancelV1', fields=fields)
        body = dict(context=self.context, **{k: signed[k] for k in ('wire_base64', 'signature_base64')})
        status, cancelled = self.request('POST', '/s2/cancels', body, self.tokens[0])
        assert status == 201 and cancelled['state'] == 'LOCAL_ACCEPTED', cancelled
        assert self.request('POST', '/s2/cancels', body, self.tokens[0]) == (200, cancelled)
        (self.output / 'cancel.json').write_bytes(encode(dict(request=body, receipt=cancelled)))
        for user in range(2):
            view = self.view(user, 'cancelled')
            assert all(r['R'] == '0' for r in view['ledger']), view
            assert any(int(r['D']) > 0 for r in view['ledger']), view
        # Price-limited IOC with an empty book releases all unfilled reservation.
        ioc = dict(self.fields[1], order_id=f'{151:064x}', order_type='2',
                   limit_price_ticks='9999', expiry_height=str(height+100))
        signed = self.sign(1, 'OrderV1', fields=ioc)
        body = dict(context=self.context, **{k: signed[k] for k in ('wire_base64', 'signature_base64')})
        status, result = self.request('POST', '/s2/orders', body, self.tokens[1])
        assert status == 201 and result['state'] == 'LOCAL_ACCEPTED', result
        view = self.view(1, 'ioc')
        assert all(r['R'] == '0' for r in view['ledger']) and len(view['fills']) == 1, view
        (self.output / 'ioc.json').write_bytes(encode(dict(request=body, receipt=result)))
        status, held = self.request('POST', '/s2/me/withdraw-prepare', {'request_id': f'{200:064x}'}, self.tokens[0])
        assert held['code'] == 'UNSETTLED_HOLD', held
        (self.output / 'withdraw-hold.json').write_bytes(encode(held))
        assert self.request('GET', '/s2/me')[0] == 401

    def corrected(self, name):
        for user in range(2):
            view = self.view(user, name)
            assert len(view['fills']) == 1 and view['fills'][0]['fill_id'] == self.fill_id
            assert view['fills'][0]['state'] == 'CORRECTED', view
            assert all(r['D'] == '0' and r['P'] == '0' and r['R'] == '0' for r in view['ledger']), view
            body, receipt, path = self.receipts[user]
            assert self.request('GET', path, token=self.tokens[user]) == (200, receipt)
            assert self.request('POST', '/s2/orders', body, self.tokens[user]) == (200, receipt)

    def restart(self, engine):
        self.server.engine = engine
        for token in self.tokens:
            assert self.request('GET', '/s2/me', token=token)[0] == 401
        self.tokens = [self.login(i) for i in range(2)]
        self.corrected('replayed')
