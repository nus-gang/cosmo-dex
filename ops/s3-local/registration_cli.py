#!/usr/bin/env python3
"""Pure exact worker registration packet; no API calls or mailbox creation."""
import json
from pathlib import Path
import sys

from workspace_registration import prepare

ERROR = 'LOCAL_REGISTRATION_PACKET_REJECTED'


def packet_main(argv, *, command, prepare_packet):
    try:
        args = list(sys.argv[1:] if argv is None else argv)
        # Fixed envelope keeps launcher-only options out of worker argv.
        if (len(args) < 9 or args[0] != command or
                args[1] != '--python' or args[3] != '--candidate' or
                args[5] != '--fee-bps' or args[6] not in ('0', '25') or
                args[7] != '--'):
            raise ValueError(ERROR)
        python, candidate = args[2], args[4]
        if any(not Path(p).is_absolute() or '..' in Path(p).parts
               for p in (python, candidate)):
            raise ValueError(ERROR)
        packet = prepare_packet(python, candidate, args[8:], fee_bps=int(args[6]))
        # Serialize before writing: every input rejection leaves stdout empty.
        output = json.dumps(packet, ensure_ascii=True, sort_keys=True,
                            separators=(',', ':')) + '\n'
        sys.stdout.write(output)
        return 0
    except (ValueError, TypeError, KeyError, OSError, KeyboardInterrupt):
        print(ERROR, file=sys.stderr)
        return 2


def main(argv=None):
    return packet_main(argv, command='packet', prepare_packet=prepare)


if __name__ == '__main__':
    sys.exit(main())
