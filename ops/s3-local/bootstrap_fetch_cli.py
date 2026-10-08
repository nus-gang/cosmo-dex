#!/usr/bin/env python3
"""L-T one-shot authenticated fetch; evidence only, never creates a home."""
import hashlib
import ipaddress
import json
import os
from pathlib import Path
import stat
import sys
import bootstrap_cli
from bootstrap_fetch import fetch
from fetch_process import FetchFailure, MAX_RPC
from launcher_signal import stop_latch
from offline_cli import Parser
from preflight import checked_root


def parse(argv):
    options = [v for v in argv if v.startswith('--')]
    if len(options) != len(set(options)) or any('=' in v for v in options):
        raise ValueError('ARGUMENTS')
    parser = Parser(allow_abbrev=False, add_help=False)
    parser.add_argument('--chain-rpc', required=True)
    parser.add_argument('--evidence-root', type=Path, required=True)
    extra, remaining = parser.parse_known_args(argv)
    args, decision, revisions = bootstrap_cli.parse(remaining)
    host, port = extra.chain_rpc.rsplit(':', 1)
    ip = ipaddress.ip_address(host[1:-1] if host.startswith('[') and host.endswith(']') else host)
    number = int(port)
    canonical = f'[{ip}]:{number}' if ip.version == 6 else f'{ip}:{number}'
    if not ip.is_loopback or not 1024 <= number <= 65535 or canonical != extra.chain_rpc:
        raise ValueError('ADDRESS')
    return args, decision, revisions, extra


def execute_raw(args, decision, revisions, extra, stopped):
    """Preserve first, then return exact received bytes to an internal caller."""
    root = checked_root(extra.evidence_root)
    directory = os.open(root, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW)
    try:
        original = os.fstat(directory)
        if stat.S_IMODE(original.st_mode) != 0o700 or original.st_uid != os.getuid():
            raise ValueError('EVIDENCE_ROOT')
        # Reserve before IO. Every outcome retains this file; never retry it.
        fd = os.open('snapshot-fetch.raw', os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW,
                     0o600, dir_fd=directory)
        with os.fdopen(fd, 'wb') as evidence:
            os.fchmod(evidence.fileno(), 0o600)
            evidence.flush()
            os.fsync(evidence.fileno())
            os.fsync(directory)
            file_identity = os.fstat(evidence.fileno())
            def root_matches():
                current = os.stat(root, follow_symlinks=False)
                if (current.st_dev, current.st_ino, current.st_mode) != (original.st_dev, original.st_ino, original.st_mode):
                    raise ValueError('EVIDENCE_ROOT_CHANGED')
                named = os.stat('snapshot-fetch.raw', dir_fd=directory, follow_symlinks=False)
                if (named.st_dev, named.st_ino, named.st_nlink, stat.S_IMODE(named.st_mode)) != (file_identity.st_dev, file_identity.st_ino, 1, 0o600):
                    raise ValueError('EVIDENCE_FILE_CHANGED')
            root_matches()
            failure = None
            try:
                raw = fetch(args.bundle, args.artifacts, args.runtime_pin,
                    args.local_demo_profile, args.acknowledge_unproven_space,
                    decision, revisions, args.input_set.parent, args.input_set.name,
                    args.effective_profile, extra.chain_rpc, args.scratch, stopped=stopped)
            except FetchFailure as error:
                raw, failure = error.partial_raw, error
            if not isinstance(raw, bytes) or len(raw) > MAX_RPC or (not raw and failure is None):
                raise ValueError('FETCH_REPORT')
            evidence.write(raw)
            evidence.flush()
            os.fsync(evidence.fileno())
            os.fsync(directory)
            root_matches()
            if failure is not None:
                raise failure
            if stopped():
                raise ValueError('STOPPED')
            return {'transport_completed': True, 'raw_sha256': hashlib.sha256(raw).hexdigest(),
                    'raw_bytes': len(raw), 'snapshot_verified': False,
                    'home_created': False, 'reusable_permit': False}, raw
    finally:
        os.close(directory)


def execute(args, decision, revisions, extra, stopped):
    report, _ = execute_raw(args, decision, revisions, extra, stopped)
    return report


def main(argv=None):
    try:
        args = parse(list(sys.argv[1:] if argv is None else argv))
        with stop_latch() as stopped:
            result = execute(*args, stopped)
        print(json.dumps(result, sort_keys=True))
        return 0
    except (ValueError, OSError, KeyError, TypeError, KeyboardInterrupt):
        print('LOCAL_BOOTSTRAP_FETCH_REJECTED', file=sys.stderr)
        return 2


if __name__ == '__main__':
    sys.exit(main())
