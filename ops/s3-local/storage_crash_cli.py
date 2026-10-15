#!/usr/bin/env python3
"""Authenticated crash READY probe and L-T one-shot execution command."""
import json
import sys
import native_review
import approval_gate
from offline_cli import parse
from launcher_signal import stop_latch
from storage_crash_stage import stage
from storage_crash_ready import ready, crash_arguments
from storage_crash_run import run


def validate_run_report(report, fault_command="Seal"):
    fixed = dict(crash_started=True, crash_verified=False, approval_verified=False,
                 reusable_permit=False, replay_verified=False)
    if type(report) is not dict or set(report) != set(fixed) | {'child_exit', 'child_result', 'outcome'}:
        raise ValueError('REPORT')
    if any(type(report[k]) is not bool or report[k] != v for k, v in fixed.items()):
        raise ValueError('REPORT')
    code = report['child_exit']
    if type(code) is not int or type(report['outcome']) is not str:
        raise ValueError('REPORT')
    if code == 86:
        if report['child_result'] is not None or report['outcome'] != 'UNKNOWN':
            raise ValueError('REPORT')
        return
    if code != 0 or report['outcome'] != 'RECORDED_NOT_REACHED':
        raise ValueError('REPORT')
    result = report['child_result']
    expected = dict(schema='s3-local-crash-'+fault_command.lower()+'-result/1', crash_reached=False,
                    crash_verified=False, durable_ack=False, DEV='NOT_RUN')
    if type(result) is not dict or set(result) != set(expected) | {'command_succeeded'}:
        raise ValueError('REPORT')
    if any(type(result[k]) is not type(v) or result[k] != v for k, v in expected.items()) or \
       type(result['command_succeeded']) is not bool:
        raise ValueError('REPORT')


def main(argv=None):
    try:
        argv = list(sys.argv[1:] if argv is None else argv)
        command = argv.pop(0) if argv else None
        if command not in ('check-ready-reviewed', 'run-reviewed'):
            raise ValueError('COMMAND')
        names = ('--fault-point', '--fault-occurrence', '--fault-purpose', '--fault-evidence-root')
        selected = {}
        for name in names:
            if argv.count(name) != 1:
                raise ValueError('FAULT_OPTIONS')
            pos = argv.index(name)
            if pos + 1 == len(argv) or argv[pos+1].startswith('--'):
                raise ValueError('FAULT_VALUE')
            selected[name] = argv[pos+1]
            del argv[pos:pos+2]
        if argv.count('--enable-storage-crash') != 1:
            raise ValueError('FAULT_OPT_IN')
        argv.remove('--enable-storage-crash')
        value = selected['--fault-occurrence']
        if not value.isascii() or not value.isdecimal() or str(int(value)) != value:
            raise ValueError('OCCURRENCE')
        options = dict(point=selected['--fault-point'], occurrence=int(value),
                       purpose=selected['--fault-purpose'],
                       evidence_root=selected['--fault-evidence-root'], enable=True)
        fault_command = "Seal"
        if '--fault-command' in argv:
            if argv.count('--fault-command') != 1:
                raise ValueError('COMMAND')
            pos = argv.index('--fault-command')
            if pos+1 == len(argv):
                raise ValueError('COMMAND')
            fault_command = argv[pos+1]
            del argv[pos:pos+2]
            options['fault_command'] = fault_command
        crash_arguments(**options)
        a, arguments = parse(argv, reviewed=True)
        decision = native_review._uuid(a.native_decision_id)
        revisions = {role: native_review._uuid(getattr(a, role+'_revision'))
                     for role in ('ceo', 'cto')}
        audit_args = (a.bundle, a.artifacts, a.runtime_pin, a.local_demo_profile,
                      a.acknowledge_unproven_space, decision, revisions)
        with stop_latch() as stop:
            if stop():
                raise ValueError('STOPPED')
            with stage(*audit_args, a.input_set.parent, a.input_set.name,
                       arguments, a.scratch) as staged:
                audit = lambda: approval_gate.inspect(*audit_args)
                if command == 'check-ready-reviewed':
                    with ready(staged, arguments, audit, stop=stop, **options) as report:
                        expected = {'ready': True, 'crash_started': False, 'service_started': False,
                                    'approval_verified': False, 'reusable_permit': False}
                        if type(report) is not dict or set(report) != set(expected) or any(
                                type(report[k]) is not bool or report[k] != v
                                for k, v in expected.items()):
                            raise ValueError('REPORT')
                else:
                    report = run(staged, arguments, audit, stop=stop, **options)
                    validate_run_report(report, fault_command)
                if stop():
                    raise ValueError('STOPPED')
        # Success is emitted only after child reap and private-copy cleanup.
        print(json.dumps(report, sort_keys=True))
        return 0
    except (ValueError, OSError, KeyError, TypeError, KeyboardInterrupt):
        print('LOCAL_STORAGE_CRASH_RUN_REJECTED_OUTCOME_UNKNOWN' if locals().get('command') == 'run-reviewed'
              else 'LOCAL_STORAGE_CRASH_CHECK_REJECTED', file=sys.stderr)
        return 2


if __name__ == '__main__':
    sys.exit(main())
