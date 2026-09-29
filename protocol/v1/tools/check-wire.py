"""Minimal reference shape checker; no product/resource/crypto/state validation."""
from pathlib import Path
import json
r=Path(__file__).resolve().parents[1]
schema=json.loads((r/'schema.json').read_text())
def check(raw,name):
 pos=0;last=0;seen=set();fields={x['tag']:x for x in schema[name]}
 def vi():
  nonlocal pos
  n=0;start=pos
  for i in range(10):
   assert pos<len(raw)
   b=raw[pos];pos+=1;n|=(b&127)<<(7*i)
   if b<128:
    assert n<2**64 and (pos-start==1 or b>0)
    return n
  raise AssertionError('varint too long')
 while pos<len(raw):
  key=vi();tag,wire=key>>3,key&7
  assert tag in fields and tag>=last
  f=fields[tag];assert tag not in seen or f['repeated'];seen.add(tag);last=tag
  t=f['type']
  if t in ('u32','u64'):
   assert wire==0;n=vi();assert n<2**int(t[1:])
  else:
   assert wire==2;n=vi();assert n<=len(raw)-pos
   body=raw[pos:pos+n];pos+=n
   if t in schema:check(body,t)
   elif t in ('h','a','pk','sig','atoms'):assert len(body)=={'h':32,'a':20,'pk':1952,'sig':3309,'atoms':16}[t]
   else:body.decode('ascii')
 assert all(f['repeated'] or f['tag'] in seen for f in fields.values())
cases=json.loads((r/'vectors/wire-cases.json').read_text())['cases']
for c in cases:
 try:check(bytes.fromhex(c['wire_hex']),c['message']);result='CANONICAL'
 except (AssertionError,UnicodeError):result='NON_CANONICAL_WIRE'
 assert result==c['expected'],c['id']
print(f'PASS {len(cases)} reference wire shape examples; no product parser claim')
