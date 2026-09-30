#!/usr/bin/env python3
"""Real CLI initialization: public-key propagation and immutable homes, no server."""
import argparse
import json
from pathlib import Path
import tempfile
from types import SimpleNamespace

from devnet import ROOT, digest, init, load


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--binary', type=Path, default=ROOT/'chain/app/bin/nusd')
    a = parser.parse_args()
    with tempfile.TemporaryDirectory(prefix='nus-init-') as tmp:
        root = Path(tmp)
        args = SimpleNamespace(home=root/'fixture', binary=a.binary,
                               operators=ROOT/'chain/app/config/operator-accounts.json',
                               base_port=28656)
        fixture = init(args)
        fixture_genesis = json.loads((args.home/'node0/config/genesis.json').read_text())
        keys = list(reversed(fixture_genesis['app_state']['public_keys']))
        key_file = root/'users.json'
        key_file.write_text(json.dumps(keys))
        args.home = root/'wallet'
        args.user_public_keys = key_file
        manifest = init(args)
        load(args.home)
        assert manifest['genesis_sha256'] != fixture['genesis_sha256']
        for n in manifest['nodes']:
            path = Path(n['home'])/'config/genesis.json'
            genesis = json.loads(path.read_text())
            assert genesis['app_state']['public_keys'] == keys
            assert digest(path) == manifest['genesis_sha256']
            assert len({v['address'] for v in genesis['validators']}) == 4
            assert [int(v['power']) for v in genesis['validators']] == [10]*4
        try:
            init(args)
        except RuntimeError as error:
            assert 'refusing existing home' in str(error)
        else:
            raise AssertionError('existing home was accepted')
        load(args.home)
        key_file.write_text('[]')
        args.home = root/'invalid'
        try:
            init(args)
        except RuntimeError:
            assert not (args.home/'manifest.json').exists()
        else:
            raise AssertionError('invalid keys were accepted')
        print('PASS: fixture default, public-key order on four nodes, common genesis, 4x10 power, existing-home refusal, invalid-input refusal')


if __name__ == '__main__':
    main()
