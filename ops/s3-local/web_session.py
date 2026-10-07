"""L-T web session seam: exact registration, targeted stop, PID/port observations.

No registration or live CLI. Trusted client/broker/audit inputs are required.
Mailbox evidence is retained on every exit; web does not own the worker writer.
"""
from contextlib import contextmanager
import copy
from pathlib import Path

import port_release
import process_release
from runtime_evidence import operation, stopped
from web_cli import parse_web
from web_pid_mailbox import WebMailbox
from workspace_registration import PROJECT, prepare_web, registered

ERROR = 'WEB_SESSION_REJECTED'
STOP_ERROR = 'WEB_SESSION_STOP_UNCONFIRMED'
RELEASE_ERROR = 'WEB_SESSION_RELEASE_UNCONFIRMED'


@contextmanager
def session(python, candidate, argv, **kwargs):
    with _session(python, candidate, argv, process=process_release.check,
                  port=port_release.check, **kwargs) as evidence:
        yield evidence


@contextmanager
def _session(python, candidate, argv, *, fee_bps, mailbox, broker_root,
             broker, client, audit, process, port):
    try:
        argv = tuple(argv)
        args, _, endpoint, _, _, _, _, _ = parse_web(['serve-web-reviewed', *argv])
        packet = prepare_web(python, candidate, argv, fee_bps=fee_bps)
        config = packet['body']['runtimeConfig']
        command, = config['workspaceRuntime']['commands']
        endpoints = (('127.0.0.1', command['port']),)
        root = Path(broker_root)
        if (type(mailbox) is not WebMailbox or mailbox.used or
                args.pid_mailbox != mailbox.root or not root.is_absolute() or
                '..' in root.parts or endpoint != root / 's'):
            raise ValueError()
        evidence = dict(start_attempted=False, stop_attempted=False,
            control_plane_stop_verified=False, host_release_observations={},
            host_release_observations_complete=False, pid_handoff_collected=False,
            pid_evidence_retained=True, inventory_complete_verified=False,
            cleanup_complete_verified=False, approval_verified=False, DEV='NOT_RUN')
        with broker(root) as actual:
            if Path(actual) != endpoint:
                raise ValueError()
            path = f'/api/projects/{PROJECT}/workspaces'
            target = registered(packet, client.read_workspaces(path))
            if target['workspace_id'] != client._workspace_id:
                raise ValueError()
            audit()
            if registered(packet, client.read_workspaces(path)) != target:
                raise ValueError()
            yielded = False
            try:
                evidence['start_attempted'] = True
                response = client.request(copy.deepcopy(target['requests']['start']))
                operation(response, 'start', client._workspace_id, command)
                yielded = True
                yield evidence
            finally:
                evidence['stop_attempted'] = True
                try:
                    response = client.request(copy.deepcopy(target['requests']['stop']))
                    evidence.update(stopped(response, client.read_workspaces(path),
                                            client._workspace_id, config))
                except BaseException:
                    raise ValueError(STOP_ERROR) from None
                if yielded:
                    try:
                        pids = mailbox.collect()
                        evidence['pid_handoff_collected'] = True
                        evidence['listed_pids'] = list(pids)
                        for name, probe, inputs, schema, success, field in (
                            ('process', process, pids, 's3-local-process-probe/1',
                             'listed_pids_absent', 'pids'),
                            ('port', port, endpoints, 's3-local-port-probe/1',
                             'simultaneous_bind_verified', 'endpoints')):
                            report = probe(inputs)
                            if (type(report) is not dict or report.get('schema') != schema or
                                    report.get(success) is not True or
                                    report.get(field) != list(inputs)):
                                raise ValueError()
                            evidence['host_release_observations'][name] = copy.deepcopy(report)
                        evidence['host_release_observations_complete'] = True
                    except (KeyboardInterrupt, SystemExit):
                        raise
                    except Exception:
                        raise ValueError(RELEASE_ERROR) from None
    except (KeyboardInterrupt, SystemExit):
        raise
    except Exception as exc:
        if isinstance(exc, ValueError) and str(exc) in (STOP_ERROR, RELEASE_ERROR):
            raise
        raise ValueError(ERROR) from None
