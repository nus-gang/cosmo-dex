"""Materialize immutable review inputs into an isolated checkout; never overwrite them."""
import io,json,pathlib,subprocess,tarfile
root=pathlib.Path(__file__).resolve().parents[1]
for row in json.loads((root/'security/evidence/inputs.json').read_text()):
 dest=root/row['component']
 if dest.exists():raise SystemExit(f'Refusing existing input directory: {dest}; use a fresh isolated checkout')
 raw=subprocess.check_output(['git','archive',row['sha'],row['component']],cwd=root)
 with tarfile.open(fileobj=io.BytesIO(raw)) as t:t.extractall(root,filter='data')
 raw=subprocess.check_output(['git','archive',row['sha'],'protocol/v1'],cwd=root)
 with tarfile.open(fileobj=io.BytesIO(raw)) as t:
  for m in t:
   if m.isfile():assert t.extractfile(m).read()==(root/m.name).read_bytes(),(row['issue'],m.name)
 print(row['issue'],row['sha'],'protocol baseline identical')
