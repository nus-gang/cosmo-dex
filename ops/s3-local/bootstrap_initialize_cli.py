#!/usr/bin/env python3
"""L-T only: fetch once, preserve evidence, then separately gate C creation."""
import json
from pathlib import Path
import sys
import bootstrap_fetch_cli
from bootstrap_initialize import initialize
from launcher_signal import stop_latch
from offline_cli import Parser


def parse(argv):
    options = [v for v in argv if v.startswith('--')]
    if len(options) != len(set(options)) or any('=' in v for v in options):
        raise ValueError('ARGUMENTS')
    parser = Parser(allow_abbrev=False, add_help=False)
    parser.add_argument('--home', type=Path, required=True)
    home, remaining = parser.parse_known_args(argv)
    if not home.home.is_absolute() or '..' in home.home.parts:
        raise ValueError('HOME_PATH')
    return (*bootstrap_fetch_cli.parse(remaining), home.home)


def main(argv=None):
    try:
        arguments = parse(list(sys.argv[1:] if argv is None else argv))
        with stop_latch() as stopped:
            result = initialize(*arguments, stopped=stopped)
        print(json.dumps(result, sort_keys=True))
        return 0
    except (ValueError, OSError, KeyError, TypeError, KeyboardInterrupt):
        # START may have reached the child. Preserve evidence/home for diagnosis.
        print('LOCAL_INITIALIZE_REJECTED_PRESERVE_HOME_AND_EVIDENCE', file=sys.stderr)
        return 2


if __name__ == '__main__':
    sys.exit(main())
