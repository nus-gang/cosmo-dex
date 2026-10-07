#!/usr/bin/env python3
"""Authenticated pre-create check. Never creates a home or contacts Chain RPC."""
import json
from pathlib import Path
import re
import sys
from offline_cli import Parser
import native_review
from bootstrap_check import check


def parse(argv=None):
    argv = list(sys.argv[1:] if argv is None else argv)
    options = [v for v in argv if v.startswith('--')]
    if len(options) != len(set(options)) or any('=' in v for v in options):
        raise ValueError('ARGUMENTS')
    parser = Parser(allow_abbrev=False, add_help=False)
    for name in ('bundle', 'artifacts', 'input-set', 'effective-profile', 'scratch'):
        parser.add_argument('--'+name, type=Path, required=True)
    for name in ('runtime-pin', 'local-demo-profile', 'native-decision-id',
                 'ceo-revision', 'cto-revision'):
        parser.add_argument('--'+name, required=True)
    parser.add_argument('--acknowledge-unproven-space', action='store_true')
    a = parser.parse_args(argv)
    if a.local_demo_profile != 's3-dev-local/1' or not a.acknowledge_unproven_space:
        raise ValueError('OPT_IN')
    if re.fullmatch('[0-9a-f]{64}', a.runtime_pin) is None:
        raise ValueError('PIN')
    for name in ('bundle', 'artifacts', 'input_set', 'effective_profile', 'scratch'):
        value = getattr(a, name)
        if not value.is_absolute() or '..' in value.parts:
            raise ValueError('PATH')
    decision = native_review._uuid(a.native_decision_id)
    revisions = {role: native_review._uuid(getattr(a, role+'_revision'))
                 for role in ('ceo', 'cto')}
    return a, decision, revisions


def main(argv=None):
    try:
        a, decision, revisions = parse(argv)
        result = check(a.bundle, a.artifacts, a.runtime_pin, a.local_demo_profile,
            a.acknowledge_unproven_space, decision, revisions, a.input_set.parent,
            a.input_set.name, a.effective_profile, a.scratch)
        print(json.dumps(result, sort_keys=True))
        return 0
    except (ValueError, OSError, KeyError, TypeError, KeyboardInterrupt):
        print('LOCAL_BOOTSTRAP_CHECK_REJECTED', file=sys.stderr)
        return 2


if __name__ == '__main__':
    sys.exit(main())
