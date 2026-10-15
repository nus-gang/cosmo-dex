#!/usr/bin/env python3
"""L-T foreground service entrypoint for Paperclip runtime configuration.

This CLI does not register a runtime or prove ownership from environment flags.
Only configure/start it through Paperclip after L-R exact-candidate approval.
"""
import sys
import native_review
from pid_mailbox import reporter
from approval_gate import private_transport
from offline_cli import parse
from launcher_signal import run


def main(argv=None):
    try:
        argv = list(sys.argv[1:] if argv is None else argv)
        if not argv or argv.pop(0) != 'serve-reviewed':
            raise ValueError('COMMAND')
        a, arguments = parse(argv, reviewed=True, managed=True)
        decision = native_review._uuid(a.native_decision_id)
        revisions = {role: native_review._uuid(getattr(a, role+'_revision'))
                     for role in ('ceo', 'cto')}
        # Bound parent lifetime too; reject aliases, signs and floating values.
        value = a.lifetime_seconds
        if not value.isascii() or not value.isdecimal() or str(int(value)) != value:
            raise ValueError('LIFETIME')
        lifetime = int(value)
        if not 1 <= lifetime <= 300:
            raise ValueError('LIFETIME')
        on_spawn = reporter(a.pid_mailbox)
        with private_transport(a.approval_socket):
            result = run(a.bundle, a.artifacts, a.runtime_pin, a.local_demo_profile,
                         a.acknowledge_unproven_space, decision, revisions,
                         a.input_set.parent, a.input_set.name, arguments, a.scratch,
                         lifetime=lifetime, on_spawn=on_spawn)
        if result != {'worker_exit': 0, 'reusable_permit': False}:
            raise ValueError('RESULT')
        return 0
    except (ValueError, OSError, KeyError, TypeError, KeyboardInterrupt):
        print('LOCAL_MANAGED_WORKER_REJECTED', file=sys.stderr)
        return 2


if __name__ == '__main__':
    sys.exit(main())
