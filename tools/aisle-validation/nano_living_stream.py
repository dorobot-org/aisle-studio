"""Latest-frame Unix preview server; lossless recording remains a Dora sibling."""

from __future__ import annotations
import json
import os
import socket
import struct
import threading
from pathlib import Path


class PreviewHub:
    def __init__(self, path):
        self.path = Path(path)
        self.path.parent.mkdir(parents=True, exist_ok=True)
        if self.path.exists():
            raise ValueError("Preview socket already exists")
        self.socket = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        self.socket.bind(str(self.path))
        os.chmod(self.path, 0o600)
        self.socket.listen(4)
        self.socket.settimeout(0.25)
        self.condition = threading.Condition()
        self.latest = None
        self.generation = 0
        self.clients = 0
        self.closed = False
        threading.Thread(target=self.accept, daemon=True).start()

    def accept(self):
        while not self.closed:
            try:
                client, _ = self.socket.accept()
            except socket.timeout:
                continue
            except OSError:
                return
            with self.condition:
                self.clients += 1
            threading.Thread(target=self.serve, args=(client,), daemon=True).start()

    def serve(self, client):
        generation = -1
        client.settimeout(2.0)
        try:
            while not self.closed:
                with self.condition:
                    self.condition.wait_for(
                        lambda: self.closed or self.generation != generation,
                        timeout=1.0,
                    )
                    if self.closed:
                        return
                    if generation == self.generation:
                        continue
                    generation = self.generation
                    packet = self.latest
                if packet is not None:
                    client.sendall(packet)
        except (OSError, TimeoutError):
            pass
        finally:
            client.close()
            with self.condition:
                self.clients -= 1

    def publish(self, header, images):
        raw = json.dumps(header, allow_nan=False, separators=(",", ":")).encode()
        packet = struct.pack(">I", len(raw)) + raw + b"".join(images)
        with self.condition:
            self.latest = packet
            self.generation += 1
            self.condition.notify_all()

    def close(self):
        self.closed = True
        with self.condition:
            self.condition.notify_all()
        self.socket.close()
        self.path.unlink(missing_ok=True)
