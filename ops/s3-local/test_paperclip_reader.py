import email.message
import io
import unittest
from unittest.mock import Mock, patch
import paperclip_reader as api

PATH='/api/issues/'+api.ISSUE
BASE='http://127.0.0.1:3100'

class Response(io.BytesIO):
    def __init__(self, data=b'{"status":"in_progress"}', status=200, url=BASE+PATH, mime='application/json'):
        super().__init__(data); self.status=status; self.url=url
        self.headers=email.message.Message(); self.headers['Content-Type']=mime
    def geturl(self): return self.url

class ReaderTest(unittest.TestCase):
    def reader(self, response):
        reader=api.Reader(BASE+'/api/','synthetic-test-token')
        reader._opener=Mock(); reader._opener.open.return_value=response
        return reader

    def test_get_auth_exact_path_fresh_each_time(self):
        reader=self.reader(None)
        reader._opener.open.side_effect=[Response(),Response()]
        for _ in range(2): self.assertEqual(reader(PATH),{'status':'in_progress'})
        self.assertEqual(reader._opener.open.call_count,2)
        request=reader._opener.open.call_args.args[0]
        self.assertEqual(request.get_method(),'GET')
        self.assertEqual(request.full_url,BASE+PATH)
        self.assertEqual(request.get_header('Authorization'),'Bearer synthetic-test-token')
        self.assertEqual(reader._opener.open.call_args.kwargs,{'timeout':5})

    def test_config_and_path_reject_before_io(self):
        for base in [None,'file:///etc/passwd','https://external.test:443','http://localhost',
            BASE+'/evil',BASE+'?x=y','http://user:secret@127.0.0.1:3100']:
            with self.assertRaisesRegex(ValueError,'PAPERCLIP_READER_CONFIG'): api.Reader(base,'x')
        for token in [None,'','x\ny']:
            with self.assertRaises(ValueError): api.Reader(BASE,token)
        reader=self.reader(Response())
        for path in ['/api/agents/me',PATH+'/../secrets',PATH+'?x=y','http://external.test']:
            with self.assertRaisesRegex(ValueError,'PAPERCLIP_READ_PATH'): reader(path)
        reader._opener.open.assert_not_called()

    def test_redirect_status_mime_json_size_and_deadline_fail_closed(self):
        for response in [Response(status=302),Response(url='http://external.test'),
            Response(mime='text/html'),Response(b'[]'),Response(b'{"a":1,"a":2}'),Response(b'bad')]:
            with self.assertRaisesRegex(ValueError,'^PAPERCLIP_READ_FAILED$'): self.reader(response)(PATH)
        with patch.object(api,'MAX_RESPONSE',4), self.assertRaises(ValueError):
            self.reader(Response())(PATH)
        with patch.object(api.time,'monotonic',side_effect=[0,11]), self.assertRaises(ValueError):
            self.reader(Response())(PATH)
        with self.assertRaises(ValueError): api.NoRedirect().redirect_request(None,None,302,'',{},BASE)

    def test_io_diagnostic_never_contains_token_or_body(self):
        reader=self.reader(None); reader._opener.open.side_effect=OSError('synthetic-test-token private')
        with self.assertRaisesRegex(ValueError,'^PAPERCLIP_READ_FAILED$'): reader(PATH)

if __name__=='__main__': unittest.main()
