"""Read-only, bounded authenticated local Paperclip reader for L-R prerequisites.

Authority comes from run-injected environment, never manifest/launcher input.
No proxy, redirects, token logging, local-file fallback, or service startup.
"""
import os
import time
import urllib.parse
import urllib.request
from manifest import decode
from native_review import ISSUE
from review_documents import KEYS

MAX_RESPONSE = 32 * 1024 * 1024
PATHS = frozenset(['/api/issues/' + ISSUE] +
    ['/api/issues/' + ISSUE + '/documents/' + key for key in KEYS.values()])


class NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, req, fp, code, msg, headers, newurl):
        raise ValueError('PAPERCLIP_READ_FAILED')


class Reader:
    def __init__(self, base, token):
        try:
            url = urllib.parse.urlsplit(base)
            if (url.scheme != 'http' or url.hostname not in ('localhost','127.0.0.1','::1') or
                    url.username or url.password or url.query or url.fragment or
                    url.path.rstrip('/') not in ('','/api') or url.port is None or
                    not isinstance(token,str) or not token or any(c.isspace() for c in token)):
                raise ValueError()
        except (ValueError, TypeError, AttributeError):
            raise ValueError('PAPERCLIP_READER_CONFIG') from None
        self._base = urllib.parse.urlunsplit((url.scheme,url.netloc,'','',''))
        self._token = token
        self._opener = urllib.request.build_opener(urllib.request.ProxyHandler({}), NoRedirect())

    @classmethod
    def from_environment(cls):
        return cls(os.environ.get('PAPERCLIP_API_URL'), os.environ.get('PAPERCLIP_API_KEY'))

    def __call__(self, path):
        if path not in PATHS:
            raise ValueError('PAPERCLIP_READ_PATH')
        req = urllib.request.Request(self._base + path,
            headers={'Authorization':'Bearer '+self._token, 'Accept':'application/json',
                     'Cache-Control':'no-cache'}, method='GET')
        try:
            deadline = time.monotonic()+10
            with self._opener.open(req, timeout=5) as response:
                if response.status != 200 or response.geturl() != self._base + path:
                    raise ValueError()
                if response.headers.get_content_type() != 'application/json':
                    raise ValueError()
                chunks=[]; size=0
                while True:
                    chunk=response.read(min(65536, MAX_RESPONSE+1-size))
                    size+=len(chunk)
                    if size>MAX_RESPONSE or time.monotonic()>deadline:
                        raise ValueError()
                    if not chunk: break
                    chunks.append(chunk)
                result=decode(b''.join(chunks))
                if not isinstance(result,dict): raise ValueError()
                return result
        except Exception:
            # Neither bearer value nor response bodies enter diagnostics.
            raise ValueError('PAPERCLIP_READ_FAILED') from None
