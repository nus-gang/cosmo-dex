"""Internal immutable web asset capture. No listener, semantic or approval grant."""
import base64
import hashlib
import re
from dataclasses import dataclass
from manifest import decode, COMPONENTS
from preflight import bounded, checked_root
from static_web import StaticWeb

ASSETS = {'web/index.html': 65536, 'web/page.js': 4194304}

@dataclass(frozen=True)
class CapturedWeb:
    html: bytes
    javascript: bytes
    capture_sha256: str

    def response_boundary(self, origin, context):
        # Context must come from C semantic validation at the calling gate.
        return StaticWeb(origin=origin, html=self.html, javascript=self.javascript,
                         context=context)


def capture_assets(capture, artifacts):
    """Caller must supply byte-verified capture and separately validate C/approval.

    Cross-check every descriptor's occurrence of each asset, rejecting conflicts.
    Return bytes so later path replacement cannot change served content.
    """
    if type(capture) is not bytes or not 0 < len(capture) <= 48*1024*1024:
        raise ValueError('WEB_CAPTURE_REQUIRED')
    value = decode(capture)
    manifest_raw = base64.b64decode(value['runtime_manifest'], validate=True)
    manifest = decode(manifest_raw)
    inventory = {}
    for component in COMPONENTS:
        path = 'chain/local-demo/components/' + component + '.json'
        raw = base64.b64decode(value['files'][path], validate=True)
        if hashlib.sha256(raw).hexdigest() != manifest['files_sha256'][path]:
            raise ValueError('WEB_DESCRIPTOR_CHANGED')
        settings = decode(raw)['implementation_settings']
        hashes = decode(settings['artifacts_sha256_json'])
        for asset in ASSETS:
            if asset not in hashes:
                continue
            digest = hashes[asset]
            if type(digest) is not str or not re.fullmatch('[0-9a-f]{64}', digest):
                raise ValueError('WEB_ASSET_DIGEST')
            if asset in inventory and inventory[asset] != digest:
                raise ValueError('WEB_ASSET_CONFLICT')
            inventory[asset] = digest
    if set(inventory) != set(ASSETS):
        raise ValueError('WEB_ASSET_MISSING')
    root = checked_root(artifacts)
    contents = {}
    for asset, limit in ASSETS.items():
        raw = bounded(root, asset, limit)
        if not raw or hashlib.sha256(raw).hexdigest() != inventory[asset]:
            raise ValueError('WEB_ASSET_CHANGED')
        contents[asset] = raw
    return CapturedWeb(contents['web/index.html'], contents['web/page.js'],
                       hashlib.sha256(capture).hexdigest())
