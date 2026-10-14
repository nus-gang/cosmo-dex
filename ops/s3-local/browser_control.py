"""Fixed public UI commands over a private mailbox; no key/script export API."""
import secrets
import time
from pid_mailbox import _root,_write
from preflight import bounded
from manifest import decode
from workspace_reentry import encode,_rename_exclusive


def publish(root,name,raw):
    pending='.'+name+'.pending'
    _write(root,pending,raw)
    _rename_exclusive(root/pending,root/name)


def wait_ready(control,stop=lambda:False):
    from pathlib import Path
    root=Path(control);started=previous=time.monotonic()
    while True:
        now=time.monotonic()
        if stop() or not previous<=now<started+60:raise ValueError('BROWSER_READY_TIMEOUT')
        previous=now
        try:
            _root(root)
            if any(p.name.endswith('.pending') for p in root.iterdir()):
                time.sleep(.05);continue
            names=sorted(p.name for p in root.iterdir() if p.name[:1].isdigit())
            if len(names)>10000:raise ValueError('BROWSER_EVENTS')
            events=[decode(bounded(root,n,65536)) for n in names]
            if any(e.get('phase') in ('closed','failed') for e in events):raise ValueError('BROWSER_CLOSED')
            if any(e.get('phase')=='tab_ready' for e in events):return command(root,'inspect',stop=stop)
        except FileNotFoundError:pass
        time.sleep(.05)


def command(control, action, arguments=None, *, stop=lambda:False, seconds=10):
    if action not in ('prepare','inspect','activate','click','select','amount','finish') or type(seconds)is not int or not 1<=seconds<=10:
        raise ValueError('BROWSER_COMMAND')
    arguments={} if arguments is None else dict(arguments)
    allowed={'click':{'name'},'select':{'name','value'},'amount':{'value'}}.get(action,set())
    if set(arguments)!=allowed:raise ValueError('BROWSER_COMMAND')
    root=_root(control);pending=_root(root/'commands');results=_root(root/'results')
    nonce=secrets.token_hex(32);name=nonce+'.json';started=previous=time.monotonic()
    if stop():raise ValueError('STOPPED')
    publish(pending,name,encode(dict(nonce=nonce,action=action,**arguments)))
    while True:
        now=time.monotonic()
        if stop() or not previous<=now<started+seconds:raise ValueError('BROWSER_REPLY_TIMEOUT')
        previous=now
        if (results/('.'+name+'.pending')).exists():
            time.sleep(.05);continue
        try:raw=bounded(results,name,65536)
        except FileNotFoundError:
            time.sleep(min(.05,started+seconds-now));continue
        result=decode(raw)
        if set(result)!={'nonce','action','state','result'} or result['nonce']!=nonce or result['action']!=action:
            raise ValueError('BROWSER_REPLY')
        if result['state'] not in ('WAIT_KEYS','KEYS_HELD','ACTIVE','CLOSED'):raise ValueError('BROWSER_CLOSED')
        return result


def activate(control,pin,stop=lambda:False):
    if len(pin)!=64 or any(c not in '0123456789abcdef' for c in pin):raise ValueError('PIN')
    root=_root(control)
    publish(root,'activate.json',encode(dict(schema='s3-local-browser-transition/1',runtime_pin=pin)))
    # Wait at most sixty seconds for the existing C checker. No approval or
    # Context fallback. The tab lifetime continues while this runs.
    started=previous=time.monotonic()
    while True:
        now=time.monotonic()
        if stop() or not previous<=now<started+60:raise ValueError('BROWSER_TRANSITION_TIMEOUT')
        previous=now
        try:bounded(root,'active-context.json',16384);break
        except FileNotFoundError:time.sleep(.05)
    return command(root,'activate',stop=stop)
