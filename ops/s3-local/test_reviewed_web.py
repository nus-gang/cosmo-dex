import base64
import unittest
from unittest.mock import patch
from contextlib import ExitStack
from manifest import encode, decode
import reviewed_web
from test_captured_web import CapturedWebTest


class ReviewedWebTest(unittest.TestCase):
    def setUp(self):
        self.fixture = CapturedWebTest()
        self.fixture.setUp()
        self.addCleanup(self.fixture.doCleanups)
        self.context = dict(service_schema='s3/3', chain_id='nus-s3-dev-1',
            genesis_hash='a'*64, contract_hash='b'*64, config_hash='c'*64,
            market_id='DEVBASE/DEVQUOTE', market_config_version='1')
        value = decode(self.fixture.capture())
        value['guard'] = base64.b64encode(encode({'context':self.context})).decode()
        self.raw = encode(value)
        self.stack = ExitStack()
        self.addCleanup(self.stack.close)
        self.audit = self.stack.enter_context(patch.object(reviewed_web.approval_gate,
            'inspect', return_value={'candidate':'same'}))
        self.capture = self.stack.enter_context(patch.object(reviewed_web,
            'verify_input_set', return_value=(self.raw, {})))
        self.validate = self.stack.enter_context(patch.object(reviewed_web.offline_check,
            'validate_snapshot', return_value=('d'*64, {})))

    def prepare(self):
        return reviewed_web.prepare('bundle', self.fixture.root, 'pin',
            's3-dev-local/1', True, 'decision', {}, 'inputs', 'input.json',
            [], self.fixture.root, 'http://127.0.0.1:5173')

    def test_same_capture_context_and_immutable_assets(self):
        def validate(raw, *args):
            self.assertEqual(raw, self.raw)
            (self.fixture.root/'web/page.js').write_bytes(b'replaced')
            return 'd'*64, {}
        self.validate.side_effect = validate
        prepared = self.prepare()
        respond = prepared.response_boundary.respond
        headers = [('Host','127.0.0.1:5173')]
        self.assertEqual(respond('GET','/page.js',headers,'127.0.0.1')[2], b'// synthetic')
        self.assertEqual(decode(respond('GET','/runtime-context.json',headers,'127.0.0.1')[2]),self.context)
        self.assertEqual(self.audit.call_count,2)

    def test_initial_denial_has_no_capture_or_validator(self):
        self.audit.side_effect = ValueError('denied')
        with self.assertRaises(ValueError): self.prepare()
        self.capture.assert_not_called()
        self.validate.assert_not_called()

    def test_semantic_failure_interrupt_or_revocation_no_result(self):
        for failure in [ValueError('semantic'), KeyboardInterrupt()]:
            self.validate.side_effect = failure
            with self.assertRaises(type(failure)): self.prepare()
        self.validate.side_effect = None
        self.audit.side_effect = [{'candidate':'same'}, {'candidate':'changed'}]
        with self.assertRaisesRegex(ValueError,'APPROVAL_CHANGED'): self.prepare()

    def test_bad_assets_or_nonpublic_context_rejected(self):
        value = decode(self.raw)
        context = dict(self.context, private_key='forbidden')
        value['guard'] = base64.b64encode(encode({'context':context})).decode()
        self.capture.return_value = encode(value), {}
        with self.assertRaisesRegex(ValueError,'CONTEXT_FORMAT'): self.prepare()
        self.capture.return_value = self.raw, {}
        self.validate.reset_mock()
        (self.fixture.root/'web/index.html').write_bytes(b'changed')
        with self.assertRaisesRegex(ValueError,'WEB_ASSET_CHANGED'): self.prepare()
        self.validate.assert_not_called()

if __name__ == '__main__': unittest.main()
