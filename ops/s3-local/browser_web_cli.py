#!/usr/bin/env python3
"""Registered once BEFORE keys; preparation and final web share a bounded run."""
from pathlib import Path
import sys
import approval_gate
from launcher_signal import stop_latch
from web_cli import parse_web
from web_pid_mailbox import reporter
from browser_web import run


def parse(argv):
    argv=list(argv)
    if not argv or argv.pop(0)!='serve-browser-reviewed' or argv.count('--browser-control')!=1:raise ValueError('ARGUMENTS')
    at=argv.index('--browser-control')
    if at+1>=len(argv):raise ValueError('ARGUMENTS')
    root=Path(argv[at+1]);del argv[at:at+2]
    result=parse_web(['serve-web-reviewed',*argv]);a=result[0]
    if not root.is_absolute() or '..' in root.parts or a.response_loss_sha256 is not None or result[3]!='http://127.0.0.1:5173' or result[5][0]>300:
        raise ValueError('BROWSER_INPUT')
    if any(root==p or root in p.parents or p in root.parents for p in (a.home,a.pid_mailbox,a.scratch,a.input_set,result[2].parent)):
        raise ValueError('BROWSER_ROOT')
    a.browser_control=root;a.web_origin=result[3];a.approval_socket=result[2]
    return result


def main(argv=None):
    try:
        a,arguments,endpoint,origin,port,values,decision,revisions=parse(sys.argv[1:] if argv is None else argv)
        with stop_latch() as stop,approval_gate.private_transport(endpoint):
            reporter(a.pid_mailbox)()
            result=run(a,arguments,origin,port,values,decision,revisions,stop)
        if result.get('listener_closed') is not True:raise ValueError('RELEASE')
        return 0
    except (Exception,KeyboardInterrupt):
        print('LOCAL_MANAGED_BROWSER_REJECTED',file=sys.stderr);return 2

if __name__=='__main__':sys.exit(main())
