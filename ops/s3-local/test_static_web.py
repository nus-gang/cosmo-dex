import unittest
from static_web import StaticWeb

class StaticTests(unittest.TestCase):
    def make(self,**kw):
        return StaticWeb(origin='http://127.0.0.1:5173',html=b'<html>',javascript=b'export {};',**kw)
    def request(self,s,target='/',method='GET',headers=None,peer='127.0.0.1'):
        return s.respond(method,target,[('Host','127.0.0.1:5173')] if headers is None else headers,peer)
    def test_exact_assets_and_headers(self):
        s=self.make()
        for path,raw in [('/',b'<html>'),('/page.js',b'export {};')]:
            code,h,b=self.request(s,path);self.assertEqual((code,b),(200,raw))
            self.assertEqual(h['Cache-Control'],'no-store');self.assertEqual(h['X-Content-Type-Options'],'nosniff')
            self.assertIn("frame-ancestors 'none'",h['Content-Security-Policy'])
            code,head,b=self.request(s,path,'HEAD');self.assertEqual((code,head,b),(200,h,b''))
    def test_no_file_fallback_or_proxy(self):
        s=self.make()
        for path in ['/runtime-context.json','/entry.js','/../key','/%2e%2e/key','//page.js','/page.js?x','http://127.0.0.1:5173/','/dev-local/v1/account','/.env']:
            self.assertEqual(self.request(s,path)[0],404)
        self.assertEqual(self.request(s,method='POST')[0],405)
    def test_host_peer_duplicate_and_header_rejection(self):
        s=self.make()
        for headers in [[],[('Host','evil:5173')],[('Host','127.0.0.1:5173'),('host','127.0.0.1:5173')],{'Host':'127.0.0.1:5173'},[('Host','127.0.0.1:5173\r\n')]]:
            self.assertEqual(self.request(s,headers=headers)[0],400)
        self.assertEqual(self.request(s,peer='192.0.2.1')[0],400)
    def test_public_context_captured_and_limits(self):
        ctx={'chain_id':'nus-s3-dev-1','service_schema':'s3/3','genesis_hash':'a'*64,'contract_hash':'b'*64,'config_hash':'c'*64,'market_id':'DEVBASE/DEVQUOTE','market_config_version':'1'};s=self.make(context=ctx)
        before=self.request(s,'/runtime-context.json');ctx['chain_id']='changed'
        self.assertEqual(self.request(s,'/runtime-context.json'),before)
        self.assertEqual(before[1]['Content-Type'],'application/json')
        for bad in [dict(ctx,private_seed='secret'),ctx,{'chain_id':'nus-s3-dev-1','service_schema':'s3/3','private_seed':bytes(32)}, {'chain_id':'nus-s3-dev-1','service_schema':'s3/3','__proto__':'x'}]:
            with self.assertRaises(ValueError):self.make(context=bad)
        with self.assertRaises(ValueError):StaticWeb(origin='http://0.0.0.0:5173',html=b'x',javascript=b'x')
        with self.assertRaises(ValueError):StaticWeb(origin='http://127.0.0.1:5173',html=b'x',javascript=b'x'*4194305)
