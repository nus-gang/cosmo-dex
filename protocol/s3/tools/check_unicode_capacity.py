"""SEC-65-01 encoding regressions; schema shapes, not reachable engine traces.

The maximal-value construction adapts Security's independent unicode_repro.py
(NUS-65 attachment 2f842972-bb16-47d3-b1a8-6592a5f1d7bf). It serializes actual
values independently of the oracle's size arithmetic. No chain/allocator IO.
"""
import base64
import hashlib
import json
from codec import canon, read
from capacity import Bounds, certificate, admit, b64_size


def maximum(spec, defs, text, path='', counts=None, overrides=None):
    counts=counts or {};overrides=overrides or {}
    if path in overrides:return overrides[path]
    if '$ref' in spec:
        name=spec['$ref'].split('/')[-1]
        if name in ('U32','U64','Atoms'):
            return str(2**{'U32':32,'U64':64,'Atoms':128}[name]-1)
        if name=='Hash':return 'f'*64
        return maximum(defs[name],defs,text,name,counts,overrides)
    make=lambda s,p:maximum(s,defs,text,p,counts,overrides)
    if 'anyOf' in spec:return max((make(s,path) for s in spec['anyOf']),key=lambda v:len(canon(v)))
    if 'enum' in spec:return max(spec['enum'],key=lambda v:len(canon(v)))
    if 'const' in spec:return spec['const']
    kind=spec['type']
    if kind=='object':return {k:make(s,path+'.'+k) for k,s in spec['properties'].items()}
    if kind=='array':return [make(spec['items'],path+'[]') for _ in range(spec.get('maxItems',counts.get(path,0)))]
    if kind=='boolean':return False
    if kind=='null':return None
    if kind=='string':
        width=spec['maxLength']
        if 'pattern' in spec:return 'A'*width  # only narrowed Bytes reach here
        return (text*((width+len(text)-1)//len(text)))[:width]
    raise ValueError(path)


def measure():
    from check import validate
    defs=read('schema.json')['$defs'];bounds=Bounds(0,0)
    samples={
        'ascii':'A','quote':'"','backslash':'\\','nul':'\x00','control':'\x1f',
        'short_escape':'\n','bmp_start':'\u0080','bmp_korean':'\ud55c','bmp_end':'\uffff',
        'supplementary_start':'\U00010000','emoji':'\U0001f600','unicode_end':'\U0010ffff',
        'mixed':'A\ud55c\U0001f600\x00"\\\n',
    }
    text_cases=[]
    for label,sample in samples.items():
        for length in (0,1,255,256):
            value=(sample*((length+len(sample)-1)//len(sample)))[:length]
            validate(value,defs['Text'],defs)
            actual=len(canon(value))
            assert actual<=bounds.size('Text'),(label,length,actual)
            assert json.loads(canon(value))==value
            text_cases.append(dict(case=label,code_points=length,actual_bytes=actual))
        try:validate(sample[0]*257,defs['Text'],defs)
        except AssertionError:pass
        else:raise AssertionError('Text maxLength not enforced: '+label)
    assert canon('\U0001f600')==b'"\\ud83d\\ude00"'
    assert len(canon('\U0001f600'*256))==bounds.size('Text')==3074
    # A future pattern's existence (or a name ending in Hash) is not proof of ASCII.
    for pattern in ('^.*$','^[\U0001f600]+$','^\\S+$'):
        spec={'type':'string','maxLength':256,'pattern':pattern}
        validate('\U0001f600'*256,spec,defs)
        assert bounds.spec(spec,'FutureHash')>=3074
    try:bounds.spec({'type':'string','pattern':'.*'},'UnboundedText')
    except ValueError as exc:assert str(exc)=='UNBOUNDED_STRING UnboundedText'
    else:raise AssertionError('unbounded text accepted')

    # Security's original state has no retained external refs or unbounded
    # histories, but all schema-bounded arrays and mutable text at full width.
    reports=[]
    for label in ('bmp_korean','emoji','control','mixed'):
        state=maximum(defs['EngineState'],defs,samples[label],'EngineState',
                      overrides={'EngineState.latest_observation_ref':None})
        validate(state,defs['EngineState'],defs)
        cert=certificate(state)
        result=maximum(defs['CommandResult'],defs,samples[label],'CommandResult')
        validate(result,defs['CommandResult'],defs)
        state_raw,result_raw=canon(state),canon(result)
        journal=maximum(defs['JournalRecord'],defs,samples[label],'JournalRecord',overrides={
            'JournalRecord.state_json':base64.b64encode(state_raw).decode(),
            'JournalRecord.result_json':base64.b64encode(result_raw).decode()})
        validate(journal,defs['JournalRecord'],defs)
        journal_raw=canon(journal)
        assert len(state_raw)<=cert['max_state_bytes']
        assert len(result_raw)<=cert['max_result_bytes']
        assert len(journal_raw)<=cert['max_journal_payload_bytes']
        assert len(journal['state_json'])==b64_size(len(state_raw))<=b64_size(cert['max_state_bytes'])
        assert len(journal['result_json'])==b64_size(len(result_raw))<=b64_size(cert['max_result_bytes'])
        # Compare reservation B to independently serialized lengths, including
        # allocation rounding, both checkpoints and marker copies, not averages.
        def allocated(size):
            whole,partial=divmod(size,4096)
            return (whole+bool(partial))*4096+8192
        actual_record=allocated(72+len(journal_raw))+2*allocated(len(state_raw))+2*allocated(len(result_raw))+2*allocated(4096)
        actual_drain=cert['max_drain_records']*actual_record+cert['evidence_reserved_bytes']
        assert actual_drain<=cert['reserved_bytes']
        budget=cert['reserved_bytes']
        assert admit(cert,budget) and not admit(cert,budget-1)
        assert admit(cert,0,prior_reserved=budget) and not admit(cert,0,prior_reserved=budget-1)
        reports.append(dict(case=label,state_bytes=len(state_raw),result_bytes=len(result_raw),
            journal_bytes=len(journal_raw),state_sha256=hashlib.sha256(state_raw).hexdigest(),
            result_sha256=hashlib.sha256(result_raw).hexdigest(),journal_sha256=hashlib.sha256(journal_raw).hexdigest(),
            serialized_drain_bytes=actual_drain,certificate=cert))
    emoji=next(r for r in reports if r['case']=='emoji')
    assert emoji['state_bytes']==128901  # Security's original shape and bytes

    # Supplementary text in nested receipts, corrections and future evidence;
    # independent counts use two pending fills and four retained orders.
    counts={}
    for name in ('Correction','CorrectionRecord'):
        for field in ('root_fill_ids','corrected_fill_ids','surviving_fill_ids'):counts[name+'.'+field]=2
        for field in ('affected_order_hashes','cancelled_order_hashes'):counts[name+'.'+field]=4
    for field in ('created_fill_ids','corrected_fill_ids','committed_fill_ids','applied_batch_ids','correction_results'):
        counts['CommandResult.'+field]=2
    counts['CommandResult.affected_order_hashes']=4
    nested=[]
    for name in ('ConfirmedTx','BatchView','ResolutionReceipt','CorrectionRecord','Correction',
                 'CommandResult','Attempt','ResolutionEvidence','ChainSnapshot'):
        value=maximum(defs[name],defs,samples['emoji'],name,counts)
        validate(value,{'$ref':'#/$defs/'+name},defs)
        actual=len(canon(value));bound=Bounds(2,4).size(name)
        assert actual<=bound,(name,actual,bound)
        if name in ('Attempt','ResolutionEvidence','ChainSnapshot'):assert bound<=262144
        nested.append(dict(type=name,actual_bytes=actual,bound_bytes=bound))
    return dict(scope='SCHEMA/SERIALIZATION/RESERVATION MODEL; economic trace/chain/allocator IO NOT_RUN',
        finding='SEC-65-01',text_bound_bytes=bounds.size('Text'),text_cases=text_cases,
        full_shapes=reports,nested_shapes=nested,
        previous_candidate=dict(head='97f8338096bb7692042fd49bda56e9972a8316b4',
            text_bound_bytes=1538,state_bound_bytes=127518,state_actual_bytes=128901,state_underestimate_bytes=1383))


def run():
    got=measure()
    assert got==read('vectors/evidence-capacity.json')['unicode_capacity']
    print(f'PASS SEC-65-01 Unicode capacity: {len(got["text_cases"])} text cases, '
          f'{len(got["full_shapes"])} full S/R/J/B shapes, {len(got["nested_shapes"])} nested shapes; '
          'Text 3074B, B/B-1 and prior reservation boundaries; chain/allocator IO NOT_RUN')


if __name__=='__main__':run()
