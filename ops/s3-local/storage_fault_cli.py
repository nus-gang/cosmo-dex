#!/usr/bin/env python3
"""Authenticated fault READY probe and L-T one-shot execution command."""
import json
import sys
import native_review
import approval_gate
from offline_cli import parse
from launcher_signal import stop_latch
from storage_fault_stage import stage
from storage_fault_ready import ready, fault_arguments
from storage_fault_run import run


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
        if argv.count('--enable-storage-fault') != 1:
            raise ValueError('FAULT_OPT_IN')
        argv.remove('--enable-storage-fault')
        value = selected['--fault-occurrence']
        if not value.isascii() or not value.isdecimal() or str(int(value)) != value:
            raise ValueError('OCCURRENCE')
        options = dict(point=selected['--fault-point'], occurrence=int(value),
                       purpose=selected['--fault-purpose'],
                       evidence_root=selected['--fault-evidence-root'], enable=True)
        if '--fault-errno' in argv:
            if argv.count('--fault-errno') != 1:
                raise ValueError('FAULT_ERRNO')
            pos = argv.index('--fault-errno')
            if pos + 1 == len(argv):
                raise ValueError('FAULT_ERRNO')
            options['errno'] = argv[pos+1]
            del argv[pos:pos+2]
        operation = 'Seal'
        if '--fault-command' in argv:
            if argv.count('--fault-command') != 1:
                raise ValueError('FAULT_COMMAND')
            pos = argv.index('--fault-command')
            if pos + 1 == len(argv):
                raise ValueError('FAULT_COMMAND')
            operation = argv[pos+1]
            del argv[pos:pos+2]
            if operation not in ('Seal', 'Apply'):
                raise ValueError('FAULT_COMMAND')
        if operation == 'Apply' and options['purpose'] != 'NORMAL':
            raise ValueError('FAULT_COMMAND_PURPOSE')
        fault_arguments(**options)
        if operation != 'Seal':
            options['operation'] = operation
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
                        if report != {'ready': True, 'fault_started': False, 'service_started': False,
                                      'approval_verified': False, 'reusable_permit': False}:
                            raise ValueError('REPORT')
                else:
                    report = run(staged, arguments, audit, stop=stop, **options)
                    expected = dict(child_exit=0, fault_started=True, approval_verified=False,
                                    reusable_permit=False, replay_verified=False)
                    if type(report) is not dict or set(report) != set(expected) | {'child_result'}:
                        raise ValueError('REPORT')
                    if any(type(report[k]) is not type(v) or report[k] != v
                           for k, v in expected.items()):
                        raise ValueError('REPORT')
                    result = report['child_result']
                    fixed = dict(schema='s3-local-fault-'+operation.lower()+'-result/1', durable_ack=False, DEV='NOT_RUN')
                    if type(result) is not dict or set(result) != set(fixed) | {'command_succeeded', 'injected'}:
                        raise ValueError('REPORT')
                    if any(type(result[k]) is not type(v) or result[k] != v for k, v in fixed.items()) or \
                       any(type(result[k]) is not bool for k in ('command_succeeded', 'injected')):
                        raise ValueError('REPORT')
                if stop():
                    raise ValueError('STOPPED')
        # Success is emitted only after child reap and private-copy cleanup.
        print(json.dumps(report, sort_keys=True))
        return 0
    except (ValueError, OSError, KeyError, TypeError, KeyboardInterrupt):
        print('LOCAL_STORAGE_FAULT_RUN_REJECTED_OUTCOME_UNKNOWN' if locals().get('command') == 'run-reviewed'
              else 'LOCAL_STORAGE_FAULT_CHECK_REJECTED', file=sys.stderr)
        return 2


if __name__ == '__main__':
    sys.exit(main())
