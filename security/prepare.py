"""Materialize immutable review inputs into an isolated checkout; never overwrite them."""
import io,json,pathlib,subprocess,tarfile
root=pathlib.Path(__file__).resolve().parents[1]
for row in json.loads((root/'security/evidence/inputs.json').read_text()):
 dest=root/row['component']
 if dest.exists():raise SystemExit(f'Refusing existing input directory: {dest}; use a fresh isolated checkout')
 raw=subprocess.check_output(['git','archive',row['sha'],row['component']],cwd=root)
 with tarfile.open(fileobj=io.BytesIO(raw)) as t:t.extractall(root,filter='data')
 if row['issue']=='NUS-14':
  for rel in ['schema.json','vectors/message-codec.json','vectors/s0-cases.json','vectors/batches.json']:
   old=subprocess.check_output(['git','show',row['sha']+':protocol/v1/'+rel],cwd=root)
   assert old==(root/'protocol/v1'/rel).read_bytes(),('E compatibility',rel)
  print('NUS-14 rc2 provenance retained; all four consumed protocol files identical to rc3')
  continue
 raw=subprocess.check_output(['git','archive',row['sha'],'protocol/v1'],cwd=root)
 with tarfile.open(fileobj=io.BytesIO(raw)) as t:
  for m in t:
   if m.isfile():assert t.extractfile(m).read()==(root/m.name).read_bytes(),(row['issue'],m.name)
 print(row['issue'],row['sha'],'protocol baseline identical')
