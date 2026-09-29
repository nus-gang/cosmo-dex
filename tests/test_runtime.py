#!/usr/bin/env python3
"""Exercise runtime routing with a fake curl; never contacts an API."""
import json
import os
from pathlib import Path
import subprocess
import tempfile

base = Path(__file__).resolve().parents[1]
results = []
with tempfile.TemporaryDirectory(dir=os.environ.get("PAPERCLIP_RUN_SCRATCH_DIR"),
                                 prefix="sre-runtime-") as tmp:
    root = Path(tmp)
    mock = root / "curl"
    mock.write_text("""#!/usr/bin/env python3
import json, os, sys
from pathlib import Path
args = sys.argv[1:]
assert '-fsS' in args, 'HTTP errors must fail'
post = '-X' in args and args[args.index('-X') + 1] == 'POST'
with open(os.environ['MOCK_CALLS'], 'a') as f:
    f.write(json.dumps(args) + '\\n')
if os.environ['MOCK_FAIL'] == ('post' if post else 'get'):
    sys.exit(22)
print('{}' if post else os.environ['MOCK_CONTEXT'])
""")
    mock.chmod(0o700)

    def check(name, workspace, command="four-validators", action="start",
              fail="", expected=0, post=False, omit_workspace=False):
        calls = root / "calls.jsonl"
        calls.write_text("")
        env = {**os.environ, "PATH": str(root) + os.pathsep + os.environ["PATH"],
               "PAPERCLIP_API_URL": "https://mock.invalid/api/",
               "PAPERCLIP_API_KEY": "mock-token", "PAPERCLIP_TASK_ID": "mock-issue",
               "PAPERCLIP_RUN_ID": "mock-run", "MOCK_CALLS": str(calls),
               "MOCK_FAIL": fail,
               "MOCK_CONTEXT": json.dumps({} if omit_workspace else
                                         {"currentExecutionWorkspace": workspace})}
        env.pop("SRE_WORKSPACE_COMMAND_ID", None)
        # Keep the mock PATH hermetic; shell startup must not replace it.
        env.pop("BASH_ENV", None)
        if command is not None:
            env["SRE_WORKSPACE_COMMAND_ID"] = command
        run = subprocess.run(["bash", str(base / "ops/runtime.sh"), action],
                             env=env, capture_output=True, text=True)
        assert run.returncode == expected, (name, run.returncode, run.stderr)
        requests = [json.loads(line) for line in calls.read_text().splitlines()]
        posts = [r for r in requests if "-X" in r]
        assert len(posts) == int(post), (name, posts)
        if post:
            request = posts[0]
            assert json.loads(request[request.index("--data-binary") + 1]) == {
                "workspaceCommandId": command}
            assert "https://mock.invalid/api/execution-workspaces/review-fixture/runtime-services/" + action in request
            assert "X-Paperclip-Run-Id: mock-run" in request
        if expected == 2:
            assert "BLOCKED" in run.stderr
        results.append({"case": name, "exit_code": run.returncode,
                        "post_count": len(posts), "status": "PASS"})

    empty = {"id": "review-fixture", "runtimeServices": []}
    check("first start with empty services", empty, post=True)
    check("workspace null", None, expected=2)
    check("workspace absent", None, expected=2, omit_workspace=True)
    check("workspace missing id", {"runtimeServices": []}, expected=2)
    check("command unset", empty, command=None, expected=1)
    check("command empty", empty, command="", expected=1)
    check("context HTTP failure", empty, fail="get", expected=22)
    check("start HTTP failure", empty, fail="post", expected=22, post=True)
    unrelated = {"id": "review-fixture", "runtimeServices": [
        {"runtimeServiceId": "other-service", "workspaceCommandId": "other-command"}]}
    for action in ("start", "stop", "restart"):
        check("only explicit command: " + action, unrelated, action=action, post=True)

print(json.dumps({"status": "PASS", "actual_runtime_started": False,
                  "transport": "mock curl", "cases": results}, indent=2))
