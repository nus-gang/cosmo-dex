#!/usr/bin/env python3
"""Finite CI controller. Owns child PIDs; never signals a port-derived process."""
import argparse, fcntl, hashlib, json, os, re, select, signal, socket, subprocess, sys, time
from pathlib import Path

PORTS = [30556+i*10+j for i in range(4) for j in (0,1)]+[8788,5173]
p=argparse.ArgumentParser()
for name in ('product','docs','driver-root','home','output'):
    p.add_argument('--'+name,type=Path,required=True)
a=p.parse_args()
for name in ('product','docs','driver_root','home','output'):
    setattr(a,name,getattr(a,name).resolve())
ROOT=Path(__file__).resolve().parents[2]
config=json.loads((ROOT/'tests/docqa/input.json').read_text())
a.output.mkdir(parents=True,exist_ok=True)
report={'result':'FAIL','document_qa':'NOT_RUN','stops':[],'processes':[], 'external_stop_wait_seconds':40}
owned=[]; driver=None; initialized=False; temporary=None; runtime=None; first_pins=None; saved_wal=None

def run(argv,**kw):
    return subprocess.run(argv,text=True,capture_output=True,check=True,timeout=100,**kw)
def sha(path): return hashlib.sha256(path.read_bytes()).hexdigest()
def git(root,ref='HEAD'): return run(['git','-C',str(root),'rev-parse',ref]).stdout.strip()
def command(op): return [sys.executable,str(a.product/'ops/s2/runtime.py'),op,'--home',str(a.home)]
def process_table():
    rows=run(['ps','-axo','pid=,ppid=,pgid=,stat=']).stdout.splitlines()
    return [dict(zip(('pid','ppid','pgid','state'),(int(v[0]),int(v[1]),int(v[2]),v[3]))) for line in rows if (v:=line.split())]
def descendants(pid):
    rows=process_table(); ids={pid}
    while True:
        more={r['pid'] for r in rows if r['ppid'] in ids}
        if more<=ids: break
        ids|=more
    return [r for r in rows if r['pid'] in ids]
def ports_free(ports):
    result={}
    for port in ports:
        try:
            with socket.socket() as s:
                s.setsockopt(socket.SOL_SOCKET,socket.SO_REUSEADDR,1)
                s.bind(('127.0.0.1',port))
            result[str(port)]=True
        except OSError: result[str(port)]=False
    return result

def resources(ports,locks):
    result={'listeners_released':ports_free(ports),'locks':{},'control_socket_absent':not (a.home/'chain/.control.sock').exists()}
    for name in (['runtime.lock','chain/.supervisor.lock','journal/writer.lock'] if locks else []):
        try:
            # Existing locks must be present; do not manufacture success by creating them.
            with (a.home/name).open('r+') as f: fcntl.flock(f,fcntl.LOCK_EX|fcntl.LOCK_NB)
            result['locks'][name]=True
        except OSError: result['locks'][name]=False
    result['ok']=all(result['listeners_released'].values()) and all(result['locks'].values()) and result['control_socket_absent']
    return result

def spawn(label,argv):
    # Logs stay private in runner scratch; only explicit public evidence is uploaded.
    log=(a.home.parent/(label+'-'+str(len(owned))+'.log')).open('w')
    proc=subprocess.Popen(argv,cwd=a.product,stdout=log,stderr=log,start_new_session=True)
    owned.append((proc,log,label))
    report['processes'].append({'label':label,'pid':proc.pid,'argv':argv})
    return proc

def stop(proc,label):
    before=descendants(proc.pid) if proc.poll() is None else []
    groups={r['pgid'] for r in before}; ids={r['pid'] for r in before}
    forced=False; started=time.monotonic()
    if proc.poll() is None:
        proc.send_signal(signal.SIGTERM)
        try: proc.wait(timeout=40)
        except subprocess.TimeoutExpired:
            forced=True
            # Only process groups proven owned before signalling are eligible.
            for group in groups:
                try: os.killpg(group,signal.SIGKILL)
                except ProcessLookupError: pass
            proc.wait(timeout=5)
    remaining=[r for r in process_table() if (r['pid'] in ids or r['pgid'] in groups) and not r['state'].startswith('Z')]
    check=resources(PORTS if label=='runtime' else [5173],label=='runtime')
    row={'label':label,'pid':proc.pid,'owned_before':before,'exit':proc.returncode,'forced':forced,
         'elapsed_seconds':time.monotonic()-started,'remaining_owned':remaining,**check}
    row['ok']=check['ok'] and not remaining and not forced and proc.returncode in ([0] if label=='runtime' else [-signal.SIGTERM,0])
    report['stops'].append(row)
    if not row['ok']: raise RuntimeError('owned shutdown failed: '+label)
    return row

def health(proc):
    deadline=time.monotonic()+100
    while time.monotonic()<deadline:
        if proc.poll() is not None: raise RuntimeError('runtime exited before readiness')
        r=subprocess.run(command('health'),capture_output=True,text=True,timeout=20)
        if r.returncode==0: return json.loads(r.stdout)
        time.sleep(.5)
    raise TimeoutError('runtime readiness')

def rpc(req):
    global initialized,temporary,runtime,first_pins,saved_wal
    op=req['op']
    if op=='temporary_start':
        assert not initialized and temporary is None
        assert all(ports_free(PORTS).values()),'occupied port; no owner will be stopped'
        temporary=spawn('temporary',['node','web/s2/serve.mjs'])
        import urllib.request
        deadline=time.monotonic()+20
        while time.monotonic()<deadline:
            assert temporary.poll() is None
            try:
                with urllib.request.urlopen('http://127.0.0.1:5173',timeout=1) as r:
                    if r.status==200: return {'pid':temporary.pid,'web':200}
            except OSError: time.sleep(.2)
        raise TimeoutError('temporary web readiness')
    if op=='init':
        assert not initialized and temporary is not None and temporary.poll() is None
        keys=req['publicKeys']; assert isinstance(keys,list) and len(keys)==2 and all(isinstance(k,str) for k in keys)
        users=a.home.parent/'users.json'; users.write_text(json.dumps(keys))
        run([*command('init'),'--user-public-keys',str(users)],cwd=a.product)
        initialized=True; first_pins=(a.home/'runtime.json').read_bytes()
        return json.loads(first_pins)
    if op=='temporary_stop':
        assert temporary is not None
        row=stop(temporary,'temporary'); temporary=None; return row
    if op=='start':
        assert initialized and temporary is None and runtime is None
        assert (a.home/'runtime.json').read_bytes()==first_pins
        if saved_wal is not None: assert (a.home/'journal/journal.wal').read_bytes().startswith(saved_wal)
        runtime=spawn('runtime',command('serve'))
        return health(runtime)
    if op=='health':
        assert runtime is not None
        return health(runtime)
    if op=='stop':
        assert runtime is not None
        row=stop(runtime,'runtime'); runtime=None
        wal=(a.home/'journal/journal.wal').read_bytes()
        if saved_wal is not None: assert wal.startswith(saved_wal)
        saved_wal=wal; row['journal_sha256']=hashlib.sha256(wal).hexdigest()
        assert (a.home/'runtime.json').read_bytes()==first_pins
        return row
    raise ValueError('unsupported control operation')

try:
    assert os.environ.get('GITHUB_ACTIONS')=='true','hosted CI only; shared local services are out of scope'
    assert not a.home.exists(),'fresh home required'
    a.home.parent.mkdir(parents=True,exist_ok=True)
    assert git(a.product)==config['product_ref']
    assert git(a.docs)==config['docs_ref'] and git(a.docs,'HEAD^{tree}')==config['docs_tree']
    driver_ref=git(a.driver_root)
    assert driver_ref==(git(ROOT) if config['driver_ref']=='SELF' else config['driver_ref'])
    module=(a.driver_root/config['driver_path']).resolve()
    assert module.is_relative_to(a.driver_root) and module.is_file()
    report.update(product_ref=git(a.product),docs_ref=git(a.docs),transport_ref=git(ROOT),driver_ref=driver_ref,
        driver_path=config['driver_path'],driver_sha256=sha(module),
        run_id=os.environ.get('GITHUB_RUN_ID'),attempt=os.environ.get('GITHUB_RUN_ATTEMPT'),
        actor=os.environ.get('GITHUB_ACTOR'),triggering_actor=os.environ.get('GITHUB_TRIGGERING_ACTOR'),
        image={k:os.environ.get(k) for k in ['ImageOS','ImageVersion','RUNNER_OS','RUNNER_ARCH']})
    argv=['node',str(ROOT/'tests/docqa/browser.mjs'),str(a.product),str(module),str(a.output)]
    report['driver_command_argv']=argv; report['cwd']=str(a.product)
    report['versions']={name:run(cmd).stdout.strip() for name,cmd in {
        'go':['go','version'],'rust':['rustc','+1.92.0','--version'],'node':['node','--version'],
        'npm':['npm','--version'],'python':[sys.executable,'--version']}.items()}
    report['build_sha256']={x:sha(a.product/x) for x in ['chain/app/bin/nusd','exchange/target/debug/exchange-s2',
        'web/dist/s2/index.html','web/dist/s2/wallet.js','web/package-lock.json','exchange/Cargo.lock']}
    with (a.home.parent/'driver.stderr').open('w') as stderr:
        driver=subprocess.Popen(argv,cwd=a.product,stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=stderr,text=True,start_new_session=True,bufsize=1)
        deadline=time.monotonic()+600
        while True:
            if time.monotonic()>deadline: raise TimeoutError('driver 600s deadline')
            if not select.select([driver.stdout],[],[],1)[0]: continue
            line=driver.stdout.readline()
            if not line: break
            req=json.loads(line)
            try: reply={'id':req['id'],'result':rpc(req)}
            except Exception as error:
                reply={'id':req['id'],'error':str(error)}
                report.setdefault('control_errors',[]).append(str(error))
            driver.stdin.write(json.dumps(reply)+'\n'); driver.stdin.flush()
        report['driver_exit']=driver.wait(timeout=10)
        assert driver.returncode==0,'browser driver failed'
    assert initialized and temporary is None and runtime is None,'driver must explicitly stop its services'
    assert len([s for s in report['stops'] if s['label']=='runtime'])>=2,'restart evidence required'
    assert not report.get('control_errors')
    report['result']='PASS'
except Exception as error:
    report['error']=str(error)
finally:
    cleanup=[]
    for proc,log,label in reversed(owned):
        try:
            if proc.poll() is None: stop(proc,label)
        except Exception as error: cleanup.append(str(error))
        finally: log.close()
    if driver and driver.poll() is None:
        driver.terminate()
        try: driver.wait(timeout=10)
        except subprocess.TimeoutExpired: driver.kill(); driver.wait(timeout=5)
    if cleanup: report['cleanup_errors']=cleanup; report['result']='FAIL'
    report['final_resources']=resources(PORTS,initialized and (a.home/'journal').exists())
    if not report['final_resources']['ok']: report['result']='FAIL'
    report['public_files']={str(f.relative_to(a.output)):sha(f) for f in a.output.rglob('*') if f.is_file() and f.name!='manifest.json'}
    (a.output/'manifest.json').write_text(json.dumps(report,indent=2)+'\n')
print(json.dumps({'result':report['result'],'document_qa':report['document_qa']}))
sys.exit(0 if report['result']=='PASS' else 1)
