"""Test-only DIRECT HTTP server, deliberately without a matching engine."""
import sys
from pathlib import Path
from bootstrap import genesis_manifest
from chain import RPC
from direct import Direct
from server import Server
from transport import Unavailable

class OfflineEngine:
    def request(self, *args):
        raise Unavailable('TEST_ENGINE_OFFLINE')

manifest, _ = genesis_manifest(Path(sys.argv[1]).read_bytes())
server = Server(('127.0.0.1', int(sys.argv[4])), OfflineEngine(),
                Direct(RPC(sys.argv[2]), manifest, sys.argv[3]))
try:
    server.serve_forever()
finally:
    server.server_close()
