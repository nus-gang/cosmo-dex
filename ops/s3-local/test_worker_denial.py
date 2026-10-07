"""Real worker preparation/denial test only. Never transmits START."""
import hashlib
import os
from pathlib import Path
import signal
import socket
import subprocess
import sys
import tempfile


def check(executable, capture, args):
    with tempfile.TemporaryDirectory(dir=os.environ["PAPERCLIP_RUN_SCRATCH_DIR"]) as directory:
        helper = Path(directory).resolve() / "helper"
        helper.write_bytes(b"offline denial only")
        helper.chmod(0o500)
        return _check(executable, capture, ["--direct-helper", str(helper),
            "--direct-helper-sha256", hashlib.sha256(helper.read_bytes()).hexdigest(), *args])

def _check(executable, capture, args):
    for mode in ('eof', 'invalid', 'signal'):
        with tempfile.TemporaryFile() as source:
            source.write(capture)
            source.seek(0)
            parent, child = socket.socketpair()
            with parent, child:
                parent.settimeout(30)
                proc = subprocess.Popen([executable, 'serve-captured', '--start-gate-fd',
                    str(child.fileno()), '--capture-sha256', hashlib.sha256(capture).hexdigest(),
                    *args], stdin=source, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                    pass_fds=(child.fileno(),), env={'PATH': '/usr/bin:/bin'}, start_new_session=True)
                child.close()
                try:
                    ready = b''
                    while len(ready) < 6:
                        part = parent.recv(6-len(ready))
                        if not part:
                            raise AssertionError('worker closed before READY')
                        ready += part
                    assert ready == b'READY\n'
                    if mode == 'invalid':
                        parent.sendall(b'DENY\n')
                    if mode == 'signal':
                        proc.send_signal(signal.SIGTERM)
                    else:
                        parent.shutdown(socket.SHUT_WR)
                    stdout, stderr = proc.communicate(timeout=10)
                    assert proc.returncode == 2 and stdout == b'' and stderr == b'LOCAL_WORKER_REJECTED\n'
                finally:
                    if proc.poll() is None:
                        os.killpg(proc.pid, signal.SIGKILL)
                    proc.wait(timeout=5)
                    proc.stdout.close()
                    proc.stderr.close()
    print('WORKER_DENIAL_PASS')

if __name__ == '__main__':
    check(sys.argv[1], Path(sys.argv[2]).read_bytes(), sys.argv[3:])
