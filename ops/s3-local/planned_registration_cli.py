#!/usr/bin/env python3
"""Pure registration plan; stdin JSON only, no API, key or file effects."""
import sys
from manifest import decode,encode
from planned_registration import plan
if __name__=='__main__':
    try:
        if len(sys.argv)!=2 or sys.argv[1]!='plan-before-keys':raise ValueError()
        raw=sys.stdin.buffer.read(65537)
        if len(raw)>65536:raise ValueError()
        sys.stdout.buffer.write(encode(plan(decode(raw))))
    except Exception:
        print('LOCAL_REGISTRATION_PLAN_REJECTED',file=sys.stderr);sys.exit(2)
