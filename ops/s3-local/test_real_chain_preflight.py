"""Invoked by Go fixture test; never executes start or constructs a listener."""
import sys
from pathlib import Path
from unittest.mock import patch
import chain_stage
import chain_preflight
import chain_ready
import chain_run
import os
import fcntl


def main(root, pin, peers):
    root = Path(root)
    scratch = root/'scratch'
    scratch.mkdir(mode=0o700)
    home = root/'home'
    def snapshot():
        return {str(p.relative_to(home)):p.read_bytes() for p in home.rglob('*') if p.is_file()}
    original = snapshot()
    kwargs = dict(bundle=root/'bundle', artifacts=root/'artifacts', pin=pin,
                  profile='s3-dev-local/1', acknowledge=True, decision_id='synthetic',
                  revisions={}, inputs=root, input_name='input.json',
                  effective_profile=root/'profile.json', scratch=scratch)
    real_spawn = chain_preflight.subprocess.Popen
    children=[]
    def spawn(argv, **kw):
        assert argv[1] in ('preflight', 'start')
        child=real_spawn(argv, **kw);children.append(child);return child
    with patch.object(chain_stage.approval_gate,'inspect',return_value={}), patch.object(chain_preflight.subprocess,'Popen',side_effect=spawn):
        with chain_stage.stage(**kwargs) as staged:
            def check():
                return chain_preflight.check(staged,pin,home,'127.0.0.1:27657','127.0.0.1:27656',peers)
            assert check()['b_preflight'] is True
            assert snapshot()==original
            guard=home/'guard.dev.json'
            guard.write_bytes(original['guard.dev.json']+b' ')
            try: check()
            except ValueError as e: assert str(e)=='CHAIN_PREFLIGHT_REJECTED'
            else: raise AssertionError('changed home accepted')
            guard.write_bytes(original['guard.dev.json'])
            # Captured bytes survive source replacement, including the executable.
            (root/'input.json').write_bytes(b'replaced')
            (root/'profile.json').write_bytes(b'replaced')
            (root/'artifacts/bin/nus-s3-local-chain').write_bytes(b'replaced')
            assert check()['b_preflight'] is True
            assert snapshot()==original
            # READY acquires the writer lock but START is never transmitted.
            for revoke in (False, True):
                calls=[0]
                def audit():
                    calls[0]+=1
                    return {'revision': 'revoked' if revoke and calls[0]==3 else 'fixture'}
                try:
                    with chain_ready.ready(staged,pin,home,'127.0.0.1:27657',
                            '127.0.0.1:27656',peers,audit) as report:
                        assert not revoke and report['start_sent'] is False
                        with (home/'writer.dev.lock').open('rb') as lock:
                            try: fcntl.flock(lock,fcntl.LOCK_EX|fcntl.LOCK_NB)
                            except BlockingIOError: pass
                            else: raise AssertionError('writer not held at READY')
                except ValueError as e:
                    assert revoke and str(e)=='APPROVAL_CHANGED_AFTER_CHAIN_READY'
                else:
                    assert not revoke
                with (home/'writer.dev.lock').open('rb') as lock:
                    fcntl.flock(lock,fcntl.LOCK_EX|fcntl.LOCK_NB)
                    fcntl.flock(lock,fcntl.LOCK_UN)
                after=snapshot()
                assert after.pop('writer.dev.lock')==b''
                assert after==original
            # Exercise the real B child through the last pre-START audit.
            # A write tripwire makes accidental service activation a test failure.
            for scenario in ('revoke', 'tamper', 'stop', 'interrupt'):
                calls=[0]
                stopped=[False]
                saved=staged.effective_profile.read_bytes()
                def audit_start():
                    calls[0]+=1
                    if calls[0]==4:
                        if scenario=='revoke': return {'revision':'revoked'}
                        if scenario=='tamper': staged.effective_profile.write_bytes(saved+b' ')
                        if scenario=='stop': stopped[0]=True
                        if scenario=='interrupt': raise KeyboardInterrupt()
                    return {'revision':'fixture'}
                original_write=os.write
                def no_start(fd, data):
                    assert data != b'START\n', 'unexpected Chain START'
                    return original_write(fd, data)
                try:
                    with patch.object(chain_run.os, 'write', side_effect=no_start):
                        chain_run.run(staged,pin,home,'127.0.0.1:27657',
                                      '127.0.0.1:27656',peers,audit_start,
                                      stopped=lambda:stopped[0])
                except KeyboardInterrupt:
                    assert scenario=='interrupt'
                except ValueError as e:
                    expected={'revoke':'APPROVAL_CHANGED_BEFORE_CHAIN_START',
                              'tamper':'CHAIN_STAGED_BYTES_CHANGED',
                              'stop':'CHAIN_STOPPED_BEFORE_START'}
                    assert str(e)==expected[scenario], (scenario,str(e))
                else:
                    raise AssertionError('pre-START denial missing')
                finally:
                    staged.effective_profile.write_bytes(saved)
                assert calls[0]==4
                with (home/'writer.dev.lock').open('rb') as lock:
                    fcntl.flock(lock,fcntl.LOCK_EX|fcntl.LOCK_NB)
                    fcntl.flock(lock,fcntl.LOCK_UN)
                after=snapshot()
                assert after.pop('writer.dev.lock')==b''
                assert after==original
        assert all(p.returncode is not None and p.stdout.closed and p.stderr.closed for p in children)
        assert len(children)==15
    assert list(scratch.iterdir())==[]
    after=snapshot();assert after.pop('writer.dev.lock')==b'';assert after==original
    print('REAL_CHAIN_PREFLIGHT_PASS')

if __name__=='__main__': main(*sys.argv[1:])
