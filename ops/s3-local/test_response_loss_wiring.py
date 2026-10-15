import tempfile
from pathlib import Path
import contextlib
import hashlib
import unittest
from unittest.mock import patch
import managed_web
import web_cli
import test_managed_web
import test_web_cli
import test_response_loss
from response_loss import BroadcastResponseLoss


class WiringTests(unittest.TestCase):
    def test_auth_and_result_survive_single_broadcast_loss(self):
        helper = test_response_loss.ResponseLossTests()
        calls = []
        def upstream(dest, raw):
            calls.append(raw.partition(b'\r\n')[0])
            return 200, [('Content-Type','application/json'),('Content-Length','2')], b'{}'
        routed = BroadcastResponseLoss(upstream, destination=helper.destination,
            body_sha256=hashlib.sha256(helper.body).hexdigest(),
            enable_local_demo=True, allow_unproven_host_space=True)
        for route, status in [('auth/challenge',200),('auth/session',200),
                             ('chain/broadcast',503),('chain/result',200),
                             ('chain/broadcast',503),('chain/result',200)]:
            self.assertEqual(helper.request(routed, route)[0], status)
        self.assertEqual(sum(b'chain/broadcast' in row for row in calls),1)
        self.assertTrue(routed.report()['response_discarded'])
        self.assertFalse(routed.report()['chain_effect_verified'])
        before = len(calls)
        for raw in (b'GET /dev-local/v1/chain/broadcast HTTP/1.1\r\n',
                    b'POST /dev-local/v1/chain/broadcast?x HTTP/1.1\r\n'):
            with self.assertRaises(ValueError): routed(helper.destination, raw)
        self.assertEqual(len(calls), before)

    def test_lifecycle_uses_router_and_default_stays_disabled(self):
        fixture = test_managed_web.ManagedWebTest()
        fixture.setUp(); self.addCleanup(fixture.doCleanups)
        def serve(listener, proxy, exchange, **kwargs):
            self.assertIsInstance(exchange, BroadcastResponseLoss)
            listener.close()
            return dict(listener_closed=True, approval_verified=False, handled_connections=0)
        with patch.object(managed_web,'serve_listener',side_effect=serve):
            with tempfile.TemporaryDirectory() as root:
                result = fixture.run_web(response_loss_sha256='a'*64, fault_evidence_root=Path(root).resolve())
        self.assertFalse(result['response_loss']['used'])
        self.assertFalse(result['reusable_permit'])
        fixture.listener.close.assert_called_once()
        fixture.factory.reset_mock()
        for value in ('A'*64, '', 1):
            with self.assertRaises(ValueError): fixture.run_web(response_loss_sha256=value)
        fixture.factory.assert_not_called()
        with patch.object(managed_web,'serve_listener',return_value={}) as serve:
            result = fixture.run_web()
            self.assertNotIsInstance(serve.call_args.args[2], BroadcastResponseLoss)
            self.assertNotIn('response_loss',result)

    def test_cli_passes_only_explicit_fault_and_rejects_bad_inputs(self):
        fixture = test_web_cli.WebCliTest()
        fixture.setUp(); self.addCleanup(fixture.doCleanups)
        option = '--drop-broadcast-response-sha256'
        result = dict(listener_closed=True, approval_verified=False, reusable_permit=False,
                      handled_connections=0, capture_sha256='a'*64, validator_sha256='b'*64)
        with patch.object(web_cli,'private_transport',return_value=contextlib.nullcontext()), \
             patch.object(web_cli,'run',return_value=result) as run:
            self.assertEqual(fixture.invoke(fixture.args()+[option,'c'*64,'--fault-evidence-root','/private/evidence']),(0,'',''))
            self.assertEqual(run.call_args.kwargs['response_loss_sha256'],'c'*64)
            self.assertNotIn(option,run.call_args.args[9])
            self.assertEqual(fixture.invoke(fixture.args()),(0,'',''))
            self.assertIsNone(run.call_args.kwargs['response_loss_sha256'])
        cases = [[option], [option,'A'*64], [option,'a'*64,option,'b'*64],
                 [option+'=x'], ['--drop-broadcast-response','a'*64]]
        with patch.object(web_cli,'run') as run, patch.object(web_cli,'private_transport') as reader:
            for extra in cases:
                self.assertEqual(fixture.invoke(fixture.args()+extra),(2,'','LOCAL_MANAGED_WEB_REJECTED\n'))
            args = [a for a in fixture.args() if a != '--acknowledge-unproven-space']
            self.assertEqual(fixture.invoke(args+[option,'a'*64])[0],2)
            run.assert_not_called(); reader.assert_not_called()

    def test_mismatched_broadcast_closes_but_queries_remain_available(self):
        helper = test_response_loss.ResponseLossTests()
        calls=[]
        def exchange(*args):
            calls.append(args)
            return 200,[('Content-Type','application/json'),('Content-Length','2')],b'{}'
        routed=BroadcastResponseLoss(exchange,destination=helper.destination,body_sha256='0'*64,
                                    enable_local_demo=True,allow_unproven_host_space=True)
        self.assertEqual(helper.request(routed)[0],503)
        self.assertEqual(helper.request(routed)[0],503)
        self.assertEqual(calls,[])
        self.assertEqual(helper.request(routed,'chain/result')[0],200)
        self.assertEqual(len(calls),1)
        self.assertFalse(routed.report()['upstream_attempted'])
