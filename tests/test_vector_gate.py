"""Synthetic receipts test orchestration only, never protocol correctness."""
import hashlib
import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

SCRIPT = Path(__file__).resolve().parents[1] / "ops/ci/vectors.py"

class VectorGate(unittest.TestCase):
    def test_failure_boundaries(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            (root / "vector.json").write_text('{"synthetic":true}')
            sha = hashlib.sha256((root / "vector.json").read_bytes()).hexdigest()
            receipt = {"contract_revision": "test-only", "vectors_sha256": sha,
                       "results": [{"id": "stub", "sign_bytes_hex": "00", "valid": False}]}
            cfg = {"contract_revision": "test-only", "vectors": "vector.json",
                   "vectors_sha256": sha, "lanes": {}}
            for lang in ("go", "rust", "ts"):
                (root / lang).mkdir()
                (root / lang / "receipt.json").write_text(json.dumps(receipt))
                cfg["lanes"][lang] = {"cwd": lang, "build": [sys.executable, "-c", "pass"],
                    "test": [sys.executable, "-c", "pass"],
                    "vectors": [sys.executable, "-c", 'from pathlib import Path; print(Path("receipt.json").read_text())']}
            def run(code, status):
                (root / "manifest.json").write_text(json.dumps(cfg))
                result = subprocess.run([sys.executable, str(SCRIPT), "--repo", str(root),
                    "--config", str(root / "manifest.json"), "--output", str(root / "report.json")],
                    capture_output=True, text=True)
                self.assertEqual(result.returncode, code, result.stderr)
                self.assertEqual(json.loads((root / "report.json").read_text())["status"], status)
            run(0, "PASS")
            cfg["lanes"]["ts"]["vectors"] = None
            run(2, "BLOCKED")
            cfg["lanes"]["ts"]["vectors"] = [sys.executable, "-c", 'print("{}")']
            run(1, "FAIL")
            cfg["lanes"]["ts"]["vectors"] = [sys.executable, "-c", "print(" + repr(json.dumps({**receipt, "results": [{"id": "wrong"}]})) + ")"]
            run(1, "FAIL")
            cfg["vectors_sha256"] = "0" * 64
            run(1, "FAIL")
            cfg["vectors_sha256"] = sha
            cfg["lanes"]["go"]["build"] = [sys.executable, "-c", "raise SystemExit(9)"]
            run(1, "FAIL")

if __name__ == "__main__":
    unittest.main()
