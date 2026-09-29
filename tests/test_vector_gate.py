"""Reject plausible unanimous bad receipts independently of cross-lane equality."""
import copy, importlib.util, unittest
from pathlib import Path
spec=importlib.util.spec_from_file_location('gate',Path(__file__).resolve().parents[1]/'ops/ci/vectors.py')
gate=importlib.util.module_from_spec(spec);spec.loader.exec_module(gate)
class Gate(unittest.TestCase):
 def setUp(self):
  self.expected={'positive':True,'negative':False,'bytes':'0011','error':'INTEGER_RANGE'}
  self.good={'contract_revision':'rc3','vectors_sha256':'abc','results':[{'id':k,'actual':v} for k,v in self.expected.items()]}
 def check(self,r):gate.validate_receipt(r,self.expected,'rc3','abc')
 def test_correct(self):self.check(self.good)
 def test_same_wrong_answer(self):
  for _ in range(3):
   r=copy.deepcopy(self.good);r['results'][1]['actual']=True
   with self.assertRaisesRegex(ValueError,'oracle mismatch'):self.check(r)
 def test_same_subset(self):
  for _ in range(3):
   r=copy.deepcopy(self.good);r['results']=r['results'][:1]
   with self.assertRaisesRegex(ValueError,'missing/extra'):self.check(r)
 def test_duplicate(self):
  r=copy.deepcopy(self.good);r['results'].append(r['results'][0])
  with self.assertRaisesRegex(ValueError,'duplicate'):self.check(r)
 def test_missing(self):
  r=copy.deepcopy(self.good);r['results'].pop()
  with self.assertRaisesRegex(ValueError,'missing/extra'):self.check(r)
 def test_extra(self):
  r=copy.deepcopy(self.good);r['results'].append({'id':'extra','actual':True})
  with self.assertRaisesRegex(ValueError,'missing/extra'):self.check(r)
 def test_wrong_bytes_or_error(self):
  for i in (2,3):
   r=copy.deepcopy(self.good);r['results'][i]['actual']='wrong'
   with self.assertRaisesRegex(ValueError,'oracle mismatch'):self.check(r)
 def test_hash_revision(self):
  for key in ('contract_revision','vectors_sha256'):
   r=copy.deepcopy(self.good);r[key]='bad'
   with self.assertRaisesRegex(ValueError,'revision/hash'):self.check(r)
 def test_bool_is_not_integer(self):
  r=copy.deepcopy(self.good);r['results'][0]['actual']=1
  with self.assertRaisesRegex(ValueError,'oracle mismatch'):self.check(r)
if __name__=='__main__':unittest.main()
