#!/usr/bin/env python3
"""L-T only: foreground web command for an approved Paperclip runtime."""
import re
import sys
from pathlib import Path
import native_review
from web_pid_mailbox import reporter
from offline_cli import parse
from approval_gate import private_transport
from launcher_signal import stop_latch
from managed_web import run


def parse_web(argv):
    argv = list(sys.argv[1:] if argv is None else argv)
    if not argv or argv.pop(0) != 'serve-web-reviewed':
        raise ValueError('COMMAND')
    fault_hash = None
    fault_option = '--drop-broadcast-response-sha256'
    if fault_option in argv:
        at = argv.index(fault_option)
        if argv.count(fault_option) != 1 or at + 1 >= len(argv):
            raise ValueError('FAULT_ARGUMENT')
        fault_hash = argv[at+1]
        if re.fullmatch('[0-9a-f]{64}', fault_hash) is None:
            raise ValueError('FAULT_ARGUMENT')
        del argv[at:at+2]
    evidence = None
    option = '--fault-evidence-root'
    if option in argv:
        at = argv.index(option)
        if argv.count(option) != 1 or at + 1 >= len(argv):
            raise ValueError('FAULT_EVIDENCE_ARGUMENT')
        evidence = Path(argv[at+1])
        if not evidence.is_absolute() or '..' in evidence.parts:
            raise ValueError('FAULT_EVIDENCE_PATH')
        del argv[at:at+2]
    if (fault_hash is None) != (evidence is None):
        raise ValueError('FAULT_EVIDENCE_REQUIRED')
    extra = {}
    for name in ('approval-socket', 'web-origin', 'pid-mailbox'):
        option = '--' + name
        if argv.count(option) != 1:
            raise ValueError('ARGUMENTS')
        at = argv.index(option)
        if at + 1 >= len(argv) or argv[at+1].startswith('--'):
            raise ValueError('ARGUMENTS')
        extra[name] = argv[at+1]
        del argv[at:at+2]
    a, arguments = parse(argv, reviewed=True)
    a.response_loss_sha256 = fault_hash
    a.fault_evidence_root = evidence
    endpoint = Path(extra['approval-socket'])
    if not endpoint.is_absolute() or '..' in endpoint.parts:
        raise ValueError('APPROVAL_SOCKET')
    mailbox = Path(extra['pid-mailbox'])
    if not mailbox.is_absolute() or '..' in mailbox.parts:
        raise ValueError('PID_MAILBOX')
    a.pid_mailbox = mailbox
    origin = extra['web-origin']
    if origin not in ('http://127.0.0.1:5173', 'http://localhost:5173'):
        raise ValueError('ORIGIN')
    if not re.fullmatch(r'127\.0\.0\.1:[1-9][0-9]{3,4}', a.bind):
        raise ValueError('WORKER_BIND')
    port = int(a.bind.split(':')[1])
    if not 1024 <= port <= 65535 or port == 5173:
        raise ValueError('WORKER_PORT')
    values = []
    for value, limit in ((a.lifetime_seconds, 300), (a.max_requests, 10000)):
        if not re.fullmatch(r'[1-9][0-9]*', value) or len(value) > 5 or int(value) > limit:
            raise ValueError('LIMIT')
        values.append(int(value))
    decision = native_review._uuid(a.native_decision_id)
    revisions = {role: native_review._uuid(getattr(a, role+'_revision'))
                 for role in ('ceo', 'cto')}
    return a, arguments, endpoint, origin, port, values, decision, revisions


def main(argv=None):
    try:
        a, arguments, endpoint, origin, port, values, decision, revisions = parse_web(argv)
        record = reporter(a.pid_mailbox)
        with stop_latch() as stop, private_transport(endpoint):
            result = run(a.bundle, a.artifacts, a.runtime_pin, a.local_demo_profile,
                a.acknowledge_unproven_space, decision, revisions,
                a.input_set.parent, a.input_set.name, arguments, a.scratch,
                origin, port, max_requests=values[1], lifetime_seconds=values[0], stop=stop, on_ready=record,
                response_loss_sha256=a.response_loss_sha256, fault_evidence_root=a.fault_evidence_root)
        if (type(result) is not dict or result.get('listener_closed') is not True or
            result.get('approval_verified') is not False or result.get('reusable_permit') is not False or
            type(result.get('handled_connections')) is not int or
            not 0 <= result['handled_connections'] <= values[1] or
            any(not isinstance(result.get(k), str) or not re.fullmatch('[0-9a-f]{64}', result[k])
                for k in ('capture_sha256', 'validator_sha256'))):
            raise ValueError('RESULT')
        return 0
    except (ValueError, OSError, KeyError, TypeError, KeyboardInterrupt):
        print('LOCAL_MANAGED_WEB_REJECTED', file=sys.stderr)
        return 2


if __name__ == '__main__':
    sys.exit(main())
