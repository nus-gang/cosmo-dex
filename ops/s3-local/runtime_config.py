"""Pure Paperclip configuration preparation. No API writes, spawn, or approval."""
from pathlib import Path
import re
import shlex

import native_review
from offline_cli import parse


def shell_command(argv):
    if not argv or any(not isinstance(x, str) or not x or
                       any(ord(c) < 32 or ord(c) == 127 for c in x) for x in argv):
        raise ValueError('COMMAND')
    # Paperclip templates cwd/env separately. Reject template syntax too so
    # this literal command remains safe if template rendering is later added.
    if any('{{' in x or '}}' in x for x in argv):
        raise ValueError('TEMPLATE')
    return 'exec ' + shlex.join(argv)


def worker_config(python, candidate, argv, *, fee_bps):
    """Return a manual, unexposed worker config; caller must supply a live current-run broker socket.

    This is a complete replacement config for a dedicated project workspace,
    never a patch to the shared primary workspace's service list.
    """
    if type(fee_bps) is not int or fee_bps not in (0, 25):
        raise ValueError('FEE')
    python, candidate = Path(python), Path(candidate)
    if any(not p.is_absolute() or '..' in p.parts for p in (python, candidate)):
        raise ValueError('PATH')
    argv = list(argv)
    a, _ = parse(argv, reviewed=True, managed=True)
    native_review._uuid(a.native_decision_id)
    native_review._uuid(a.ceo_revision)
    native_review._uuid(a.cto_revision)
    if not re.fullmatch('[0-9a-f]{64}', a.runtime_pin):
        raise ValueError('PIN')
    for name, limit in [('lifetime_seconds', 300), ('max_requests', 10000), ('max_ticks', 10000)]:
        raw = getattr(a, name)
        if not re.fullmatch('[1-9][0-9]*', raw) or not 1 <= int(raw) <= limit:
            raise ValueError('LIMIT')
    bind = re.fullmatch(r'127\.0\.0\.1:([1-9][0-9]*)', a.bind)
    rpc = re.fullmatch(r'127\.0\.0\.1:([1-9][0-9]*)', a.rpc)
    if not bind or not rpc:
        raise ValueError('LOOPBACK')
    port, rpc_port = int(bind[1]), int(rpc[1])
    if not all(1024 <= p <= 65535 for p in (port, rpc_port)) or port == rpc_port:
        raise ValueError('PORT')
    name = f's3-worker-fee{fee_bps}'
    command = shell_command([str(python), '-B', str(candidate / 'ops/s3-local/managed_cli.py'),
                             'serve-reviewed', *argv])
    # No token/env placeholders, exposure, install/setup, or restart directives.
    # manual also survives an explicit targeted start in current Paperclip.
    return {'runtimeConfig': {
        'desiredState': 'manual', 'serviceStates': {'0': 'manual'},
        'workspaceRuntime': {'commands': [{
            'id': name, 'name': name, 'kind': 'service', 'command': command,
            'cwd': str(candidate), 'port': port, 'lifecycle': 'shared',
            'reuseScope': 'project_workspace',
        }]},
    }}


def web_config(python, candidate, argv, *, fee_bps):
    """Pure manual web command; no registration, listener or approval effect."""
    from web_cli import parse_web
    if type(fee_bps) is not int or fee_bps not in (0, 25):
        raise ValueError('FEE')
    python, candidate = Path(python), Path(candidate)
    if any(not p.is_absolute() or '..' in p.parts for p in (python, candidate)):
        raise ValueError('PATH')
    argv = list(argv)
    a, *_ = parse_web(['serve-web-reviewed', *argv])
    if not re.fullmatch('[0-9a-f]{64}', a.runtime_pin):
        raise ValueError('PIN')
    name = f's3-web-fee{fee_bps}'
    command = shell_command([str(python), '-B', str(candidate / 'ops/s3-local/web_cli.py'),
                             'serve-web-reviewed', *argv])
    return {'runtimeConfig': {
        'desiredState': 'manual', 'serviceStates': {'0': 'manual'},
        'workspaceRuntime': {'commands': [{
            'id': name, 'name': name, 'kind': 'service', 'command': command,
            'cwd': str(candidate), 'port': 5173, 'lifecycle': 'shared',
            'reuseScope': 'project_workspace',
        }]},
    }}


def chain_config(python, candidate, argv, *, fee_bps, validator_index):
    """One manual Chain command per dedicated validator workspace; no effects."""
    from chain_cli import parse as parse_chain
    if type(fee_bps) is not int or fee_bps not in (0, 25):
        raise ValueError('FEE')
    if type(validator_index) is not int or not 0 <= validator_index < 4:
        raise ValueError('VALIDATOR')
    python, candidate = Path(python), Path(candidate)
    if any(not p.is_absolute() or '..' in p.parts for p in (python, candidate)):
        raise ValueError('PATH')
    argv = list(argv)
    a = parse_chain(['serve-chain-reviewed', *argv])
    name = f's3-chain-fee{fee_bps}-v{validator_index}'
    command = shell_command([str(python), '-B', str(candidate / 'ops/s3-local/chain_cli.py'),
                             'serve-chain-reviewed', *argv])
    return {'runtimeConfig': {
        'desiredState': 'manual', 'serviceStates': {'0': 'manual'},
        'workspaceRuntime': {'commands': [{
            'id': name, 'name': name, 'kind': 'service', 'command': command,
            'cwd': str(candidate), 'port': int(a.rpc.split(':')[1]),
            'lifecycle': 'shared', 'reuseScope': 'project_workspace',
        }]},
    }}
