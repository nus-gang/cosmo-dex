"""Pure response boundary; no socket, file reads, proxy or approval grant.

The managed web launcher must supply reviewed asset bytes and a gate-validated
public Context. HTTP parsing must preserve duplicate Host headers. A future
listener must enforce loopback and bounded request IO before calling respond.
"""
import json
from types import MappingProxyType

CSP = "default-src 'none'; script-src 'self'; connect-src 'self'; base-uri 'none'; frame-ancestors 'none'; form-action 'none'"

class StaticWeb:
    def __init__(self, *, origin, html, javascript, context=None):
        if origin not in ('http://127.0.0.1:5173', 'http://localhost:5173'):
            raise ValueError('ORIGIN_REJECTED')
        if not isinstance(html, bytes) or not 0 < len(html) <= 65536:
            raise ValueError('HTML_SIZE')
        if not isinstance(javascript, bytes) or not 0 < len(javascript) <= 4194304:
            raise ValueError('SCRIPT_SIZE')
        routes = {'/': ('text/html; charset=utf-8', html), '/page.js': ('text/javascript; charset=utf-8', javascript)}
        if context is not None:
            if (not isinstance(context, dict) or set(context) != {'service_schema','chain_id','genesis_hash','contract_hash','config_hash','market_id','market_config_version'} or
                any(not isinstance(k, str) or not isinstance(v, str) or not 0 < len(v) <= 256 for k,v in context.items()) or
                context.get('chain_id') != 'nus-s3-dev-1' or context.get('service_schema') != 's3/3'):
                raise ValueError('CONTEXT_FORMAT')
            import re
            if context['market_id']!='DEVBASE/DEVQUOTE' or context['market_config_version']!='1' or any(not re.fullmatch('[0-9a-f]{64}',context[k]) for k in ('genesis_hash','contract_hash','config_hash')):
                raise ValueError('CONTEXT_FORMAT')
            if any(not re.fullmatch('[a-z][a-z0-9_]{0,63}',k) or k in ('constructor','prototype','__proto__') for k in context):
                raise ValueError('CONTEXT_FORMAT')
            raw=json.dumps(context,ensure_ascii=True,separators=(',',':')).encode()
            if len(raw)>16384: raise ValueError('CONTEXT_SIZE')
            routes['/runtime-context.json']=('application/json',raw)
        self._routes=MappingProxyType(routes)
        self._host=origin.removeprefix('http://')

    def respond(self, method, target, headers, peer):
        # List (not dict) is required so a parser cannot erase duplicate Host.
        if (peer not in ('127.0.0.1','::1') or not isinstance(headers,list) or len(headers)>32 or
            any(not isinstance(row,tuple) or len(row)!=2 or any(not isinstance(v,str) or len(v)>4096 or '\r' in v or '\n' in v for v in row) for row in headers)):
            return self._response(400,b'REQUEST_REJECTED','text/plain')
        hosts=[v for k,v in headers if k.lower()=='host']
        if hosts != [self._host]: return self._response(400,b'HOST_REJECTED','text/plain')
        if method not in ('GET','HEAD'): return self._response(405,b'METHOD_REJECTED','text/plain')
        # No decoding/normalization, directory listing, filesystem fallback or SPA fallback.
        if not isinstance(target,str) or target not in self._routes:
            return self._response(404,b'NOT_FOUND','text/plain',method=='HEAD')
        mime,body=self._routes[target]
        return self._response(200,body,mime,method=='HEAD')

    @staticmethod
    def _response(status,body,mime,head=False):
        return status, {'Content-Type':mime,'Content-Length':str(len(body)),
            'Cache-Control':'no-store','Content-Security-Policy':CSP,
            'X-Content-Type-Options':'nosniff','Referrer-Policy':'no-referrer',
            'Cross-Origin-Resource-Policy':'same-origin'}, b'' if head else body
