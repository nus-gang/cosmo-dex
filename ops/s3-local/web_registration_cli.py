#!/usr/bin/env python3
"""Pure web registration packet, including explicit response-loss selection."""
import sys
from registration_cli import packet_main
from workspace_registration import prepare_web


def main(argv=None):
    return packet_main(argv, command='web-packet', prepare_packet=prepare_web)


if __name__ == '__main__':
    sys.exit(main())
