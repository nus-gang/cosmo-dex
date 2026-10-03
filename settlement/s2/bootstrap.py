#!/usr/bin/env python3
"""Bind a new S2 engine to exact genesis bytes and committed height one."""
import argparse
import base64
import hashlib
import os
from pathlib import Path

from chain import Collector, RPC, durable_file
from transport import Unavailable, decode, encode

ROOT = Path(__file__).resolve().parents[2]


def genesis_manifest(raw):
    genesis = decode(raw)
    profile_raw = (ROOT / 'protocol/s2/profile.json').read_bytes()
    profile = decode(profile_raw)
    pin = decode((ROOT / 'protocol/s2/manifest.json').read_bytes())
    if hashlib.sha256(profile_raw).hexdigest() != pin['config_sha256']:
        raise Unavailable('PROFILE_HASH_MISMATCH')
    if genesis['chain_id'] != profile['chain_id'] or str(genesis.get('initial_height', '1')) != '1':
        raise Unavailable('GENESIS_CONTEXT')
    keys = genesis['app_state']['public_keys']
    if not isinstance(keys, list) or len(keys) != 2:
        raise Unavailable('GENESIS_KEYS')
    identities = []
    for key in keys:
        public = base64.b64decode(key, validate=True)
        if len(public) != 1952 or base64.b64encode(public).decode() != key:
            raise Unavailable('GENESIS_KEYS')
        identities.append((hashlib.sha256(public).digest()[:20], key))
    identities.sort()
    if identities[0][0] == identities[1][0]:
        raise Unavailable('GENESIS_KEYS')
    context = {'schema_version': '1', 'chain_id': profile['chain_id'],
               'genesis_hash': hashlib.sha256(raw).hexdigest(),
               'contract_hash': pin['contract_sha256'], 'config_hash': pin['config_sha256'],
               'market_id': profile['market_id'],
               'market_config_version': profile['market_config_version']}
    market = {key: profile[key] for key in (
        'market_id', 'base_atoms_per_lot', 'quote_atoms_per_lot_tick', 'min_qty_lots',
        'max_qty_lots', 'min_price_ticks', 'max_price_ticks', 'max_order_quote_atoms')}
    market.update(config_version=profile['market_config_version'], base_denom='DEVBASE',
                  quote_denom='DEVQUOTE', fee_policy_version=profile['active_fee_version'],
                  fee_bps=next(f['bps'] for f in profile['fee_profiles']
                               if f['version'] == profile['active_fee_version']))
    manifest = {'context': context, 'market': market,
                'owners': [base64.b64encode(owner).decode() for owner, _ in identities],
                'supplies': [str(int(a['initial_bank_atoms_per_user']) * len(keys))
                             for a in profile['assets']]}
    return manifest, [key for _, key in identities]


def prepare(genesis_path, output, rpc):
    raw = Path(genesis_path).read_bytes()
    manifest, keys = genesis_manifest(raw)
    output = Path(output)
    # Exclusive directory ownership; never overwrite a partial or existing run.
    output.mkdir(parents=False, exist_ok=False)
    snapshot = Collector(rpc, None, manifest, output / 'rpc-evidence').fetch(1)
    accounts = snapshot['body']['accounts']
    if ([a['public_key'] for a in accounts] != keys
            or any(a['public_key_type'] != 'ML_DSA_65' for a in accounts)):
        raise Unavailable('GENESIS_REGISTERED_KEYS')
    manifest['bootstrap_snapshot_id'] = snapshot['snapshot_id']
    durable_file(output / 'genesis.json', raw)
    durable_file(output / 'bootstrap.json', encode(snapshot))
    # Manifest is the final published marker. Failures preserve evidence for inspection.
    durable_file(output / 'manifest.json', encode(manifest))
    fd = os.open(output.parent, os.O_RDONLY)
    try:
        os.fsync(fd)
    finally:
        os.close(fd)
    return manifest


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--genesis', required=True)
    parser.add_argument('--output', required=True, help='new directory under an existing parent')
    parser.add_argument('--rpc', default='http://127.0.0.1:26657')
    args = parser.parse_args()
    try:
        manifest = prepare(args.genesis, args.output, RPC(args.rpc))
    except (Unavailable, OSError, ValueError, KeyError, TypeError) as exc:
        parser.exit(1, f'bootstrap failed: {exc}\n')
    print(encode(manifest).decode())


if __name__ == '__main__':
    main()
