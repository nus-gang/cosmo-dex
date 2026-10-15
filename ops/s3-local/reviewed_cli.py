#!/usr/bin/env python3
"""Authenticated, reviewed OFFLINE validation. No service-start command."""
import json
import sys
import native_review
from offline_cli import parse
from reviewed_check import check


def main(argv=None):
    try:
        a, arguments = parse(argv, reviewed=True)
        # Reject malformed handoff references before filesystem, API or child IO.
        decision = native_review._uuid(a.native_decision_id)
        revisions = {role: native_review._uuid(getattr(a, role+'_revision'))
                     for role in ('ceo', 'cto')}
        result = check(a.bundle, a.artifacts, a.runtime_pin, a.local_demo_profile,
                       a.acknowledge_unproven_space, decision, revisions,
                       a.input_set.parent, a.input_set.name, arguments, a.scratch)
        print(json.dumps(result, sort_keys=True))
        return 0
    except (ValueError, OSError, KeyError, TypeError, KeyboardInterrupt):
        print('LOCAL_REVIEWED_CHECK_REJECTED', file=sys.stderr)
        return 2


if __name__ == '__main__':
    sys.exit(main())
