#!/usr/bin/env python3
"""Validate real snapshot against the approved schema subset and hash oracle."""
import hashlib,json,pathlib,runpy,sys
paths=sys.argv[1:]
sys.argv=[sys.argv[0]]
root=pathlib.Path(__file__).resolve().parents[3]
sys.path.insert(0,str(root/'protocol/s2/tools'))
oracle=runpy.run_path(str(root/'protocol/s2/tools/check.py'))
manifest=json.loads((root/'protocol/s2/manifest.json').read_text())
for path in paths:
 v=json.loads(pathlib.Path(path).read_text())
 oracle['valid']({'$ref':'#/$defs/ChainSnapshot'},v)
 assert oracle['hjson']('NUS/S2/SNAPSHOT/V1',v['body'])==v['snapshot_id']
 context=v['body']['context']
 assert context['contract_hash']==manifest['contract_sha256']
 assert context['config_hash']==manifest['config_sha256']
 assert context['chain_id']=='nus-s2-dev-1'
 print('PASS real ChainSnapshot schema/hash/pinned contract:',path)
