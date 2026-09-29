#!/usr/bin/env bash
# Only use Paperclip management. No background process fallback.
set -euo pipefail
case "${1:-}" in start|stop|restart) action="$1";; *) echo 'usage: runtime.sh start|stop|restart' >&2; exit 2;; esac
: "${PAPERCLIP_API_URL:?}" "${PAPERCLIP_API_KEY:?}" "${PAPERCLIP_TASK_ID:?}" "${PAPERCLIP_RUN_ID:?}"
base="${PAPERCLIP_API_URL%/}"; base="${base%/api}"
context=$(curl -fsS -H "Authorization: Bearer $PAPERCLIP_API_KEY" "$base/api/issues/$PAPERCLIP_TASK_ID/heartbeat-context")
workspace=$(python3 -c 'import sys,json; w=json.load(sys.stdin).get("currentExecutionWorkspace"); print(w.get("id", "") if w else "")' <<<"$context")
if [[ -z "$workspace" ]]; then
  echo 'BLOCKED: managed execution workspace is not configured' >&2
  exit 2
fi
# Target only an explicitly configured service; do not stop unrelated services.
: "${SRE_WORKSPACE_COMMAND_ID:?set the configured four-validator supervisor command id}"
payload=$(python3 -c 'import json,os; print(json.dumps({"workspaceCommandId":os.environ["SRE_WORKSPACE_COMMAND_ID"]}))')
curl -fsS -X POST -H "Authorization: Bearer $PAPERCLIP_API_KEY" -H "X-Paperclip-Run-Id: $PAPERCLIP_RUN_ID" -H 'Content-Type: application/json' "$base/api/execution-workspaces/$workspace/runtime-services/$action" --data-binary "$payload"
