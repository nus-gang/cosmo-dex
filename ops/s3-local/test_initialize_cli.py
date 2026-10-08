import hashlib
from pathlib import Path
from types import SimpleNamespace
import sys
import unittest
from unittest.mock import patch

import initialize_cli as target
from manifest import decode, encode, aggregate
import test_offline_check as fixtures


class InitializerTest(fixtures.PreflightFixture, unittest.TestCase):
    def setUp(self):
        super().setUp()
        fixtures.OfflineCheckTest.prepare(self)
        self.args = SimpleNamespace(bundle=self.bundle, artifacts=self.artifacts, runtime_pin=self.pin,
            local_demo_profile='s3-dev-local/1', effective_profile=self.root/'profile.json',
            user_public_keys=self.root/'users.json', output=self.root/'new-home', scratch=self.scratch,
            run_uuid='11111111-2222-4333-8444-555555555555', genesis_time='2027-01-15T08:00:00Z', fee_bps='0')
        self.args.effective_profile.write_bytes(b'profile')
        self.args.user_public_keys.write_bytes(b'public-only')
        # Synthetic subprocess protocol; never a product validation or pin.
        code = '#!'+sys.executable+'''
import sys, json, os
args=dict(zip(sys.argv[2:-1:2],sys.argv[3:-1:2]))
print('INITIALIZER_READY',flush=True)
if sys.stdin.buffer.read(9)!=b'PUBLISH\\n':sys.exit(2)
root=args['--output'];os.mkdir(root,0o700)
report=dict(schema='s3-local-initialization/1',root=root,runtime_pin=args['--runtime-pin'],fee_bps=args['--fee-bps'],service_started=False,c_semantic_validation_verified=True)
with open(root+'/initialization.json','w') as f:json.dump(report,f)
print(json.dumps(report),flush=True)
'''
        for component, name, content in [('chain', target.INITIALIZER, code.encode()),
                                           ('exchange', target.VALIDATOR, b'NOT_AN_EXECUTABLE_VALIDATOR')]:
            path=self.artifacts/name; path.write_bytes(content)
            desc='chain/local-demo/components/'+component+'.json'
            value=decode((self.bundle/'files'/desc).read_bytes())
            value['implementation_settings']['artifacts_sha256_json']=encode({name:hashlib.sha256(content).hexdigest()}).decode()
            self.put(desc,encode(value))
        self.manifest['contract_sha256']=aggregate(self.hashes)
        self.seal();self.args.runtime_pin=self.pin

    def test_captured_bytes_and_publication_after_recheck(self):
        with target.stage(self.args,lambda:{}) as (root,check,baseline):
            self.args.user_public_keys.write_bytes(b'replaced')
            self.assertEqual((root/'users.json').read_bytes(),b'public-only')
            result=target.run(self.args,root,check,baseline,lambda:{})
            self.assertFalse(result['service_started'])
            self.assertTrue(self.args.output.is_dir())
        self.assertEqual(list(self.scratch.iterdir()),[])

    def test_revocation_or_staged_mutation_prevents_publication(self):
        for mode in ('revoke','mutate','stop'):
            with self.subTest(mode=mode),target.stage(self.args,lambda:{}) as (root,check,baseline):
                calls=0
                def audit():
                    nonlocal calls
                    calls+=1
                    if calls==2:
                        if mode=='revoke':return {'revoked':True}
                        if mode=='mutate':(root/'profile.json').write_bytes(b'changed')
                    return {}
                with self.assertRaises(ValueError):
                    target.run(self.args,root,check,baseline,audit,lambda:mode=='stop')
                self.assertFalse(self.args.output.exists())
        self.assertEqual(list(self.scratch.iterdir()),[])

    def test_first_audit_denial_and_staging_change_cleanup(self):
        def deny(): raise ValueError('unapproved')
        with self.assertRaises(ValueError),target.stage(self.args,deny):self.fail('staged')
        self.assertEqual(list(self.scratch.iterdir()),[])
        calls=iter([{}, {'revoked':True}])
        with self.assertRaises(ValueError),target.stage(self.args,lambda:next(calls)):self.fail('staged')
        self.assertFalse(self.args.output.exists())
        self.assertEqual(list(self.scratch.iterdir()),[])

    def test_partial_output_is_preserved_and_no_retry(self):
        self.args.output.mkdir();(self.args.output/'evidence').write_bytes(b'keep')
        with target.stage(self.args,lambda:{}) as (root,check,baseline):
            with self.assertRaises(ValueError):target.run(self.args,root,check,baseline,lambda:{})
        self.assertEqual((self.args.output/'evidence').read_bytes(),b'keep')


if __name__=='__main__':unittest.main()
