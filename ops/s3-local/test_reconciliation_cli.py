import json
from pathlib import Path
import subprocess
import sys
import unittest
import test_source_reconciliation as fixture


class CLI(unittest.TestCase):
    setUp = fixture.Reconciliation.setUp
    git = fixture.Reconciliation.git
    commit = fixture.Reconciliation.commit
    branch_pair = fixture.Reconciliation.branch_pair

    def argv(self, left, right, candidate):
        return [sys.executable, str(Path(__file__).with_name('source_reconciliation.py')),
                '--source', str(self.root), '--left', left, '--right', right,
                '--candidate', candidate, '--prefix', 'web/', '--scratch', str(self.root)]

    def test_exact_and_mismatch_have_evidence_and_distinct_exit(self):
        l, r = self.branch_pair('LEFT\nmiddle\nlast\n', 'first\nmiddle\nRIGHT\n')
        (self.root/'web/a.ts').write_text('LEFT\nmiddle\nRIGHT\n')
        candidate = self.commit()
        before = self.git('status', '--porcelain')
        for head, status in ((candidate, 0), (r, 1)):
            result = subprocess.run(self.argv(l, r, head), capture_output=True, timeout=30)
            self.assertEqual(result.returncode, status, result.stderr)
            report = json.loads(result.stdout)
            self.assertEqual(report['candidate_head'], head)
            self.assertEqual(report['exact_reconciled_tree'], status == 0)
            self.assertFalse(report['approval_verified'])
            self.assertFalse(report['sealing_exception_granted'])
            self.assertEqual(result.stderr, b'')
        self.assertEqual(self.git('status', '--porcelain'), before)
        self.assertEqual(sorted(p.name for p in self.root.iterdir()), ['.git', 'web'])

    def test_invalid_inputs_do_not_wait_for_stdin_or_emit_report(self):
        head = self.source
        base = self.argv(head, head, head)
        for argv in (base+['--left', head], base+['--cand', head],
                     self.argv(head, head, 'HEAD'), base+['--secret-token-example']):
            child = subprocess.Popen(argv, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                     stderr=subprocess.PIPE)
            try:
                child.wait(timeout=20)
                self.assertEqual(child.returncode, 2)
                self.assertEqual(child.stdout.read(), b'')
                self.assertEqual(child.stderr.read(), b'SOURCE_RECONCILIATION_INVALID\n')
            finally:
                if child.poll() is None:
                    child.kill(); child.wait()
                for stream in (child.stdin, child.stdout, child.stderr): stream.close()


if __name__ == '__main__': unittest.main()
