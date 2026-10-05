"""Contract fixture helpers, NOT a product fsync/object store implementation."""
import hashlib
import json
import re
from pathlib import Path
from codec import S3, canon

RPC = 'application/json'
TX = 'application/vnd.nus.txraw'
JSON = 'application/vnd.nus.s3+json'
LIMITS = {RPC: 16777216, TX: 139264, JSON: 262144}
OBJECTS = S3/'vectors/evidence-objects'


def reference(raw, media=RPC):
    if media not in LIMITS or not 0 < len(raw) <= LIMITS[media]:
        raise ValueError('EVIDENCE_SIZE')
    return dict(sha256=hashlib.sha256(raw).hexdigest(), byte_length=str(len(raw)), media_type=media)


def put(raw, media=RPC):
    ref = reference(raw, media)
    OBJECTS.mkdir(exist_ok=True)
    path = OBJECTS/ref['sha256']
    if path.exists() and path.read_bytes()!=raw:
        raise ValueError('EVIDENCE_COLLISION')
    path.write_bytes(raw)
    return ref


def refs(value):
    found = {}
    def walk(v):
        if isinstance(v, dict):
            if set(v)=={'sha256','byte_length','media_type'}:
                previous=found.setdefault(v['sha256'],v)
                if previous!=v:raise ValueError('EVIDENCE_REF_CONFLICT')
            else:
                for x in v.values():walk(x)
        elif isinstance(v,list):
            for x in v:walk(x)
    walk(value)
    return [found[k] for k in sorted(found)]


def resolve(ref, objects=OBJECTS):
    if set(ref)!={'sha256','byte_length','media_type'} or not re.fullmatch('[0-9a-f]{64}',ref['sha256']):
        raise ValueError('EVIDENCE_REF')
    length=ref['byte_length'];media=ref['media_type']
    if not isinstance(length,str) or not re.fullmatch('[1-9][0-9]*',length) or media not in LIMITS or int(length)>LIMITS[media]:
        raise ValueError('EVIDENCE_SIZE')
    if isinstance(objects,dict):
        raw=objects.get(ref['sha256'])
        if raw is None:raise ValueError('EVIDENCE_MISSING')
    else:
        path=Path(objects)/ref['sha256']
        if path.is_symlink() or not path.is_file():raise ValueError('EVIDENCE_MISSING')
        raw=path.read_bytes()
    if len(raw)!=int(length) or hashlib.sha256(raw).hexdigest()!=ref['sha256']:
        raise ValueError('EVIDENCE_HASH_OR_LENGTH')
    if media in (RPC,JSON):
        def unique(items):
            out={}
            for key,value in items:
                if key in out:raise ValueError('DUPLICATE_KEY')
                out[key]=value
            return out
        value=json.loads(raw.decode('utf-8'),object_pairs_hook=unique,parse_constant=lambda s: (_ for _ in ()).throw(ValueError(s)))
        if media==JSON and canon(value)!=raw:raise ValueError('NONCANONICAL_OBJECT')
    return raw


def verify_graph(value, objects=OBJECTS):
    """Hash every transitive canonical object before typed/chain semantic checks."""
    seen={};active=set()
    def visit(v):
        for ref in refs(v):
            key=ref['sha256']
            if key in active:raise ValueError('EVIDENCE_CYCLE')
            if key in seen:
                if seen[key]!=ref:raise ValueError('EVIDENCE_REF_CONFLICT')
                continue
            raw=resolve(ref,objects);active.add(key)
            if ref['media_type']==JSON:visit(json.loads(raw))
            active.remove(key);seen[key]=ref
    visit(value)
    return [seen[k] for k in sorted(seen)]
