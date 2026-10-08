"""Prepare a board-authorized registration packet; never sends an API request.

Installed Paperclip forbids agent host-command writes. This packet is neither
runtime approval nor a way around that policy. Refresh current-run IPC before
registration; a stored expired socket command is deliberately unusable.
"""
import copy
from pathlib import Path
from native_review import COMPANY, _uuid
from runtime_config import worker_config, web_config, chain_config

PROJECT = 'b29572d7-fa92-4e4c-8370-a08aa50ee11a'
ERROR = 'WORKSPACE_REGISTRATION_REJECTED'


def prepare(python, candidate, argv, *, fee_bps):
    config = worker_config(python, candidate, argv, fee_bps=fee_bps)
    candidate = str(Path(candidate))
    body = {'name': f'nus-s3-local-fee{fee_bps}', 'sourceType': 'local_path',
            'cwd': candidate, 'isPrimary': False, **config}
    return {'method': 'POST', 'path': f'/api/projects/{PROJECT}/workspaces',
            'body': body, 'requires_board_registration': True,
            'approval_verified': False, 'starts_service': False}


def registered(packet, workspaces):
    """Match a fresh authenticated workspace listing to the prepared request.

    Strict equality refuses command/env/config drift and duplicate registrations.
    This does not eliminate read/start races or certify runtime approval. Caller
    must use the current broker and existing fresh launch audits in L-T.
    """
    try:
        body = packet['body']
        # Validate packet structure, rather than accepting arbitrary request URLs.
        if (packet['method'] != 'POST' or
                packet['path'] != f'/api/projects/{PROJECT}/workspaces' or
                packet['requires_board_registration'] is not True or
                packet['approval_verified'] is not False or
                packet['starts_service'] is not False or not isinstance(workspaces, list)):
            raise ValueError()
        matches = [w for w in workspaces if w.get('name') == body['name']]
        if len(matches) != 1:
            raise ValueError()
        w = matches[0]
        wid = _uuid(w['id'])
        if (w['companyId'] != COMPANY or w['projectId'] != PROJECT or
                w['isPrimary'] is not False or w['sourceType'] != 'local_path' or
                w['cwd'] != body['cwd'] or w['runtimeConfig'] != body['runtimeConfig'] or
                w['runtimeServices'] != [] or
                any(w.get(k) is not None for k in (
                    'setupCommand', 'cleanupCommand', 'sharedWorkspaceKey',
                    'remoteProvider', 'remoteWorkspaceRef', 'repoUrl'))):
            raise ValueError()
        commands = body['runtimeConfig']['workspaceRuntime']['commands']
        if len(commands) != 1:
            raise ValueError()
        command_id = commands[0]['id']
        if command_id not in ('s3-worker-fee0', 's3-worker-fee25', 's3-web-fee0', 's3-web-fee25',
                *(f's3-chain-fee{fee}-v{index}' for fee in (0, 25) for index in range(4))):
            raise ValueError()
        base = f'/api/projects/{PROJECT}/workspaces/{wid}/runtime-services/'
        return {'workspace_id': wid, 'approval_verified': False,
                'requests': {action: {'method': 'POST', 'path': base + action,
                    'body': {'workspaceCommandId': command_id}}
                    for action in ('start', 'stop')},
                'registered_config': copy.deepcopy(w['runtimeConfig'])}
    except (KeyError, TypeError, AttributeError, ValueError):
        raise ValueError(ERROR) from None


def prepare_web(python, candidate, argv, *, fee_bps):
    """Separate dedicated web workspace; never alters worker registration."""
    config = web_config(python, candidate, argv, fee_bps=fee_bps)
    return {'method': 'POST', 'path': f'/api/projects/{PROJECT}/workspaces',
            'body': {'name': f'nus-s3-local-web-fee{fee_bps}',
                     'sourceType': 'local_path', 'cwd': str(Path(candidate)),
                     'isPrimary': False, **config},
            'requires_board_registration': True,
            'approval_verified': False, 'starts_service': False}


def prepare_chain(python, candidate, argv, *, fee_bps, validator_index):
    """Pure dedicated-validator packet; host-command registration still needs authority."""
    config = chain_config(python, candidate, argv, fee_bps=fee_bps,
                          validator_index=validator_index)
    return {'method': 'POST', 'path': f'/api/projects/{PROJECT}/workspaces',
            'body': {'name': f'nus-s3-local-chain-fee{fee_bps}-v{validator_index}',
                     'sourceType': 'local_path', 'cwd': str(Path(candidate)),
                     'isPrimary': False, **config},
            'requires_board_registration': True,
            'approval_verified': False, 'starts_service': False}
