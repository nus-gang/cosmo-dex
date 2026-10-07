import unittest
from web_upstream import Upstream
from web_proxy import WebProxy
from static_web import StaticWeb

class Socket:
    def __init__(self,response):self.response=response;self.sent=b'';self.closed=False;self.timeouts=[]
    def settimeout(self,value):self.timeouts.append(value)
    def send(self,data):n=min(7,len(data));self.sent+=data[:n];return n
    def recv(self,n):data=self.response[:n];self.response=self.response[n:];return data
    def close(self):self.closed=True

class UpstreamTests(unittest.TestCase):
    def wire(self,body=b'{}'):
        return b'HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: '+str(len(body)).encode()+b'\r\n\r\n'+body
    def run_wire(self,raw,clock=lambda:1.0):
        stream=Socket(raw);calls=[]
        def connect(dest,timeout):calls.append((dest,timeout));return stream
        client=Upstream(connect=connect,clock=clock)
        return stream,calls,client
    def test_partial_io_and_proxy_roundtrip(self):
        body=b'{"state":"SUBMISSION_UNKNOWN","durable_ack":false}'
        s,c,u=self.run_wire(self.wire(body))
        p=WebProxy(origin='http://localhost:5173',worker_port=8787,
            static=StaticWeb(origin='http://localhost:5173',html=b'x',javascript=b'y'))
        tx=b'{"tx_bytes":"YWJj"}'
        result=p.respond('POST','/dev-local/v1/chain/broadcast',[
            ('Host','localhost:5173'),('Origin','http://localhost:5173'),('Authorization','Bearer private'),
            ('Content-Type','application/json'),('Content-Length',str(len(tx)))],tx,'127.0.0.1',u)
        self.assertEqual((result[0],result[2]),(200,body))
        self.assertTrue(s.sent.endswith(tx));self.assertTrue(s.closed);self.assertEqual(len(c),1)
        self.assertTrue(all(0<x<=2 for x in s.timeouts))
    def test_response_framing_caps_and_no_retry(self):
        good=self.wire()
        for bad in [good[:-1],good.replace(b'200 OK',b'302 Found'),good.replace(b'Content-Length: 2',b'Content-Length: 02'),
            good.replace(b'Content-Length: 2',b'Content-Length: 2\r\nContent-Length: 2'),
            good.replace(b'Content-Length: 2',b'Content-Length: 2097153'),
            good.replace(b'Content-Type:',b'Bad Header:'),good.replace(b'200 OK\r\n',b'200 OK\n'),
            good.replace(b'Content-Length: 2',b'Transfer-Encoding: chunked\r\nContent-Length: 2'),
            good.replace(b'Content-Length: 2',b'Set-Cookie: secret\r\nContent-Length: 2'),
            b'HTTP/1.1 200 '+b'x'*4096+b'\r\n',
            good.replace(b'Content-Length: 2',b'X: a\r\n'*33+b'Content-Length: 2')]:
            s,c,u=self.run_wire(bad)
            with self.assertRaises((ValueError,UnicodeError)):u(('127.0.0.1',8787),b'request')
            self.assertEqual(len(c),1);self.assertTrue(s.closed)
    def test_total_deadline_clock_regression_and_interrupt_close(self):
        for times in [[1,1,1,3],[1,1,1,.5]]:
            it=iter(times);s,c,u=self.run_wire(self.wire(),lambda:next(it))
            with self.assertRaises(TimeoutError):u(('127.0.0.1',8787),b'request')
            self.assertTrue(s.closed);self.assertEqual(len(c),1)
        s,c,u=self.run_wire(self.wire())
        s.recv=lambda _:(_ for _ in ()).throw(KeyboardInterrupt())
        with self.assertRaises(KeyboardInterrupt):u(('127.0.0.1',8787),b'request')
        self.assertTrue(s.closed)
    def test_invalid_destination_before_connect(self):
        s,c,u=self.run_wire(self.wire())
        for d in [('localhost',8787),('192.0.2.1',8787),('127.0.0.1',5173),('127.0.0.1',True)]:
            with self.assertRaises(ValueError):u(d,b'request')
        self.assertEqual(c,[])
