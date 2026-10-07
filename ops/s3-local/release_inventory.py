"""Bind trusted launcher inventory to one-shot release probes.

Not a serialized permit or process discovery mechanism. The caller must collect
all PIDs from the managed launcher and endpoints from its pinned configuration.
This freezes inputs but does not establish inventory completeness.
"""
import copy
import hashlib
from pathlib import Path

import process_release
import port_release
import writer_release

ERROR = 'RELEASE_INVENTORY_REJECTED'


def probes(pids, endpoints, raw, artifacts, arguments, scratch, validator_sha256):
    return _probes(pids, endpoints, raw, artifacts, arguments, scratch,
                   validator_sha256, process_release.check, port_release.check,
                   writer_release.check)


def _probes(pids, endpoints, raw, artifacts, arguments, scratch, validator_sha256,
            process, port, writer):
    try:
        if (not isinstance(pids, (list, tuple)) or not 1 <= len(pids) <= 32 or
                any(type(p) is not int or not 2 <= p <= 2147483647 for p in pids) or
                len(set(pids)) != len(pids) or
                not isinstance(endpoints, (list, tuple)) or not 1 <= len(endpoints) <= 32):
            raise ValueError()
        frozen_ports = []
        for entry in endpoints:
            if (not isinstance(entry, (list, tuple)) or len(entry) != 2 or
                    entry[0] not in ('127.0.0.1', '::1') or type(entry[1]) is not int or
                    not 1024 <= entry[1] <= 65535):
                raise ValueError()
            frozen_ports.append(tuple(entry))
        if (len(set(frozen_ports)) != len(frozen_ports) or not isinstance(raw, bytes) or
                not raw or not isinstance(artifacts, (str, Path)) or
                not isinstance(arguments, (list, tuple)) or not arguments or
                any(not isinstance(a, str) for a in arguments) or
                not isinstance(validator_sha256, str) or len(validator_sha256) != 64 or
                any(c not in '0123456789abcdef' for c in validator_sha256)):
            raise ValueError()
        scratch, artifacts = Path(scratch), Path(artifacts)
        if (not scratch.is_absolute() or '..' in scratch.parts or
                not artifacts.is_absolute() or '..' in artifacts.parts):
            raise ValueError()
        frozen_pids, frozen_ports = tuple(pids), tuple(frozen_ports)
        # Freeze the path, not a mock artifact map. validate_snapshot reopens
        # the actual validator and checks bytes against the captured descriptor.
        frozen_artifacts, frozen_args = artifacts, tuple(arguments)
        digest = hashlib.sha256(raw).hexdigest()
    except Exception:
        raise ValueError(ERROR) from None

    used = set()
    def once(name, invoke, matches):
        # Consume before IO, including exceptions/interrupts. No implicit retry.
        if name in used:
            raise ValueError(ERROR)
        used.add(name)
        try:
            report = invoke()
            if not isinstance(report, dict) or not matches(report):
                raise ValueError()
            report = copy.deepcopy(report)
            report['capture_sha256'] = digest
            report['inventory_complete_verified'] = False
            return report
        except (KeyboardInterrupt, SystemExit):
            raise
        except Exception:
            raise ValueError(ERROR) from None

    return {
        'process_probe': lambda: once('process', lambda: process(frozen_pids),
            lambda r: r.get('pids') == list(frozen_pids)),
        'port_probe': lambda: once('port', lambda: port(frozen_ports),
            lambda r: r.get('endpoints') == list(frozen_ports)),
        'writer_probe': lambda: once('writer', lambda: writer(raw,
            copy.deepcopy(frozen_artifacts), frozen_args, scratch),
            lambda r: r.get('validator_sha256') == validator_sha256),
    }
