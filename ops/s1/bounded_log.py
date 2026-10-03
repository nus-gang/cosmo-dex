"""Byte-bounded validator output; one writer per node, under supervisor lock."""
import os
from pathlib import Path
import threading

MAX_BYTES = 100 * 1024 * 1024
BACKUPS = 4
CHUNK_BYTES = 64 * 1024


class BoundedLog:
    def __init__(self, path, max_bytes=MAX_BYTES, backups=BACKUPS):
        if max_bytes < 1 or backups < 1:
            raise ValueError('positive log size and backup count required')
        self.path = Path(path)
        self.max_bytes, self.backups = max_bytes, backups
        # Never silently destroy legacy oversized evidence on startup.
        for p in [self.path] + [self.backup(i) for i in range(1, backups + 1)]:
            if p.exists() and p.stat().st_size > max_bytes:
                raise RuntimeError(f'oversized existing log: {p}; archive before restart')
        self.file = self.path.open('ab', buffering=0)
        self.size = self.file.tell()

    def backup(self, i):
        return self.path.with_name(self.path.name + f'.{i}')

    def rotate(self):
        self.file.close()
        self.backup(self.backups).unlink(missing_ok=True)
        for i in range(self.backups - 1, 0, -1):
            p = self.backup(i)
            if p.exists():
                p.replace(self.backup(i + 1))
        self.path.replace(self.backup(1))
        self.file = self.path.open('ab', buffering=0)
        self.size = 0

    def write(self, data):
        data = memoryview(data)
        while data:
            if self.size == self.max_bytes:
                self.rotate()
            count = min(len(data), self.max_bytes - self.size)
            n = self.file.write(data[:count])
            if not n:
                raise OSError('log write made no progress')
            self.size += n
            data = data[n:]

    def close(self):
        self.file.close()


class LogPump:
    """Drain merged stdout/stderr without a growing queue or line buffer."""
    def __init__(self, stream, sink):
        self.stream, self.sink = stream, sink
        self.error = None
        self.thread = threading.Thread(target=self.run, daemon=True)
        self.thread.start()

    def run(self):
        try:
            while True:
                chunk = os.read(self.stream.fileno(), CHUNK_BYTES)
                if not chunk:
                    break
                self.sink.write(chunk)
        except Exception as error:
            self.error = error
        finally:
            try:
                self.stream.close()
                self.sink.close()
            except Exception as error:
                self.error = self.error or error

    def finish(self):
        self.thread.join(timeout=5)
        if self.thread.is_alive():
            raise RuntimeError('log drain did not finish in 5 seconds')
        if self.error:
            raise RuntimeError(f'validator log write/rotation failed: {self.error}')
