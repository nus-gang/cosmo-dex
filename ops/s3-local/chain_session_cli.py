#!/usr/bin/env python3
"""L-T only: bounded authenticated session for an already registered packet."""
import json
from pathlib import Path
import sys
import time

from authenticated_chain_session import session
from launcher_signal import stop_latch
from native_review import _uuid
from pid_mailbox import Mailbox
from chain_cli import parse as parse_chain
from workspace_registration import prepare_chain

ERROR = 'LOCAL_AUTHENTICATED_CHAIN_SESSION_REJECTED'


def parse(argv):
    if (len(argv) < 15 or argv[0] != 'run-chain-reviewed' or
            argv[1] != '--python' or argv[3] != '--candidate' or
            argv[5] != '--fee-bps' or argv[6] not in ('0', '25') or
            argv[7] != '--validator-index' or argv[8] not in ('0', '1', '2', '3') or
            argv[9] != '--workspace-id' or argv[11] != '--duration-seconds' or
            argv[13] != '--'):
        raise ValueError(ERROR)
    python, candidate = argv[2], argv[4]
    if any(not Path(p).is_absolute() or '..' in Path(p).parts for p in (python, candidate)):
        raise ValueError(ERROR)
    workspace = _uuid(argv[10])
    duration = argv[12]
    if (not duration.isascii() or not duration.isdecimal() or
            str(int(duration)) != duration or not 1 <= int(duration) <= 240):
        raise ValueError(ERROR)
    worker = argv[14:]
    # Pure validation happens before mailbox creation or authentication IO.
    prepare_chain(python, candidate, worker, fee_bps=int(argv[6]), validator_index=int(argv[8]))
    return python, candidate, int(argv[6]), int(argv[8]), workspace, int(duration), worker


def main(argv=None):
    try:
        python, candidate, fee, validator, workspace, duration, worker = parse(
            list(sys.argv[1:] if argv is None else argv))
        with stop_latch() as stop:
            if stop():
                raise ValueError(ERROR)
            args = parse_chain(['serve-chain-reviewed', *worker])
            mailbox = Mailbox(args.pid_mailbox)
            with session(python, candidate, worker, workspace,
                         fee_bps=fee, validator_index=validator, mailbox=mailbox, stop=stop) as evidence:
                previous = time.monotonic()
                deadline = previous + duration
                while not stop():
                    now = time.monotonic()
                    if now < previous:
                        raise ValueError(ERROR)
                    if now >= deadline:
                        break
                    previous = now
                    time.sleep(min(0.1, deadline-now))
            # Do not archive automatically: the explicit evidence lifecycle
            # remains available to the operator after inspection.
            fields = ('control_plane_stop_verified', 'host_release_observations_complete',
                      'pid_handoff_collected')
            if any(evidence.get(key) is not True for key in fields):
                raise ValueError(ERROR)
            report = {key: True for key in fields}
            report.update(schema='s3-local-chain-session-cli/1',
                          cleanup_complete_verified=False, inventory_complete_verified=False,
                          writer_release_verified=False, DEV='NOT_RUN')
            print(json.dumps(report, sort_keys=True))
        return 0
    except (Exception, KeyboardInterrupt):
        print(ERROR, file=sys.stderr)
        return 2


if __name__ == '__main__':
    sys.exit(main())
