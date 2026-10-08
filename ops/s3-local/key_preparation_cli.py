#!/usr/bin/env python3
"""Manual Paperclip command for L-T's pre-genesis, public-assets-only web phase."""
from pathlib import Path
import sys
import re
import approval_gate
from offline_cli import Parser
from launcher_signal import stop_latch
from web_pid_mailbox import reporter
import native_review
from key_preparation import run


def parse(argv):
    args=list(argv)
    if not args or args.pop(0)!='prepare-keys-reviewed':raise ValueError('COMMAND')
    opts=[x for x in args if x.startswith('--')]
    if len(opts)!=len(set(opts)) or any('=' in x for x in opts):raise ValueError('ARGUMENTS')
    p=Parser(allow_abbrev=False,add_help=False)
    for name in ('bundle','artifacts','approval-socket','pid-mailbox'):
        p.add_argument('--'+name,type=Path,required=True)
    for name in ('runtime-pin','native-decision-id','ceo-revision','cto-revision','local-demo-profile','web-origin'):
        p.add_argument('--'+name,required=True)
    p.add_argument('--acknowledge-unproven-space',action='store_true')
    a=p.parse_args(args)
    if (a.local_demo_profile!='s3-dev-local/1' or not a.acknowledge_unproven_space
            or a.web_origin not in ('http://127.0.0.1:5173','http://localhost:5173')
            or not re.fullmatch('[0-9a-f]{64}',a.runtime_pin)):raise ValueError('INPUT')
    for name in ('bundle','artifacts','approval_socket','pid_mailbox'):
        path=getattr(a,name)
        if not path.is_absolute() or '..' in path.parts:raise ValueError('PATH')
    for name in ('native_decision_id','ceo_revision','cto_revision'):native_review._uuid(getattr(a,name))
    return a


def main(argv=None):
    try:
        a=parse(sys.argv[1:] if argv is None else argv)
        record=reporter(a.pid_mailbox)
        with stop_latch() as stop,approval_gate.private_transport(a.approval_socket):
            result=run(a.bundle,a.artifacts,a.runtime_pin,a.web_origin,a.native_decision_id,
                {'ceo':a.ceo_revision,'cto':a.cto_revision},stop,record)
        if result.get('listener_closed') is not True:raise ValueError('RELEASE')
        return 0
    except (ValueError,OSError,KeyError,TypeError,KeyboardInterrupt):
        print('LOCAL_KEY_PREPARATION_REJECTED',file=sys.stderr);return 2


if __name__=='__main__':sys.exit(main())
