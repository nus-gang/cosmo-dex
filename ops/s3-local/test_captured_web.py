import base64
import hashlib
import os
from pathlib import Path
import tempfile
import unittest
from manifest import encode, COMPONENTS
from captured_web import capture_assets

class CapturedWebTest(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name).resolve()
        (self.root/'web').mkdir()
        self.assets = {'web/index.html': b'<html>synthetic</html>',
                       'web/page.js': b'// synthetic'}
        for path, raw in self.assets.items(): (self.root/path).write_bytes(raw)
        self.hashes = {p:hashlib.sha256(v).hexdigest() for p,v in self.assets.items()}

    def capture(self, inventories=None):
        inventories = inventories or [self.hashes] + [{}]*(len(COMPONENTS)-1)
        files, hashes = {}, {}
        for component, inventory in zip(COMPONENTS, inventories):
            path = 'chain/local-demo/components/' + component + '.json'
            raw = encode({'implementation_settings': {'artifacts_sha256_json': encode(inventory).decode()}})
            files[path] = base64.b64encode(raw).decode()
            hashes[path] = hashlib.sha256(raw).hexdigest()
        return encode({'files': files, 'runtime_manifest':base64.b64encode(encode({'files_sha256':hashes})).decode()})

    def test_served_bytes_survive_path_replacement(self):
        raw = self.capture()
        captured = capture_assets(raw, self.root)
        (self.root/'web/index.html').write_bytes(b'changed')
        static = captured.response_boundary('http://127.0.0.1:5173', None)
        self.assertEqual(static.respond('GET','/', [('Host','127.0.0.1:5173')], '127.0.0.1')[2], self.assets['web/index.html'])
        self.assertEqual(captured.capture_sha256, hashlib.sha256(raw).hexdigest())
        with self.assertRaises(ValueError): capture_assets(raw, self.root)

    def test_missing_conflicting_and_changed_descriptor(self):
        for inventories in ([{}]*len(COMPONENTS), [self.hashes, {'web/page.js':'0'*64}]+[{}]*(len(COMPONENTS)-2)):
            with self.assertRaises(ValueError): capture_assets(self.capture(inventories), self.root)
        import json
        value = json.loads(self.capture())
        path = next(iter(value['files']))
        value['files'][path] = base64.b64encode(b'{}').decode()
        with self.assertRaisesRegex(ValueError,'WEB_DESCRIPTOR_CHANGED'):
            capture_assets(encode(value),self.root)

    def test_links_and_oversize_rejected(self):
        raw = self.capture()
        path = self.root/'web/page.js'
        path.rename(self.root/'original')
        path.symlink_to(self.root/'original')
        with self.assertRaises(OSError): capture_assets(raw,self.root)
        path.unlink()
        os.link(self.root/'original',path)
        with self.assertRaises(ValueError): capture_assets(raw,self.root)
        path.unlink()
        path.write_bytes(b'x'*4194305)
        with self.assertRaises(ValueError): capture_assets(raw,self.root)

    def test_immutable_input_and_context_validation(self):
        with self.assertRaises(ValueError): capture_assets(bytearray(self.capture()),self.root)
        captured = capture_assets(self.capture(),self.root)
        with self.assertRaises(ValueError): captured.response_boundary('http://example.org',None)
        with self.assertRaises(ValueError): captured.response_boundary('http://localhost:5173',{'private_key':'x'})

if __name__ == '__main__': unittest.main()
