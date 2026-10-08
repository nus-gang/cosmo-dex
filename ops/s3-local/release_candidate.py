#!/usr/bin/env python3
"""Verify one sealed L-R candidate without starting any service.

This check binds the final build spec, five descriptors, build artifacts and
runtime consumers.  It deliberately substitutes the native child call after
the exact reviewed executable has been selected, copied, hashed and made
private.  It is not an approval or a reusable launch permit.
"""
import argparse
import base64
import hashlib
import json
from pathlib import Path
import re
from unittest.mock import patch

from captured_web import ASSETS, capture_assets
from manifest import COMPONENTS, decode, encode, read_regular, relative
import offline_check
from preflight import verify


SRE_NATIVE = (
    'bin/s3-local-preflight',
    'bin/s3-local-bootstrap-check',
    'bin/s3-local-bootstrap-create',
    'bin/s3-local-bootstrap-fetch',
    'bin/s3-local-worker',
    'bin/nus-s3-local-direct',
    'bin/s3-local-storage-fault',
    'bin/s3-local-storage-crash',
    'bin/s3-local-storage-correction',
    'bin/s3-local-before-send',
    'bin/s3-local-receipt-apply',
)
SHA256 = re.compile('[0-9a-f]{64}')


def sha(raw):
    return hashlib.sha256(raw).hexdigest()


def canonical_json(raw, kind):
    try:
        value = decode(raw.encode())
    except Exception:
        raise ValueError(kind) from None
    if encode(value).decode().strip() != raw:
        raise ValueError(kind)
    return value


def descriptor(bundle, manifest, name):
    path = manifest['components'][name]
    raw = read_regular(bundle / 'files', path)
    if sha(raw) != manifest['files_sha256'][path]:
        raise ValueError('DESCRIPTOR_BYTES_CHANGED: ' + name)
    return decode(raw)


def verify_spec(source, spec):
    if set(spec) != set(COMPONENTS):
        raise ValueError('EXACTLY_FIVE_COMPONENTS_REQUIRED')
    report = {}
    for name in COMPONENTS:
        item = spec[name]
        if set(item) != {'build_argv', 'toolchain', 'artifacts',
                         'approval_sources', 'settings'}:
            raise ValueError('COMPONENT_SPEC_FIELDS: ' + name)
        settings = item['settings']
        required = {'build_cwd', 'build_env_json', 'build_inputs_sha256_json'}
        if not isinstance(settings, dict) or not required.issubset(settings):
            raise ValueError('BUILD_PROVENANCE_REQUIRED: ' + name)
        declared_cwd = settings['build_cwd']
        # A build may run at the exact source root. Keep that representation
        # distinct from paths accepted by relative(), which intentionally
        # rejects dot segments for captured files.
        cwd = '.' if declared_cwd == '.' else relative(declared_cwd)
        if not (source if cwd == '.' else source / cwd).is_dir():
            raise ValueError('BUILD_CWD_MISSING: ' + name)
        env = canonical_json(settings['build_env_json'], 'BUILD_ENV_INVALID: ' + name)
        if not isinstance(env, dict) or not all(isinstance(k, str) and
                isinstance(v, str) for k, v in env.items()):
            raise ValueError('BUILD_ENV_INVALID: ' + name)
        inputs = canonical_json(settings['build_inputs_sha256_json'],
                                'BUILD_INPUTS_INVALID: ' + name)
        if not isinstance(inputs, dict) or not inputs:
            raise ValueError('BUILD_INPUTS_INVALID: ' + name)
        for path, digest in inputs.items():
            if not isinstance(digest, str) or not SHA256.fullmatch(digest):
                raise ValueError('BUILD_INPUTS_INVALID: ' + name)
            if sha(read_regular(source, relative(path))) != digest:
                raise ValueError('BUILD_INPUT_CHANGED: ' + path)
        dependencies = canonical_json(settings.get('dependency_inputs_sha256_json', '{}'),
                                      'BUILD_DEPENDENCIES_INVALID: ' + name)
        if not isinstance(dependencies, dict) or not all(isinstance(k, str) and
                isinstance(v, str) and SHA256.fullmatch(v)
                for k, v in dependencies.items()):
            raise ValueError('BUILD_DEPENDENCIES_INVALID: ' + name)
        argv = []
        for command in item['build_argv']:
            value = canonical_json(command, 'BUILD_ARGV_INVALID: ' + name)
            if not isinstance(value, list) or not value or not all(
                    isinstance(arg, str) and arg for arg in value):
                raise ValueError('BUILD_ARGV_INVALID: ' + name)
            argv.append(value)
        if name in ('chain', 'exchange', 'settlement', 'wallet'):
            head = settings.get('approved_head')
            tree = settings.get('approved_tree')
            if not isinstance(head, str) or not isinstance(tree, str):
                raise ValueError('APPROVED_SOURCE_REQUIRED: ' + name)
            import subprocess
            actual = subprocess.check_output(
                ['git', '-C', str(source), 'rev-parse', head + '^{tree}'],
                stderr=subprocess.PIPE).decode().strip()
            if actual != tree:
                raise ValueError('APPROVED_TREE_MISMATCH: ' + name)
        report[name] = {'cwd': cwd, 'environment': env, 'argv': argv,
                        'source_inputs': inputs, 'dependency_inputs': dependencies}
    if 'bin/nus-s3-local-chain' not in spec['chain']['artifacts']:
        raise ValueError('CHAIN_ARTIFACT_MISSING')
    missing = sorted(set(SRE_NATIVE) - set(spec['sre']['artifacts']))
    if missing:
        raise ValueError('SRE_NATIVE_ARTIFACT_MISSING: ' + ','.join(missing))
    all_artifacts = {path for item in spec.values() for path in item['artifacts']}
    if not set(ASSETS).issubset(all_artifacts):
        raise ValueError('CANONICAL_WEB_ARTIFACT_MISSING')
    return report


def verify_candidate(source, bundle, artifacts, spec, scratch, pin):
    manifest_raw = read_regular(bundle, 'runtime-manifest.json')
    if sha(manifest_raw) != pin:
        raise ValueError('RUNTIME_MANIFEST_MISMATCH')
    manifest = decode(manifest_raw)
    build = verify_spec(source, spec)
    byte_report = verify(bundle, artifacts, pin, 's3-dev-local/1', True)
    files = {}
    for path, digest in manifest['files_sha256'].items():
        raw = read_regular(bundle / 'files', path)
        if sha(raw) != digest:
            raise ValueError('CAPTURE_FILE_CHANGED: ' + path)
        files[path] = base64.b64encode(raw).decode()
    capture = encode({'runtime_manifest': base64.b64encode(manifest_raw).decode(),
                      'files': files,
                      'guard': base64.b64encode(b'REVIEW_ONLY_NOT_A_GUARD').decode(),
                      'genesis': base64.b64encode(b'REVIEW_ONLY_NOT_A_GENESIS').decode()})
    web = capture_assets(capture, artifacts)
    sre = descriptor(bundle, manifest, 'sre')
    inventory = decode(sre['implementation_settings']['artifacts_sha256_json'])
    missing = sorted(set(SRE_NATIVE) - set(inventory))
    if missing:
        raise ValueError('SRE_DESCRIPTOR_NATIVE_MISSING: ' + ','.join(missing))
    selected = {}
    scratch.mkdir(mode=0o700, parents=False, exist_ok=False)
    for artifact in SRE_NATIVE:
        with patch.object(offline_check, 'validate_captured',
                          return_value={'approval_verified': False,
                                        'services_started': False,
                                        'DEV': 'NOT_RUN'}) as child:
            digest, result = offline_check._validate_snapshot(
                capture, artifacts, [], scratch, 1, artifact)
        if child.call_count != 1 or digest != inventory[artifact]:
            raise ValueError('SRE_NATIVE_SELECTION_FAILED: ' + artifact)
        if any(scratch.iterdir()):
            raise ValueError('SRE_NATIVE_STAGE_NOT_CLEANED: ' + artifact)
        selected[artifact] = {'sha256': digest, **result}
    scratch.rmdir()
    return {
        'format': 's3-local-release-verification/1',
        'runtime_manifest_sha256': pin,
        'byte_preflight': byte_report,
        'build_provenance': build,
        'sre_native_selection': selected,
        'web': {'paths': sorted(ASSETS), 'capture_sha256': web.capture_sha256,
                'html_sha256': sha(web.html), 'javascript_sha256': sha(web.javascript)},
        'approval_verified': False,
        'services_started': False,
        'native_children_started': 0,
        'DEV': 'NOT_RUN',
        'durable_ack': False,
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--source', required=True, type=Path)
    parser.add_argument('--bundle', required=True, type=Path)
    parser.add_argument('--artifacts', required=True, type=Path)
    parser.add_argument('--build-spec', required=True, type=Path)
    parser.add_argument('--scratch', required=True, type=Path)
    parser.add_argument('--pin', required=True)
    parser.add_argument('--out', required=True, type=Path)
    args = parser.parse_args()
    report = verify_candidate(args.source.resolve(), args.bundle.resolve(),
                              args.artifacts.resolve(),
                              decode(args.build_spec.read_bytes()),
                              args.scratch.resolve(), args.pin)
    with args.out.open('xb') as stream:
        stream.write(encode(report))
    print(json.dumps({'runtime_manifest_sha256': args.pin,
                      'native_selections': len(SRE_NATIVE),
                      'web_assets': len(ASSETS),
                      'approval_verified': False,
                      'services_started': False}))


if __name__ == '__main__':
    try:
        main()
    except (ValueError, OSError) as error:
        raise SystemExit(str(error))
