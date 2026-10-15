"""Real Rust/C READY denial; only the organizational approval reader is mocked."""
import hashlib
import os
from unittest.mock import patch
from storage_crash_stage import stage
from storage_crash_ready import ready
from storage_crash_run import run


def verify(root, bundle, artifacts, scratch, manifest, arguments):
    for command in ("Seal", "Apply"):
        _verify_command(root, bundle, artifacts, scratch, manifest, arguments, command)


def _verify_command(root, bundle, artifacts, scratch, manifest, arguments, command):
    pin = hashlib.sha256(manifest).hexdigest()
    original = (root / 'input.json').read_bytes()
    evidence = root / ('crash-evidence-' + command.lower())
    for mode in ('ready', 'revoked', 'stopped', 'interrupt'):
        calls = []
        stopped = []

        def audit():
            calls.append(1)
            if len(calls) == 1:
                return None
            if mode == 'revoked':
                raise ValueError('SYNTHETIC_REVOCATION_AFTER_READY')
            if mode == 'interrupt':
                raise KeyboardInterrupt()
            if mode == 'stopped':
                stopped.append(True)

        with patch('approval_gate.inspect', return_value={'synthetic': 'candidate'}):
            with stage(bundle, artifacts, pin, 's3-dev-local/1', True,
                       'synthetic-decision', {}, root, 'input.json', arguments,
                       scratch) as staged:
                try:
                    with ready(staged, arguments, audit, point='before_wal',
                               occurrence=1, purpose='NORMAL', evidence_root=evidence,
                               enable=True, fault_command=command, stop=lambda: bool(stopped), timeout=15) as report:
                        assert mode == 'ready'
                        assert report['ready'] and not report['crash_started']
                except ValueError as error:
                    expected = {'revoked': 'SYNTHETIC_REVOCATION_AFTER_READY',
                                'stopped': 'STORAGE_CRASH_STOPPED'}
                    assert str(error) == expected.get(mode), (mode, str(error))
                except KeyboardInterrupt:
                    assert mode == 'interrupt'
                else:
                    assert mode == 'ready'
        assert calls == [1, 1], calls
        assert not evidence.exists()
        assert (root / 'input.json').read_bytes() == original
        assert list(scratch.iterdir()) == []


    # The real crash child must never receive START in these final-gate cases.
    for mode in ('revoked', 'capture', 'stopped', 'interrupt'):
        calls = []
        stopped = []
        with patch('approval_gate.inspect', return_value={'synthetic': 'candidate'}):
            with stage(bundle, artifacts, pin, 's3-dev-local/1', True,
                       'synthetic-decision', {}, root, 'input.json', arguments,
                       scratch) as staged:
                def final_audit():
                    calls.append(1)
                    if len(calls) == 3:
                        if mode == 'revoked':
                            return 'revoked'
                        if mode == 'capture':
                            object.__setattr__(staged, 'capture', b'changed')
                        if mode == 'stopped':
                            stopped.append(True)
                        if mode == 'interrupt':
                            raise KeyboardInterrupt()
                    return 'same'
                try:
                    run(staged, arguments, final_audit, point='before_wal',
                        occurrence=1, purpose='NORMAL', evidence_root=evidence,
                        enable=True, fault_command=command, stop=lambda: bool(stopped), timeout=15)
                except ValueError as error:
                    expected = {'revoked': 'APPROVAL_CHANGED_BEFORE_FAULT',
                                'capture': 'CAPTURE_CHANGED',
                                'stopped': 'CRASH_STOPPED_BEFORE_START'}
                    assert str(error) == expected.get(mode), (mode, str(error))
                except KeyboardInterrupt:
                    assert mode == 'interrupt'
                else:
                    raise AssertionError('crash unexpectedly started')
        assert calls == [1, 1, 1], calls
        assert not evidence.exists()
        assert (root / 'input.json').read_bytes() == original
        assert list(scratch.iterdir()) == []
