"""L-T authenticated fetch-to-create composition. Never retry or repair a home."""
from pathlib import Path
import approval_gate
from bootstrap_fetch_cli import execute_raw
from bootstrap_stage import stage
from bootstrap_run import run
from preflight import verify_input_set
from bootstrap_diagnostic import diagnostic


def initialize(args, decision, revisions, extra, home, stopped=lambda: False):
    with diagnostic(extra.evidence_root) as trace:
        return _initialize(args, decision, revisions, extra, home, stopped, trace)


def _initialize(args, decision, revisions, extra, home, stopped, trace):
    home = Path(home)
    if not home.is_absolute() or '..' in home.parts or home.exists() or home.is_symlink():
        raise ValueError('NEW_HOME_REQUIRED')
    revisions = dict(revisions)
    def audit():
        if stopped():
            raise ValueError('INITIALIZE_STOPPED')
        return approval_gate.inspect(args.bundle, args.artifacts, args.runtime_pin,
            args.local_demo_profile, args.acknowledge_unproven_space, decision, revisions)
    trace.emit('initial_audit')
    baseline = audit()
    trace.emit('input_capture')
    captured, _ = verify_input_set(args.bundle, args.artifacts, args.runtime_pin,
        args.local_demo_profile, args.acknowledge_unproven_space,
        args.input_set.parent, args.input_set.name)
    # This persists full/partial transport evidence before any create child.
    # Do not re-read a caller-controlled path as trusted RPC provenance.
    _, rpc = execute_raw(args, decision, revisions, extra, stopped, trace)
    def current():
        trace.emit('create_audit')
        value = audit()
        if value != baseline:
            raise ValueError('INITIALIZE_APPROVAL_CHANGED')
        raw, _ = verify_input_set(args.bundle, args.artifacts, args.runtime_pin,
            args.local_demo_profile, args.acknowledge_unproven_space,
            args.input_set.parent, args.input_set.name)
        if raw != captured:
            raise ValueError('INITIALIZE_INPUT_CHANGED')
        return value
    current()
    trace.emit('create_stage')
    with stage(args.bundle, args.artifacts, args.runtime_pin,
            args.local_demo_profile, args.acknowledge_unproven_space,
            decision, revisions, args.input_set.parent, args.input_set.name,
            args.effective_profile, rpc, args.scratch) as staged:
        if staged.capture != captured:
            raise ValueError('INITIALIZE_CAPTURE_CHANGED')
        current()
        trace.emit('create_run')
        result = run(staged, home, extra.evidence_root, args.runtime_pin,
                   args.effective_profile, current, stop=stopped)

        trace.emit('complete')
        return result
