"""Validate authenticated control-plane observations, never host exit proof.

No polling or API writes here. The caller supplies a fresh post-stop listing.
An absent row is inconclusive, including after an ambiguous start response.
"""
from datetime import datetime
from native_review import COMPANY, _uuid
from workspace_registration import PROJECT

ERROR = 'RUNTIME_EVIDENCE_REJECTED'


def _time(value):
    if not isinstance(value, str) or not value.endswith('Z'):
        raise ValueError()
    return datetime.fromisoformat(value[:-1] + '+00:00')


def operation(response, action, workspace_id, command):
    try:
        op, workspace = response['operation'], response['workspace']
        metadata = op['metadata']
        _uuid(op['id'])
        if (action not in ('start', 'stop') or op['companyId'] != COMPANY or
                op['status'] != 'succeeded' or
                op['phase'] != ('workspace_teardown' if action == 'stop' else 'workspace_provision') or
                op['command'] != command['command'] or op['cwd'] != command['cwd'] or
                metadata['action'] != action or metadata['projectId'] != PROJECT or
                metadata['projectWorkspaceId'] != workspace_id or
                metadata['workspaceCommandId'] != command['id'] or
                metadata['workspaceCommandKind'] != 'service' or
                _time(op['finishedAt']) < _time(op['startedAt']) or
                workspace['id'] != workspace_id or workspace['companyId'] != COMPANY or
                workspace['projectId'] != PROJECT):
            raise ValueError()
        return op['id']
    except (ValueError, KeyError, TypeError, AttributeError, OverflowError):
        raise ValueError(ERROR) from None


def stopped(response, listing, workspace_id, config):
    try:
        command, = config['workspaceRuntime']['commands']
        operation_id = operation(response, 'stop', workspace_id, command)
        rows = [w for w in listing if w['id'] == workspace_id]
        fresh, = rows
        observed = []
        for workspace in (response['workspace'], fresh):
            if (workspace['companyId'] != COMPANY or workspace['projectId'] != PROJECT or
                    workspace['runtimeConfig'] != config or workspace['cwd'] != command['cwd'] or
                    workspace['isPrimary'] is not False):
                raise ValueError()
            service, = workspace['runtimeServices']
            _uuid(service['id'])
            if (service['companyId'] != COMPANY or service['projectId'] != PROJECT or
                    service['projectWorkspaceId'] != workspace_id or
                    service['provider'] != 'local_process' or service['status'] != 'stopped' or
                    service['serviceName'] != command['name'] or
                    service['command'] != command['command'] or service['cwd'] != command['cwd'] or
                    service['port'] != command['port'] or service['url'] is not None or
                    service.get('exposure') is not None or
                    _time(service['stoppedAt']) < _time(service['startedAt'])):
                raise ValueError()
            observed.append(service)
        if observed[0] != observed[1]:
            raise ValueError()
        return {'stop_operation_id': operation_id, 'service_id': observed[0]['id'],
                'control_plane_stop_verified': True, 'process_exit_verified': False,
                'port_release_verified': False, 'writer_release_verified': False}
    except (ValueError, KeyError, TypeError, AttributeError, OverflowError):
        raise ValueError(ERROR) from None
