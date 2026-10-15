#!/usr/bin/env python3
"""Authenticated F14 READY check and L-T one-shot execution command."""
import json
import sys
import native_review
import approval_gate
from offline_cli import parse
from launcher_signal import stop_latch
from correction_stage import stage
from correction_ready import ready, correction_arguments
from correction_run import run, decode_report


def validate_run_report(report, occurrence):
    fixed = dict(child_exit=0, fault_started=True, approval_verified=False,
                 reusable_permit=False, replay_verified=False, F14_verified=False)
    if type(report) is not dict or set(report) != set(fixed) | {"child_result"}:
        raise ValueError("REPORT")
    if any(type(report[k]) is not type(v) or report[k] != v for k,v in fixed.items()):
        raise ValueError("REPORT")
    raw = (json.dumps(report["child_result"], sort_keys=True, separators=(",", ":"))+"\n").encode()
    decode_report(raw, occurrence)


def main(argv=None):
    try:
        argv = list(sys.argv[1:] if argv is None else argv)
        command = argv.pop(0) if argv else None
        if command not in ('check-ready-reviewed', 'run-reviewed'):
            raise ValueError('COMMAND')
        selected = {}
        for name in ('--fault-occurrence', '--fault-evidence-root'):
            if argv.count(name) != 1:
                raise ValueError('F14_OPTIONS')
            pos = argv.index(name)
            if pos + 1 == len(argv) or argv[pos+1].startswith('--'):
                raise ValueError('F14_VALUE')
            selected[name] = argv[pos+1]
            del argv[pos:pos+2]
        if argv.count('--enable-f14-prepare') != 1:
            raise ValueError('F14_OPT_IN')
        argv.remove('--enable-f14-prepare')
        value = selected['--fault-occurrence']
        if not value.isascii() or not value.isdecimal() or str(int(value)) != value:
            raise ValueError('OCCURRENCE')
        options = dict(occurrence=int(value), evidence_root=selected['--fault-evidence-root'], enable=True)
        correction_arguments(**options)
        a, arguments = parse(argv, reviewed=True)
        decision = native_review._uuid(a.native_decision_id)
        revisions = {role: native_review._uuid(getattr(a, role+'_revision')) for role in ('ceo', 'cto')}
        audit_args = (a.bundle, a.artifacts, a.runtime_pin, a.local_demo_profile,
                      a.acknowledge_unproven_space, decision, revisions)
        with stop_latch() as stop:
            if stop():
                raise ValueError('STOPPED')
            with stage(*audit_args, a.input_set.parent, a.input_set.name, arguments, a.scratch) as staged:
                audit = lambda: approval_gate.inspect(*audit_args)
                if command == 'check-ready-reviewed':
                    with ready(staged, arguments, audit, stop=stop, **options) as report:
                        expected = dict(ready=True, fault_started=False, service_started=False,
                                        F14_verified=False, approval_verified=False, reusable_permit=False)
                        if type(report) is not dict or set(report) != set(expected) or any(
                                type(report[k]) is not bool or report[k] != v for k,v in expected.items()):
                            raise ValueError('REPORT')
                else:
                    report = run(staged, arguments, audit, stop=stop, **options)
                    validate_run_report(report, options['occurrence'])
                if stop():
                    raise ValueError('STOPPED')
        print(json.dumps(report, sort_keys=True))
        return 0
    except (ValueError, OSError, KeyError, TypeError, KeyboardInterrupt):
        print('LOCAL_F14_RUN_REJECTED_OUTCOME_UNKNOWN' if locals().get('command') == 'run-reviewed'
              else 'LOCAL_F14_CHECK_REJECTED', file=sys.stderr)
        return 2


if __name__ == '__main__':
    sys.exit(main())
