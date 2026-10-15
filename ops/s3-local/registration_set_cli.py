#!/usr/bin/env python3
"""Emit one canonical registration set; never mutates Paperclip."""
import json
import os
from pathlib import Path
import stat
import sys

from registration_set import ERROR, compile_set


def _pairs(pairs):
    value = {}
    for key, item in pairs:
        if key in value:
            raise ValueError(ERROR)
        value[key] = item
    return value


def _read(path):
    path = Path(path)
    if not path.is_absolute() or '..' in path.parts or path.resolve(strict=True) != path:
        raise ValueError(ERROR)
    before = path.lstat()
    if (not stat.S_ISREG(before.st_mode) or before.st_nlink != 1 or
            not 0 < before.st_size <= 1 << 20):
        raise ValueError(ERROR)
    fd = os.open(path, os.O_RDONLY | os.O_NOFOLLOW)
    try:
        after = os.fstat(fd)
        if (before.st_dev, before.st_ino) != (after.st_dev, after.st_ino):
            raise ValueError(ERROR)
        raw = bytearray()
        while len(raw) <= 1 << 20:
            part = os.read(fd, min(65536, (1 << 20) + 1 - len(raw)))
            if not part:
                break
            raw.extend(part)
        if len(raw) > 1 << 20:
            raise ValueError(ERROR)
        return json.loads(raw, object_pairs_hook=_pairs)
    finally:
        os.close(fd)


def main(argv=None):
    try:
        args = list(sys.argv[1:] if argv is None else argv)
        if len(args) != 3 or args[0] != 'packets' or args[1] != '--input':
            raise ValueError(ERROR)
        result = compile_set(_read(args[2]))
        output = json.dumps(result, ensure_ascii=True, sort_keys=True,
                            separators=(',', ':')) + '\n'
        sys.stdout.write(output)
        return 0
    except (ValueError, OSError, json.JSONDecodeError, KeyboardInterrupt):
        print(ERROR, file=sys.stderr)
        return 2


if __name__ == '__main__':
    sys.exit(main())
