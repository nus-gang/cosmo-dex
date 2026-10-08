import unittest
from static_web import StaticWeb
from web_proxy import WebProxy, GET, POST

class ProxyTests(unittest.TestCase):
    def setUp(self):
        self.origin='http://127.0.0.1:5173'
        self.proxy=WebProxy(origin=self.origin,worker_port=8787,
            static=StaticWeb(origin=self.origin,html=b'html',javascript=b'js'))
        self.calls=[]
    def exchange(self,dest,request):
        self.calls.append((dest,request))
        return 200,[('Content-Type','application/json'),('Content-Length','2')],b'{}'
    def request(self,method='POST',suffix='chain/broadcast',body=b'{"tx_bytes":"YWJj"}',headers=None,exchange=None,peer='127.0.0.1'):
        if headers is None:
            headers=[('Host','127.0.0.1:5173'),('Origin',self.origin),('Authorization','Bearer private'),('Content-Length',str(len(body)))]
            if method=='POST':headers.append(('Content-Type','application/json'))
        return self.proxy.respond(method,'/dev-local/v1/'+suffix,headers,body,peer,exchange or self.exchange)
    def test_all_routes_exact_bytes_fixed_destination(self):
        for method,routes in [('GET',GET),('POST',POST)]:
            for route in routes:
                body=b'{}' if method=='POST' else b''
                before=len(self.calls)
                self.assertEqual(self.request(method,route,body)[0],200)
                self.assertEqual(len(self.calls),before+1)
                dest,raw=self.calls[-1]
                self.assertEqual(dest,('127.0.0.1',8787))
                self.assertTrue(raw.startswith(f'{method} /dev-local/v1/{route} HTTP/1.1\r\n'.encode()))
                self.assertTrue(raw.endswith(b'\r\n\r\n'+body))
                self.assertIn(b'Authorization: Bearer private\r\n',raw)
                self.assertIn(b'Origin: http://127.0.0.1:5173\r\n',raw)
    def test_same_origin_get_without_origin_and_static(self):
        headers=[('Host','127.0.0.1:5173'),('Authorization','Bearer private')]
        self.assertEqual(self.request('GET','account',b'',headers)[0],200)
        self.assertIn(b'Origin: '+self.origin.encode(),self.calls[0][1])
        for path,body in [('/',b'html'),('/page.js',b'js')]:
            self.assertEqual(self.proxy.respond('GET',path,headers,b'','::1',lambda *_:self.fail('IO'))[2],body)
    def test_input_rejections_no_effect(self):
        good=[('Host','127.0.0.1:5173'),('Origin',self.origin),('Content-Type','application/json'),('Content-Length','2')]
        for h in [good+[('Host','127.0.0.1:5173')],good+[('Origin',self.origin)],good+[('Content-Length','2')],
                  good+[('Authorization','one'),('authorization','two')],good+[('Transfer-Encoding','chunked')],
                  good+[('X-Forwarded-For','127.0.0.1')],good+[('X','a\r\nb')],good+[('Bad Name','x')],
                  [('Host','evil:5173')]+good[1:],good[:1]+[('Origin','http://localhost:5173')]+good[2:],
                  good[:-1]+[('Content-Length','02')],good[0:1]+good[2:]]:
            self.assertGreaterEqual(self.request(body=b'{}',headers=h)[0],400)
        for suffix in ['chain/broadcast/','chain/%62roadcast','../orders','orders?x','http://evil','unknown']:
            self.assertEqual(self.request(suffix=suffix)[0],404)
        self.assertEqual(self.request(peer='192.0.2.1')[0],400)
        self.assertEqual(self.request(body=b'x'*16385)[0],400)
        self.assertEqual(self.calls,[])
    def test_loss_or_invalid_response_never_retry_or_claim_finality(self):
        for response in [None,(302,[],b''),(200,[('Content-Type','text/html'),('Content-Length','2')],b'{}'),
            (200,[('Content-Type','application/json'),('Content-Length','2'),('Content-Length','2')],b'{}'),
            (200,[('Content-Type','application/json'),('Content-Length','2'),('Set-Cookie','secret')],b'{}')]:
            calls=[]
            def exchange(*args):
                calls.append(args)
                if response is None:raise OSError('private upstream detail')
                return response
            status,headers,body=self.request(exchange=exchange)
            self.assertEqual(status,503);self.assertEqual(len(calls),1)
            self.assertNotIn(b'private',body);self.assertNotIn(b'COMMITTED',body)
        with self.assertRaises(KeyboardInterrupt):
            self.request(exchange=lambda *_:(_ for _ in ()).throw(KeyboardInterrupt()))
    def test_response_header_isolation_and_unknown_preserved(self):
        body=b'{"state":"SUBMISSION_UNKNOWN","durable_ack":false}'
        def exchange(*args):
            return 200,[('Content-Type','application/json'),('Content-Length',str(len(body))),('X-Internal','secret')],body
        status,headers,result=self.request(exchange=exchange)
        self.assertEqual((status,result),(200,body));self.assertNotIn('X-Internal',headers)
        self.assertEqual(headers['Cache-Control'],'no-store')
        for port in [True,0,5173,65536,'8787']:
            with self.assertRaises(ValueError):WebProxy(origin=self.origin,worker_port=port,static=self.proxy.static)
