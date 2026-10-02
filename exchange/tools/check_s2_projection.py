"""Apply the pinned contract's fixture-schema oracle to Rust state projections.

This reuses only `valid`, not the contract manifest checker (which pins the
pre-implementation Cargo.lock). It is a fixture check, not runtime validation.
"""
import ast
import base64
import json
from pathlib import Path
import re
import sys

root = Path(__file__).resolve().parents[2]
contract = root / 'protocol/s2'
source = ast.parse((contract / 'tools/check.py').read_text())
validator = next(n for n in source.body if isinstance(n, ast.FunctionDef) and n.name == 'valid')
namespace = {'defs': json.loads((contract / 'schema.json').read_text())['$defs'],
             'base64': base64, 're': re}
exec(compile(ast.Module(body=[validator], type_ignores=[]), 'contract valid()', 'exec'), namespace)
projections = json.loads(Path(sys.argv[1]).read_text())
for state in projections:
    namespace['valid']({'$ref': '#/$defs/EngineState'}, state)
print(f'PASS: {len(projections)} EngineState projections; pinned fixture schema oracle')
