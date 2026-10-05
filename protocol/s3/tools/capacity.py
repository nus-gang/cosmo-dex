"""Conservative serialized-size/credit oracle; no filesystem allocation claim.

Arrays that can grow without a protocol bound are explicitly supplied a count.
An unknown array is an error, never zero or a page-size default.
"""
import json
from codec import S3, canon

LIMIT = 16777216


def b64_size(n):
    return 4*((n+2)//3)


def array_size(count, item):
    return 2 + count*item + max(0,count-1)


def object_size(fields):
    return 2 + sum(len(canon(k))+1+v for k,v in fields.items()) + max(0,len(fields)-1)


class Bounds:
    def __init__(self, pending, orders):
        self.defs=json.loads((S3/'schema.json').read_text())['$defs']
        n,o=pending,orders
        self.counts={
            'Correction.root_fill_ids':n,'Correction.corrected_fill_ids':n,
            'Correction.affected_order_hashes':o,'Correction.cancelled_order_hashes':o,
            'Correction.surviving_fill_ids':n,
            'CorrectionRecord.root_fill_ids':n,'CorrectionRecord.corrected_fill_ids':n,
            'CorrectionRecord.affected_order_hashes':o,'CorrectionRecord.cancelled_order_hashes':o,
            'CorrectionRecord.surviving_fill_ids':n,
            'CommandResult.affected_order_hashes':o,'CommandResult.created_fill_ids':n,
            'CommandResult.corrected_fill_ids':n,'CommandResult.committed_fill_ids':n,
            'CommandResult.applied_batch_ids':n,'CommandResult.correction_results':n,
            'EngineAccount.ledger':2,
            'JournalRecord.external_event_ids':n+o,
        }

    def size(self, name):
        return self.spec(self.defs[name],name)

    def spec(self, spec, path):
        if '$ref' in spec:return self.size(spec['$ref'].split('/')[-1])
        if 'anyOf' in spec:return max(self.spec(v,path) for v in spec['anyOf'])
        if 'const' in spec:return len(canon(spec['const']))
        if 'enum' in spec:return max(len(canon(v)) for v in spec['enum'])
        kind=spec['type']
        if kind=='object':return object_size({k:self.spec(v,path+'.'+k) for k,v in spec['properties'].items()})
        if kind=='array':
            count=spec.get('maxItems',self.counts.get(path))
            if count is None:raise ValueError('UNBOUNDED_ARRAY '+path)
            return array_size(count,self.spec(spec['items'],path+'[]'))
        if kind=='boolean':return 5
        if kind=='null':return 4
        if kind=='string':
            if path.endswith('Hash'):return 66
            # Decimal/base64 alphabets do not need JSON escaping.
            factor=1 if 'pattern' in spec else 6
            return 2+factor*spec['maxLength']
        raise ValueError('UNBOUNDED_TYPE '+path)


def certificate(state):
    n=sum(f['state'] in ('PENDING','SUBMISSION_UNKNOWN') for f in state['fills'])
    o=len(state['orders'])
    bounds=Bounds(n,o)
    # Scalar mutable fields and every existing order/fill can reach their full
    # declared widths. Immutable signed order bytes use known exact lengths.
    def stored_order(order):
        return object_size({k:len(canon(v)) if k in ('owner','order_wire','signature') else bounds.size('OrderView')
                            for k,v in order.items()})
    state_fields={}
    preserved=('bindings','dependencies','corrections','resolution_receipts','applied_batches','attempt_refs')
    for key,spec in bounds.defs['EngineState']['properties'].items():
        if key in preserved:
            items=state[key]
            size=sum(len(canon(v)) for v in items)
            append={'bindings':0,'dependencies':0,'corrections':n,'resolution_receipts':n,
                    'applied_batches':n,'attempt_refs':25*n}[key]
            item_type=spec['items']['$ref'].split('/')[-1]
            count=len(items)+append
            state_fields[key]=2+size+append*bounds.size(item_type)+max(0,count-1)
        elif key=='orders':state_fields[key]=2+sum(stored_order(v) for v in state[key])+max(0,o-1)
        elif key in ('fills','batches'):
            count=len(state[key])+(n if key=='batches' else 0)
            state_fields[key]=array_size(count,bounds.size(spec['items']['$ref'].split('/')[-1]))
        else:state_fields[key]=bounds.spec(spec,'EngineState.'+key)
    smax=object_size(state_fields)
    rmax=bounds.size('CommandResult')
    # Union of retained object refs plus bounded future raw and canonical objects.
    from evidence import verify_graph
    retained=len(verify_graph(state))
    objects_per_fill=80+5+96
    future_objects=objects_per_fill*n+o+2
    journal_fields={}
    for key,spec in bounds.defs['JournalRecord']['properties'].items():
        if key in ('state_json','result_json'):
            journal_fields[key]=2+b64_size(smax if key=='state_json' else rmax)
        elif key=='evidence_refs':
            journal_fields[key]=array_size(retained+future_objects,bounds.size('EvidenceRef'))
        else:journal_fields[key]=bounds.spec(spec,'JournalRecord.'+key)
    jmax=object_size(journal_fields)
    p=json.loads((S3/'profile.json').read_text())
    align=int(p['storage_allocation_unit_bytes']);meta=int(p['storage_metadata_reserve_per_object_bytes'])
    alloc=lambda value:((value+align-1)//align)*align+meta
    q=40*n+o+2
    # Each record reserves new frame, two state/result checkpoint/index copies,
    # marker+temp. Existing committed files never count as reusable free space.
    per_record=alloc(72+jmax)+2*alloc(smax)+2*alloc(rmax)+2*alloc(4096)
    evidence=n*(80*alloc(16777216)+5*alloc(139264)+96*alloc(262144))+(o+2)*alloc(262144)
    return dict(pending_fills=n,orders=o,max_state_bytes=smax,max_result_bytes=rmax,
                max_journal_payload_bytes=jmax,max_drain_records=q,
                max_new_evidence_objects=future_objects,evidence_reserved_bytes=evidence,
                reserved_bytes=q*per_record+evidence,admissible=jmax<=LIMIT)


def admit(cert, available, prior_reserved=0):
    # Available excludes committed files and every other outstanding reservation.
    return cert['admissible'] and available>=max(0,cert['reserved_bytes']-prior_reserved)


def payload_fit(payload):
    return len(payload)<=LIMIT
