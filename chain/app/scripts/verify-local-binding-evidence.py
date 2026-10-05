"""SDK component evidence cross-check. No runtime/DEV acceptance is inferred."""
import base64
import hashlib
import json
import sys
from pathlib import Path


def sha(raw):
    return hashlib.sha256(raw).hexdigest()


def decode(raw):
    return base64.b64decode(raw, validate=True)


def aggregate(files):
    return sha(''.join(f'{v}  {k}\n' for k, v in sorted(files.items())).encode())


def verify(root):
    checks = 0

    def expect(got, want, label):
        nonlocal checks
        if got != want:
            raise AssertionError((label, got, want))
        checks += 1

    for fee in (0, 25):
        p = root / f'TestLocalDemoInitQueryRestart__fee{fee}'
        inputs = json.loads((p / 'component-inputs.json').read_bytes())
        manifest_raw = decode(inputs['runtime_manifest'])
        manifest = json.loads(manifest_raw)
        files = {k: decode(v) for k, v in inputs['files'].items()}
        expect({k: sha(v) for k, v in files.items()}, manifest['files_sha256'], 'source bytes')
        expect(sha(manifest_raw), inputs['approved_runtime_sha256'], 'fixture pin')
        expect(aggregate(manifest['files_sha256']), manifest['contract_sha256'], 'aggregate')
        expect(manifest['scope'], 'COMPONENT_FIXTURE', 'scope')
        guard = json.loads(decode(inputs['guard']))
        context = guard['context']
        expect(context['service_schema'], 's3/3', 'schema')
        expect(context['genesis_hash'], sha(decode(inputs['genesis'])), 'genesis bytes')
        expect(context['config_hash'], sha(decode(inputs['effective_profile'])), 'profile bytes')
        expect(context['contract_hash'], manifest['contract_sha256'], 'contract')
        expect(guard['runtime_manifest_sha256'], sha(manifest_raw), 'guard manifest')
        expect(guard['fee_profile'], f'fee{fee}', 'fee profile')
        genesis = json.loads(decode(inputs['genesis']))
        expect(genesis['app_state']['fee_bps'], str(fee), 'genesis fee')
        expect(genesis['app_state']['contract_hash'], context['contract_hash'], 'genesis contract')
        expect(genesis['app_state']['config_hash'], context['config_hash'], 'genesis config')
        r = json.loads((p / 'result.json').read_bytes())
        expect(r['DEV03'], 'NOT_RUN', 'acceptance boundary')
        expect(r['receipt']['context'], context, 'receipt context')
        expect(r['receipt']['disposition'], 'COMMITTED', 'receipt terminal')
        expect(r['receipt']['terminal_height'], '4', 'terminal height')
        blocks = [json.loads(x.read_bytes()) for x in sorted(p.glob('block-*.json'))]
        expect(len(blocks), 5, 'block count')
        expect(r['receipt']['terminal_tx_hash'], sha(decode(blocks[3]['txs'][0])), 'terminal TX')
        expect(blocks[3]['exchange_state'], blocks[4]['exchange_state'], 'retry asset effect zero')
        prefix = f's3/{context["genesis_hash"]}/DEVBASE/DEVQUOTE/'
        state = {k: bytes.fromhex(v) for k, v in blocks[-1]['exchange_state'].items()}
        for denom, gross, receive, remaining, fee_atoms in (
            ('DEVBASE', 10_000_000, 1_000_000, 9_000_000, 2_500 if fee else 0),
            ('DEVQUOTE', 100_000_000, 10_000_000, 90_000_000, 25_000 if fee else 0),
        ):
            positions = [int(v) for k, v in state.items() if k.startswith(prefix + 'a/' + denom + '/')]
            expect(sorted(positions), sorted([remaining, receive-fee_atoms]), denom+' exact positions')
            total = json.loads(state[prefix + 'total/' + denom])
            expect(int(total['Confirmed']), sum(positions), denom+' confirmed')
            expect(int(total['Treasury']), fee_atoms, denom+' fee')
            expect(int(total['Unassigned']), 0, denom+' unassigned')
            expect(sum(positions)+fee_atoms, gross, denom+' conservation')
        restart = json.loads((p / 'restart-queries.json').read_bytes())
        expect(restart['before']['value'], restart['after']['value'], 'same-height query bytes')
        expect(restart['before']['height'], restart['after']['height'], 'same query height')
        for side in ('before', 'after'):
            out = json.loads(decode(restart[side]['value']))
            expect(out['context'], context, side+' context')
            expect(out['receipt'], r['receipt'], side+' original receipt')

        p = root / f'TestLocalDemoQueryAndSignatureBinding__fee{fee}'
        baseline = json.loads((p / 'block-003.json').read_bytes())['exchange_state']
        vectors = sorted(root.glob(f'TestLocalDemoQueryAndSignatureBinding__fee{fee}__*/signed-vector.json'))
        expect(len(vectors), 5, 'signature/fee negatives')
        for vector in vectors:
            data = json.loads(vector.read_bytes())
            expect(data['result']['code'] > 0, True, vector.parent.name+' rejection')
            block = json.loads(next(vector.parent.glob('block-*.json')).read_bytes())
            expect(block['exchange_state'], baseline, vector.parent.name+' rollback')
        queries = sorted(root.glob(f'TestLocalDemoQueryAndSignatureBinding__fee{fee}__query_*/query-*.json'))
        expect(len(queries), 21, 'query negatives')
        for query in queries:
            data = json.loads(query.read_bytes())
            expect(data['output']['code'] > 0, True, query.parent.name+' rejection')
    return {'result': 'PASS', 'checks': checks, 'scope': 'SDK_COMPONENT_EVIDENCE_ONLY',
            'DEV01_14': 'NOT_RUN', 'G00': 'FAIL_UNPROVEN', 'ACK': 'CLOSED'}


if __name__ == '__main__':
    print(json.dumps(verify(Path(sys.argv[1])), indent=2))
