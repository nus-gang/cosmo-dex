#!/usr/bin/env python3
"""Execute the workflow regression block with mock tools and injected failures."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    workflow = (ROOT / '.github/workflows/s2-integration.yml').read_text()
    assert '    defaults:\n      run:\n        shell: bash\n' in workflow
    block = workflow.split('      - name: S0 S1 regression and S2 local failures\n        run: |\n', 1)[1].split('      - name:', 1)[0]
    script = '\n'.join(line[10:] for line in block.splitlines()) + '\necho REACHED_END\n'
    results = []
    with tempfile.TemporaryDirectory(prefix='s2-ci-failure-') as temporary:
        root = Path(temporary)
        (root / 'web').mkdir()
        (root / 'bin').mkdir()
        mock = '''#!/bin/bash
case "${0##*/} $*" in
  'python3 ops/s1/log-test.py') stage=log ;;
  'python3 ops/s1/init-test.py') stage=init ;;
  cargo*) stage=engine ;;
  npm*) stage=s0_wallet ;;
  node*) stage=s1_s2_wallet ;;
  *) stage=other ;;
esac
echo "mock:$stage"
if [ "$stage" = "$FAIL_STAGE" ]; then exit 23; fi
'''
        for tool in ('python3', 'cargo', 'npm', 'node'):
            path = root / 'bin' / tool
            path.write_text(mock)
            path.chmod(0o755)
        for stage in ('none', 'log', 'init', 'engine', 's0_wallet', 's1_s2_wallet'):
            for pipefail in (False, True):
                command = ['bash', '--noprofile', '--norc', '-e']
                if pipefail:
                    command += ['-o', 'pipefail']
                command += ['-c', script]
                env = dict(os.environ, PATH=str(root / 'bin') + os.pathsep + os.environ['PATH'], FAIL_STAGE=stage)
                env.pop('BASH_ENV', None)
                env.pop('ENV', None)
                run = subprocess.run(command, cwd=root, env=env, text=True, capture_output=True)
                expected = 23 if pipefail and stage != 'none' else 0
                logs = {str(p.relative_to(root)): p.read_text() for p in (root / '.evidence').rglob('*.log')}
                record = dict(stage=stage, pipefail=pipefail, exit_code=run.returncode, expected=expected,
                              reached_end='REACHED_END' in run.stdout, stdout=run.stdout, stderr=run.stderr, logs=logs)
                results.append(record)
                assert run.returncode == expected, record
                assert record['reached_end'] == (expected == 0), record
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(dict(workflow_sha256=hashlib.sha256(workflow.encode()).hexdigest(),
                                           scope='mock commands; actual workflow run block; no product execution',
                                           cases=results), indent=2) + '\n')
    print('PASS: 5 injected failures propagate exit 23; success and legacy-shell controls verified (12 cases)')


if __name__ == '__main__':
    main()
