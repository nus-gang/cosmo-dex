import copy
import json
import unittest
from unittest.mock import Mock, patch

import runtime_client as api
from test_paperclip_reader import Response, BASE
from test_managed_session import SessionTest

WID = '11111111-1111-4111-8111-111111111111'
RUN = '22222222-2222-4222-8222-222222222222'
LIST = f'/api/projects/{api.PROJECT}/workspaces'


class RuntimeClientTest(unittest.TestCase):
    def client(self, fee=0):
        client = api.RuntimeClient(BASE + '/api/', 'synthetic-token', RUN, WID, f's3-worker-fee{fee}')
        client._opener = Mock()
        return client

    def packet(self, action='start', fee=0):
        return {'method': 'POST', 'path': LIST + f'/{WID}/runtime-services/{action}',
                'body': {'workspaceCommandId': f's3-worker-fee{fee}'}}

    def response(self, path=LIST, value=None, **kw):
        return Response(json.dumps([] if value is None else value).encode(), url=BASE+path, **kw)

    def test_scoped_get_and_post_headers_body(self):
        for fee in (0,25):
            client = self.client(fee)
            client._opener.open.return_value = self.response()
            self.assertEqual(client.read_workspaces(LIST), [])
            for action in ('start','stop'):
                packet = self.packet(action, fee)
                client._opener.open.return_value = self.response(packet['path'], {'operation': {}})
                self.assertEqual(client.request(packet), {'operation': {}})
                req = client._opener.open.call_args.args[0]
                self.assertEqual(req.get_method(), 'POST')
                self.assertEqual(req.full_url, BASE + packet['path'])
                self.assertEqual(json.loads(req.data), packet['body'])
                self.assertEqual(req.get_header('X-paperclip-run-id'), RUN)
                self.assertEqual(req.get_header('Authorization'), 'Bearer synthetic-token')
                self.assertEqual(client._opener.open.call_args.kwargs, {'timeout':5})

    def test_reject_escape_selectors_and_registration_before_io(self):
        client = self.client()
        for field, value in [('method','GET'),('path',LIST),('path',self.packet()['path']+'?x=1'),
                             ('path',self.packet()['path'].replace(WID,RUN)),
                             ('path',self.packet()['path'].replace('start','restart')),
                             ('body',{}),('body',{'workspaceCommandId':'s3-worker-fee25'}),
                             ('body',{'workspaceCommandId':'s3-worker-fee0','serviceIndex':0})]:
            packet = self.packet(); packet[field] = value
            with self.assertRaisesRegex(ValueError, '^RUNTIME_CONTROL_PATH$'): client.request(packet)
        with self.assertRaises(ValueError): client.read_workspaces(LIST+'/../secrets')
        client._opener.open.assert_not_called()
        for base, run, wid, command in [('http://external:80',RUN,WID,'s3-worker-fee0'),
            (BASE,'bad',WID,'s3-worker-fee0'),(BASE,RUN,'../x','s3-worker-fee0'),
            (BASE,RUN,WID,'web')]:
            with self.assertRaisesRegex(ValueError, '^RUNTIME_CONTROL_CONFIG$'):
                api.RuntimeClient(base,'synthetic-token',run,wid,command)

    def test_bad_response_no_retry_or_secret_diagnostic(self):
        responses = [self.response(status=302), self.response(mime='text/plain'),
                     self.response(value={}), Response(b'{"x":1,"x":2}',url=BASE+LIST),
                     self.response(path=LIST+'/elsewhere')]
        for response in responses:
            client = self.client(); client._opener.open.return_value = response
            with self.assertRaisesRegex(ValueError, '^'+api.ERROR+'$'): client.read_workspaces(LIST)
            self.assertEqual(client._opener.open.call_count,1)
        client = self.client(); client._opener.open.side_effect = OSError('synthetic-token private body')
        with self.assertRaisesRegex(ValueError, '^'+api.ERROR+'$'): client.request(self.packet())
        self.assertEqual(client._opener.open.call_count,1)
        for setting in ('size','time'):
            client = self.client(); client._opener.open.return_value = self.response()
            context = patch.object(api,'MAX_RESPONSE',1) if setting=='size' else patch.object(api.time,'monotonic',side_effect=[0,11])
            with context, self.assertRaisesRegex(ValueError, '^'+api.ERROR+'$'): client.read_workspaces(LIST)

    def test_session_lost_start_response_still_targets_stop_once(self):
        case = SessionTest(); case.setup_case()
        client = self.client()
        listing = lambda: self.response(value=[copy.deepcopy(case.workspace)])
        client._opener.open.side_effect = [listing(), listing(), OSError('lost secret'),
                                          self.response(self.packet('stop')['path'], {'operation':{}})]
        case.kw.update(read_workspaces=client.read_workspaces, request=client.request)
        with self.assertRaisesRegex(ValueError, '^MANAGED_SESSION_REJECTED$'):
            with case.run_session(): self.fail()
        requests = [call.args[0] for call in client._opener.open.call_args_list]
        self.assertEqual([r.get_method() for r in requests], ['GET','GET','POST','POST'])
        self.assertEqual([r.full_url.rsplit('/',1)[1] for r in requests[-2:]], ['start','stop'])
        self.assertEqual(case.events[-1], 'broker-close')


if __name__ == '__main__': unittest.main()
