#!/usr/bin/env python3
"""Render reviewed registration packets; never registers or starts a service."""
import base64
import hashlib
from pathlib import Path
import sys
import approval_gate
from manifest import decode,encode
from preflight import bounded,checked_root,verify_input_set
from registration_render import render,key_packets,ERROR
from registration_set_cli import _read


def prepare(spec,keys_only=False):
    def audit():
        return approval_gate.inspect(Path(spec['bundle']),Path(spec['artifacts']),spec['runtime_pin'],
            's3-dev-local/1',True,spec['native_decision_id'],
            {'ceo':spec['ceo_revision'],'cto':spec['cto_revision']})
    before=audit(); reports={}
    if keys_only:
        result={'schema':'s3-local-key-preparation-packets/1','packets':key_packets(spec),
                'starts_service':False,'approval_verified':False}
        if audit()!=before:raise ValueError(ERROR)
        return result
    for name in ('fee0','fee25'):
        root=checked_root(spec['profiles'][name])
        raw,_=verify_input_set(Path(spec['bundle']),Path(spec['artifacts']),spec['runtime_pin'],
                               's3-dev-local/1',True,root,'input.json')
        value=decode(raw);report=decode(bounded(root,'initialization.json',16384))
        for key in ('genesis','guard'):
            content=base64.b64decode(value[key],validate=True)
            if hashlib.sha256(content).hexdigest()!=report[key+'_sha256']:raise ValueError(ERROR)
        reports[name]=report
    result=render(spec,reports)
    if audit()!=before:raise ValueError(ERROR)
    return result


def main(argv=None):
    try:
        args=list(sys.argv[1:] if argv is None else argv)
        if len(args)!=3 or args[0] not in ('packets-reviewed','key-packets-reviewed') or args[1]!='--input':raise ValueError(ERROR)
        result=prepare(_read(args[2]),args[0]=='key-packets-reviewed')
        sys.stdout.buffer.write(encode(result));return 0
    except (ValueError,OSError,TypeError,KeyError,KeyboardInterrupt):
        print(ERROR,file=sys.stderr);return 2


if __name__=='__main__':sys.exit(main())
