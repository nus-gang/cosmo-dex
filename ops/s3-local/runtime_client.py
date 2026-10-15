"""Scoped current-run Paperclip transport for future L-T orchestration.

No registration, restart, wildcard target, retry, or executable CLI. Construction
is not approval; callers must retain fresh audit and managed-session boundaries.
"""
import json
import os
import time
import urllib.request

from manifest import decode
from native_review import _uuid
from paperclip_reader import Reader
from workspace_registration import PROJECT

MAX_RESPONSE = 8 * 1024 * 1024
ERROR = 'RUNTIME_CONTROL_FAILED'


class RuntimeClient:
    def __init__(self, base, token, run_id, workspace_id, command_id):
        try:
            auth = Reader(base, token)
            self._run_id = _uuid(run_id)
            self._workspace_id = _uuid(workspace_id)
            if command_id not in ('s3-worker-fee0', 's3-worker-fee25',
                                  's3-web-fee0', 's3-web-fee25',
                                  *(f's3-chain-fee{fee}-v{index}'
                                    for fee in (0, 25) for index in range(4))):
                raise ValueError()
            self._command_id = command_id
            self._base, self._token, self._opener = auth._base, auth._token, auth._opener
            self._listing = f'/api/projects/{PROJECT}/workspaces'
            self._actions = {self._listing + f'/{workspace_id}/runtime-services/{action}'
                             for action in ('start', 'stop')}
        except (ValueError, TypeError, AttributeError):
            raise ValueError('RUNTIME_CONTROL_CONFIG') from None

    @classmethod
    def from_environment(cls, workspace_id, command_id):
        return cls(os.environ.get('PAPERCLIP_API_URL'), os.environ.get('PAPERCLIP_API_KEY'),
                   os.environ.get('PAPERCLIP_RUN_ID'), workspace_id, command_id)

    def read_workspaces(self, path):
        if path != self._listing:
            raise ValueError('RUNTIME_CONTROL_PATH')
        return self._exchange('GET', path, None, list)

    def request(self, packet):
        # Exact body disallows empty/all-service requests and additional selectors.
        if (not isinstance(packet, dict) or set(packet) != {'method', 'path', 'body'} or
                packet['method'] != 'POST' or not isinstance(packet['path'], str) or
                packet['path'] not in self._actions or
                packet['body'] != {'workspaceCommandId': self._command_id}):
            raise ValueError('RUNTIME_CONTROL_PATH')
        raw = json.dumps(packet['body'], separators=(',', ':')).encode('ascii')
        return self._exchange('POST', packet['path'], raw, dict)

    def _exchange(self, method, path, raw, result_type):
        headers = {'Authorization': 'Bearer ' + self._token, 'Accept': 'application/json',
                   'Cache-Control': 'no-cache', 'X-Paperclip-Run-Id': self._run_id}
        if raw is not None:
            headers['Content-Type'] = 'application/json'
        request = urllib.request.Request(self._base + path, data=raw, headers=headers, method=method)
        try:
            deadline = time.monotonic() + 10
            with self._opener.open(request, timeout=5) as response:
                if (response.status != 200 or response.geturl() != self._base + path or
                        response.headers.get_content_type() != 'application/json'):
                    raise ValueError()
                chunks, size = [], 0
                while True:
                    chunk = response.read(min(65536, MAX_RESPONSE + 1 - size))
                    size += len(chunk)
                    if size > MAX_RESPONSE or time.monotonic() > deadline:
                        raise ValueError()
                    if not chunk:
                        break
                    chunks.append(chunk)
                value = decode(b''.join(chunks))
                if not isinstance(value, result_type):
                    raise ValueError()
                return value
        except Exception:
            # A timed-out POST may have executed. Never retry or expose upstream text.
            raise ValueError(ERROR) from None
