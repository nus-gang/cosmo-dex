"""Single bounded loopback HTTP exchange for WebProxy; no listener/retry/DNS."""
import math
import socket
import time
import re

class Upstream:
    def __init__(self, *, timeout=2.0, connect=socket.create_connection, clock=time.monotonic):
        if type(timeout) not in (int,float) or not math.isfinite(timeout) or not 0 < timeout <= 2:
            raise ValueError('UPSTREAM_TIMEOUT')
        self.timeout, self.connect, self.clock = timeout, connect, clock

    def __call__(self, destination, request):
        if (type(destination) is not tuple or len(destination)!=2 or destination[0]!='127.0.0.1' or
            type(destination[1]) is not int or not 1024<=destination[1]<=65535 or destination[1]==5173 or
            type(request) is not bytes or not 0<len(request)<=150000):
            raise ValueError('UPSTREAM_INPUT')
        start=self.clock()
        if not math.isfinite(start):raise ValueError('UPSTREAM_CLOCK')
        deadline=start+self.timeout
        previous=start
        def remaining():
            nonlocal previous
            now=self.clock()
            if not math.isfinite(now) or now<previous or now>=deadline:raise TimeoutError('UPSTREAM_DEADLINE')
            previous=now
            return deadline-now
        stream=None
        try:
            stream=self.connect(destination,timeout=remaining())
            at=0
            while at<len(request):
                stream.settimeout(remaining())
                sent=stream.send(request[at:])
                if type(sent) is not int or not 0<sent<=len(request)-at:raise ValueError('UPSTREAM_SEND')
                at+=sent
            def read(n):
                stream.settimeout(remaining())
                data=stream.recv(n)
                if type(data) is not bytes or not 0<len(data)<=n:raise ValueError('UPSTREAM_EOF')
                return data
            def line():
                raw=bytearray()
                while len(raw)<4096:
                    raw.extend(read(1))
                    if raw[-1]==10:
                        if not raw.endswith(b'\r\n'):raise ValueError('UPSTREAM_FRAMING')
                        return bytes(raw[:-2]).decode('ascii')
                raise ValueError('UPSTREAM_LINE_LIMIT')
            status_line=line()
            if not re.fullmatch(r'HTTP/1\.1 [1-5][0-9]{2} [\x20-\x7e]+',status_line):
                raise ValueError('UPSTREAM_STATUS')
            status=int(status_line[9:12])
            if status not in (200,204,400,401,403,404,405,409,413,503):raise ValueError('UPSTREAM_STATUS')
            headers=[]
            while True:
                row=line()
                if not row:break
                if len(headers)>=32:raise ValueError('UPSTREAM_HEADER_LIMIT')
                key,sep,value=row.partition(':')
                if not sep or not re.fullmatch(r"[!#$%&'*+.^_`|~0-9A-Za-z-]+",key) or any(ord(c)<32 or ord(c)>126 for c in value):
                    raise ValueError('UPSTREAM_HEADER')
                headers.append((key,value.strip(' ')))
            fields=lambda name:[v for k,v in headers if k.lower()==name]
            lengths=fields('content-length')
            if (len(lengths)!=1 or not re.fullmatch('0|[1-9][0-9]{0,6}',lengths[0]) or
                fields('content-type')!=['application/json'] or
                any(fields(k) for k in ('transfer-encoding','location','set-cookie','upgrade'))):
                raise ValueError('UPSTREAM_FRAMING')
            size=int(lengths[0])
            if size>2*1024*1024 or (status==204 and size):raise ValueError('UPSTREAM_BODY_LIMIT')
            body=bytearray()
            while len(body)<size:body.extend(read(min(65536,size-len(body))))
            remaining()
            return status,headers,bytes(body)
        finally:
            if stream is not None:stream.close()
