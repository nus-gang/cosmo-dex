"""Bounded orchestration core for L-T. No HTTP client or live entry point yet.

Injected operations are trusted, bounded adapters, never configuration values.
The worker still performs its independent capture/READY/pre-START audits.
"""
from contextlib import contextmanager
from pathlib import Path
import copy

from offline_cli import parse
from workspace_registration import PROJECT, prepare, registered

ERROR = 'MANAGED_SESSION_REJECTED'
CLEANUP_ERROR = 'MANAGED_SESSION_STOP_UNCONFIRMED'


@contextmanager
def _session(python, candidate, argv, *, fee_bps, broker_root,
             broker, read_workspaces, request, audit):
    """Internal seam: one targeted start, unconditional targeted stop once sent.

    A stop response is retained as evidence, not asserted to prove process exit.
    Start/stop are never retried after ambiguous failures. Caller must reconcile
    stop evidence with managed runtime state before reporting cleanup complete.
    """
    attempted = False
    target = None
    try:
        argv = tuple(argv)
        packet = prepare(python, candidate, argv, fee_bps=fee_bps)
        args, _ = parse(list(argv), reviewed=True, managed=True)
        root = Path(broker_root)
        if (not root.is_absolute() or '..' in root.parts or
                args.approval_socket != root / 's'):
            raise ValueError()
        # Scope the broker around both control requests, including error cleanup.
        with broker(root) as endpoint:
            if Path(endpoint) != args.approval_socket:
                raise ValueError()
            target = registered(packet, read_workspaces(
                f'/api/projects/{PROJECT}/workspaces'))
            audit()  # Must raise on denial; no serialized permit is accepted.
            # Re-read after audit: no command/config drift is accepted at dispatch.
            current = registered(packet, read_workspaces(
                f'/api/projects/{PROJECT}/workspaces'))
            if current != target:
                raise ValueError()
            evidence = {'start_attempted': False, 'stop_attempted': False,
                        'stop_acknowledged': False, 'process_exit_verified': False,
                        'approval_verified': False, 'DEV': 'NOT_RUN'}
            try:
                attempted = True  # Set before IO: lost response can still mean start.
                evidence['start_attempted'] = True
                evidence['start_response'] = request(copy.deepcopy(target['requests']['start']))
                yield evidence
            finally:
                if attempted:
                    evidence['stop_attempted'] = True
                    try:
                        evidence['stop_response'] = request(copy.deepcopy(target['requests']['stop']))
                        evidence['stop_acknowledged'] = True
                    except BaseException:
                        raise ValueError(CLEANUP_ERROR) from None
    except BaseException as exc:
        if isinstance(exc, ValueError) and str(exc) == CLEANUP_ERROR:
            raise
        if isinstance(exc, (KeyboardInterrupt, SystemExit)):
            raise
        raise ValueError(ERROR) from None


@contextmanager
def _observed_session(python, candidate, argv, *, client, **kwargs):
    """Connect scoped RuntimeClient with validated operations and one stop GET.

    Reports only control-plane evidence; L-T must additionally prove process,
    listener and writer release. Any failed/ambiguous check refuses completion.
    """
    from runtime_evidence import operation, stopped
    packet = prepare(python, candidate, argv, fee_bps=kwargs['fee_bps'])
    config = packet['body']['runtimeConfig']
    command, = config['workspaceRuntime']['commands']
    stop_evidence = {}

    def request(target):
        response = client.request(target)
        action = target['path'].rsplit('/', 1)[1]
        operation(response, action, client._workspace_id, command)
        if action == 'stop':
            stop_evidence.update(stopped(response, client.read_workspaces(
                f'/api/projects/{PROJECT}/workspaces'), client._workspace_id, config))
        return response

    evidence = None
    try:
        with _session(python, candidate, argv, read_workspaces=client.read_workspaces,
                      request=request, **kwargs) as evidence:
            evidence['control_plane_stop_verified'] = False
            yield evidence
    finally:
        if evidence is not None:
            evidence.update(stop_evidence)


@contextmanager
def _release_session(python, candidate, argv, *, process_probe, port_probe,
                     writer_probe, **kwargs):
    """Trusted L-T adapter seam; probes run once after a confirmed stop.

    Callers bind probes to the captured PID/endpoint/input inventory. This layer
    does not certify inventory completeness. Preserve partial observations on
    failure and never promote them to process-tree or overall cleanup proof.
    No probes run if startup never yielded an observed session.
    """
    evidence = None
    try:
        with _observed_session(python, candidate, argv, **kwargs) as evidence:
            evidence['host_release_observations'] = {}
            evidence['host_release_observations_complete'] = False
            evidence['cleanup_complete_verified'] = False
            yield evidence
    finally:
        if evidence is not None and evidence.get('control_plane_stop_verified') is True:
            probes = (
                ('process', process_probe, 's3-local-process-probe/1', 'listed_pids_absent'),
                ('port', port_probe, 's3-local-port-probe/1', 'simultaneous_bind_verified'),
                ('writer', writer_probe, 's3-local-writer-probe/1', 'writer_reopen_verified'),
            )
            try:
                for name, probe, schema, success in probes:
                    report = probe()
                    if (not isinstance(report, dict) or report.get('schema') != schema or
                            report.get(success) is not True):
                        raise ValueError()
                    evidence['host_release_observations'][name] = copy.deepcopy(report)
                evidence['host_release_observations_complete'] = True
            except (KeyboardInterrupt, SystemExit):
                raise
            except Exception:
                raise ValueError('MANAGED_SESSION_HOST_RELEASE_UNCONFIRMED') from None
