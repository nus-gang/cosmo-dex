import base64
import hashlib
import json
import unittest
from unittest.mock import patch
from manifest import COMPONENTS, aggregate, encode
from test_preflight import PreflightFixture
import review_subject


class ReviewSubjectTest(PreflightFixture, unittest.TestCase):
    def setUp(self):
        super().setUp()
        for name in COMPONENTS:
            path = self.manifest['components'][name]
            d = json.loads((self.bundle/'files'/path).read_bytes())
            d['implementation_settings'].update({
                'build_argv_json': encode(['TEST_ONLY', '--locked']).decode(),
                'toolchain': 'TEST_ONLY',
                'approval_sources_json': encode(['TEST_ONLY_NOT_APPROVAL']).decode(),
                'implementation_locks_json': encode({'TEST_ONLY': 'c'*64}).decode()})
            self.put(path, encode(d))
        self.manifest['contract_sha256'] = aggregate(self.hashes)
        self.seal()

    def subject(self):
        return review_subject.subject(self.bundle, self.artifacts, self.pin,
                                      's3-dev-local/1', True)

    def test_exact_raw_manifest_descriptors_and_inventory(self):
        raw = self.subject()
        value = json.loads(raw)
        self.assertEqual(base64.b64decode(value['manifest_base64']),
                         (self.bundle/'runtime-manifest.json').read_bytes())
        for name, component in value['components'].items():
            self.assertEqual(base64.b64decode(component['descriptor_base64']),
                (self.bundle/'files'/self.manifest['components'][name]).read_bytes())
            self.assertEqual(component['build_argv'], ['TEST_ONLY', '--locked'])
            self.assertEqual(component['artifacts_sha256'][self.binary.name],
                             hashlib.sha256(self.binary.read_bytes()).hexdigest())
        report = review_subject.compare(raw, raw, hashlib.sha256(raw).hexdigest())
        self.assertTrue(report['subject_byte_match'])
        self.assertFalse(report['approval_verified'])
        self.assertFalse(report['services_started'])

    def test_byte_change_and_self_asserted_pin_cannot_match_old_subject(self):
        raw = self.subject()
        digest = hashlib.sha256(raw).hexdigest()
        for other, pin in [(raw+b' ', digest), (raw, '0'*64),
                           (raw+b' ', hashlib.sha256(raw+b' ').hexdigest())]:
            with self.assertRaises(ValueError): review_subject.compare(raw, other, pin)
        self.binary.write_bytes(b'CHANGED')
        with self.assertRaises(ValueError): self.subject()

    def test_manifest_or_descriptor_swap_after_verify_is_rejected(self):
        original = review_subject.verify
        for target in [self.bundle/'runtime-manifest.json',
                       self.bundle/'files'/self.manifest['components']['sre']]:
            before = target.read_bytes()
            def swap(*args):
                result = original(*args)
                target.write_bytes(before+b' ')
                return result
            with patch.object(review_subject, 'verify', side_effect=swap):
                with self.assertRaises(ValueError): self.subject()
            target.write_bytes(before)

    def test_same_semantics_different_manifest_bytes_require_new_subject(self):
        old = self.subject()
        path = self.bundle/'runtime-manifest.json'
        path.write_bytes(path.read_bytes()+b' ')
        self.pin = hashlib.sha256(path.read_bytes()).hexdigest()
        new = self.subject()
        with self.assertRaises(ValueError):
            review_subject.compare(new, old, hashlib.sha256(old).hexdigest())

    def test_missing_build_metadata_rejected_even_with_valid_manifest(self):
        path = self.manifest['components']['sre']
        descriptor = json.loads((self.bundle/'files'/path).read_bytes())
        descriptor['implementation_settings']['build_argv_json'] = '[]'
        self.put(path, encode(descriptor))
        self.manifest['contract_sha256'] = aggregate(self.hashes)
        self.seal()
        with self.assertRaises(ValueError): self.subject()

if __name__ == '__main__': unittest.main()
