#!/usr/bin/env python3
"""Offline byte and semantic validation; never starts a service or grants approval."""
import argparse
import json
from pathlib import Path
import sys
from offline_check import check


class Parser(argparse.ArgumentParser):
    def error(self, message):
        raise ValueError('ARGUMENTS')


def parse(argv=None, *, reviewed=False, managed=False):
    argv = list(sys.argv[1:] if argv is None else argv)
    # No abbreviations, equals aliases, repeated options or passthrough argv.
    options = [x for x in argv if x.startswith('--')]
    if len(set(options)) != len(options) or any('=' in x for x in options):
        raise ValueError('ARGUMENTS')
    p = Parser(allow_abbrev=False)
    for name in ('bundle', 'artifacts', 'input-set', 'effective-profile', 'home',
                 'key-directory', 'scratch'):
        p.add_argument('--'+name, type=Path, required=True)
    for name in ('runtime-pin', 'local-demo-profile', 'bind', 'rpc',
                 'lifetime-seconds', 'max-requests', 'max-ticks'):
        p.add_argument('--'+name, required=True)
    if reviewed:
        for name in ('native-decision-id', 'ceo-revision', 'cto-revision'):
            p.add_argument('--'+name, required=True)
    if managed:
        p.add_argument('--approval-socket', type=Path, required=True)
        p.add_argument('--pid-mailbox', type=Path, required=True)
    p.add_argument('--acknowledge-unproven-space', action='store_true')
    a = p.parse_args(argv)
    if a.local_demo_profile != 's3-dev-local/1' or not a.acknowledge_unproven_space:
        raise ValueError('OPT_IN')
    for name in ('bundle', 'artifacts', 'input_set', 'effective_profile',
                 'home', 'key_directory', 'scratch'):
        path = getattr(a, name)
        if not path.is_absolute() or '..' in path.parts:
            raise ValueError('PATH')
    if managed and (not a.approval_socket.is_absolute() or '..' in a.approval_socket.parts):
        raise ValueError('APPROVAL_SOCKET')
    if managed and (not a.pid_mailbox.is_absolute() or '..' in a.pid_mailbox.parts):
        raise ValueError('PID_MAILBOX')
    arguments = ['--input-set', str(a.input_set), '--local-demo-profile',
                 str(a.effective_profile), '--runtime-pin', a.runtime_pin,
                 '--acknowledge-unproven-space']
    for name in ('home', 'key-directory', 'bind', 'rpc', 'lifetime-seconds',
                 'max-requests', 'max-ticks'):
        arguments.extend(['--'+name, str(getattr(a, name.replace('-', '_')))])
    return a, arguments


def main(argv=None):
    try:
        a, arguments = parse(argv)
        result = check(a.bundle, a.artifacts, a.runtime_pin, a.local_demo_profile,
                       a.acknowledge_unproven_space, a.input_set.parent,
                       a.input_set.name, arguments, a.scratch)
        print(json.dumps(result, sort_keys=True))
        return 0
    except (ValueError, OSError, KeyError, TypeError, KeyboardInterrupt):
        # Never expose input bytes, key paths, child output or exception chains.
        print('LOCAL_OFFLINE_CHECK_REJECTED', file=sys.stderr)
        return 2


if __name__ == '__main__':
    sys.exit(main())
