"""Read-only topology subprocess over exact managed packets and staged bytes."""
import json
import os
from pathlib import Path
import tempfile
from bootstrap_stage import _write
from chain_preflight import _supervise
from chain_topology import preflight_payload
from preflight import bounded, checked_root


def check(python, candidate, nodes, packet, staged, *, fee_bps, scratch,
          timeout=60, stopped=lambda: False):
    raw = preflight_payload(python, candidate, nodes, packet, staged, fee_bps=fee_bps)
    # Copy the expected IDs before invoking any caller callback.
    ids = tuple(packet['node_ids'])
    expected = json.dumps(dict(node_ids=ids, approval_verified=False,
        port_availability_verified=False, writer_exclusion_verified=False),
        separators=(',', ':')).encode() + b'\n'
    with tempfile.TemporaryDirectory(prefix='chain-topology-', dir=checked_root(scratch)) as directory:
        root = Path(directory)
        path = root/'topology.json'
        _write(path, raw, 0o600)
        fd = os.open(root, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW)
        try:
            os.fsync(fd)
        finally:
            os.close(fd)
        def verify():
            staged.verify()
            if bounded(checked_root(root), path.name, 65536) != raw:
                raise ValueError('CHAIN_TOPOLOGY_BYTES_CHANGED')
        _supervise([str(staged.executable), 'topology', '--topology', str(path)],
                   verify, expected, timeout=timeout, stopped=stopped)
    return dict(node_ids=list(ids), home_identity_verified=True,
                approval_verified=False, port_availability_verified=False,
                writer_exclusion_verified=False, service_started=False)
