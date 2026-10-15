"""후보 파일·상속 계약의 정적 대조. runtime/보안/FS 시험이 아니다."""
import hashlib, json
from pathlib import Path
HERE=Path(__file__).resolve().parent
ROOT=HERE.parents[1]
def sha(path): return hashlib.sha256(path.read_bytes()).hexdigest()
def main():
    manifest=json.loads((HERE/'MANIFEST.json').read_text())
    for name,want in manifest['files_sha256'].items():
        assert sha(HERE/name)==want, ('CANDIDATE_HASH',name)
    baseline=json.loads((ROOT/'protocol/s3/manifest.json').read_text())
    assert sha(ROOT/'protocol/s3/manifest.json')==manifest['active_manifest_sha256']
    for name,want in baseline['files_sha256'].items():
        assert sha(ROOT/name)==want, ('BASELINE_HASH',name)
    allowed={'profile','durability','runtime_root'}
    for fee,base in [('fee0','profile.json'),('fee25','profile-fee25.json')]:
        before=json.loads((ROOT/'protocol/s3'/base).read_text())
        after=json.loads((HERE/('effective-profile-'+fee+'.json')).read_text())
        assert set(before)==set(after)
        assert {k for k in before if before[k]!=after[k]}==allowed
        assert after['durability']=='LOCAL_FSYNC_UNPROVEN_SPACE'
        assert after['runtime_root']=='.runtime/s3-dev-local-v1/'
    c=json.loads((HERE/'candidate.json').read_text())
    assert not c['activated'] and not c['default_enabled'] and not c['runtime_implemented']
    assert not c['durable_ack'] and c['standard_ACK']=='CLOSED'
    assert c['supported_backend_allowlist']==[] and c['G00']=='FAIL_UNPROVEN'
    assert c['storage_magic']!=c['standard_storage_magic']
    a=json.loads((HERE/'acceptance.json').read_text())
    assert len(a['cases'])==14 and all(x['result']=='NOT_RUN' for x in a['cases'])
    assert a['runtime_genesis_hash'] is None
    print(json.dumps({'result':'PASS','scope':'STATIC_CONTRACT_ONLY','base_hashes':len(baseline['files_sha256']),'candidate_hashes':len(manifest['files_sha256']),'effective_profiles':2,'changed_keys_each':sorted(allowed),'runtime_acceptance':'NOT_RUN','G00':'FAIL_UNPROVEN','ACK':'CLOSED'},ensure_ascii=False,indent=2))
if __name__=='__main__':main()
