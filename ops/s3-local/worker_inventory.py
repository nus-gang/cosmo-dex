"""In-process worker inventory; no PID discovery or serialized trust claim."""
import copy
import shlex

from offline_cli import parse
from runtime_config import worker_config


def worker_endpoints(config, python, candidate, *, fee_bps):
    """Extract only worker-owned listeners from an exact generated command.

    RPC is a dependency owned by Chain, not a worker listener. Caller must
    independently bind config to the approved candidate and managed workspace.
    """
    try:
        config = copy.deepcopy(config)
        commands = config['runtimeConfig']['workspaceRuntime']['commands']
        if len(commands) != 1:
            raise ValueError()
        argv = shlex.split(commands[0]['command'])
        arguments = argv[5:]
        if config != worker_config(python, candidate, arguments, fee_bps=fee_bps):
            raise ValueError()
        parsed, _ = parse(arguments, reviewed=True, managed=True)
        host, port = parsed.bind.split(':')
        return ((host, int(port)),)
    except Exception:
        raise ValueError('WORKER_INVENTORY_REJECTED') from None


class SpawnInventory:
    """One child's Popen PID, captured before any input or START is sent.

    Supply record as the trusted on_spawn callback. This is not a complete
    launcher/descendant inventory, nor a process identity or cleanup proof.
    """
    def __init__(self):
        self._pid = None
        self._closed = False

    def record(self, pid):
        if self._closed or self._pid is not None:
            self._closed = True
            raise ValueError('WORKER_INVENTORY_REJECTED')
        if type(pid) is not int or not 2 <= pid <= 2147483647:
            self._closed = True
            raise ValueError('WORKER_INVENTORY_REJECTED')
        self._pid = pid

    def finish(self):
        if self._closed or self._pid is None:
            self._closed = True
            raise ValueError('WORKER_INVENTORY_REJECTED')
        self._closed = True
        return (self._pid,)
