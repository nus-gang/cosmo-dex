"""Read-only approved Python oracle -> independent Rust differential inputs.
No protocol file is generated or resealed. Storage shapes are not trading traces.
"""
import base64, json, pathlib, sys
root=pathlib.Path(__file__).resolve().parents[3]
sys.path.insert(0,str(root/'protocol/s3/tools'))
from codec import read, canon
from capacity import certificate
from check_evidence_capacity import history_state, large_step, sized_json
from evidence import OBJECTS, verify_graph
from check_unicode_capacity import maximum
f=read('vectors/correction-state-hash.json')
objects={p.name:p.read_bytes() for p in OBJECTS.iterdir() if p.is_file()}
cases=[]
for count,orders in [(1000,200),(1001,201)]:
 for bps in (0,25):
  state=history_state(f,count,orders,bps)
  cases.append(dict(name=f'history-{count}-{orders}-{bps}',state=state,certificate=certificate(state)))
state=history_state(f,1001,201,0)
for fill in state['fills']: fill['state']='PENDING'
cases.append(dict(name='all-pending-reject',state=state,certificate=certificate(state)))
for label,text in [('bmp','한'),('emoji','😀'),('control','\x00'),('mixed','A한😀\x00"\\\n')]:
 defs=read('schema.json')['$defs']
 state=maximum(defs['EngineState'],defs,text,'EngineState',overrides={'EngineState.latest_observation_ref':None})
 cases.append(dict(name='max-'+label,state=state,certificate=certificate(state)))
step,large_objects=large_step(f,sized_json(3145728))
objects.update(large_objects)
refs={}
for state in [c['state'] for c in cases]+[step['after_state'],step['result']]:
 for ref in verify_graph(state,objects): refs[ref['sha256']]=ref
json.dump(dict(cases=cases,large_step=step,objects=[dict(ref=r,raw_b64=base64.b64encode(objects[h]).decode()) for h,r in sorted(refs.items())]),sys.stdout,ensure_ascii=True,separators=(',',':'))
