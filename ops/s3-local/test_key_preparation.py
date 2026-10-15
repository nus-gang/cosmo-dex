import unittest
from unittest.mock import patch
from static_web import StaticWeb
import key_preparation as k


class KeyPreparationTest(unittest.TestCase):
    def test_public_assets_only_no_context_or_upstream(self):
        static=StaticWeb(origin='http://127.0.0.1:5173',html=b'public html',javascript=b'approved js')
        proxy=k.KeyPreparation(origin='http://127.0.0.1:5173',worker_port=18080,static=static)
        def forbidden(*args):self.fail('upstream called')
        for target in ('/','/page.js','/runtime-context.json','/dev-local/v1/auth/challenge','/dev-local/v1/chain/broadcast','/other'):
            code,_,_=proxy.respond('GET',target,[('Host','127.0.0.1:5173')],b'','127.0.0.1',forbidden)
            self.assertEqual(code,200 if target in ('/','/page.js') else 404)
        self.assertEqual(proxy.respond('GET','/',[('Host','evil')],b'','127.0.0.1',forbidden)[0],400)
        self.assertEqual(proxy.respond('POST','/',[('Host','127.0.0.1:5173')],b'payload','127.0.0.1',forbidden)[0],404)

    def test_revocation_and_stop_create_no_listener(self):
        def forbidden(*args):self.fail('socket called')
        with patch.object(k,'prepare',return_value=object()),patch.object(k.approval_gate,'inspect',side_effect=[{}, {'revoked':True}]):
            with self.assertRaises(ValueError):k.run(None,None,None,None,None,{},lambda:False,lambda:None,socket_factory=forbidden)
        with self.assertRaises(ValueError):k.run(None,None,None,None,None,{},lambda:True,lambda:None,socket_factory=forbidden)


if __name__=='__main__':unittest.main()
