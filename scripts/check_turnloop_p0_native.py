#!/usr/bin/env python3
"""Loopback fetch/WebSocket probes with server-side proof of real native I/O."""
import base64
import hashlib
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import os
from pathlib import Path
import re
import socket
import subprocess
import tempfile
import threading

ROOT = Path(__file__).resolve().parents[1]
TARGET = Path(os.environ.get("CARGO_TARGET_DIR", ROOT / "target")).resolve()
COMPILER = Path(os.environ.get("PERRY_BIN", TARGET / "perry-dev/perry"))
RUNTIME = Path(os.environ.get("PERRY_RUNTIME_DIR", COMPILER.parent)).resolve()
ENV = dict(os.environ, PERRY_RUNTIME_DIR=str(RUNTIME), PERRY_NO_AUTO_OPTIMIZE="1")
assert subprocess.check_output(["node", "--version"], text=True).strip() == "v26.5.1"

class Handler(BaseHTTPRequestHandler):
    requests = 0
    def do_GET(self):
        Handler.requests += 1
        payload = b"p0 fetch"
        self.send_response(200)
        self.send_header("Content-Length", str(len(payload)))
        self.send_header("Connection", "close")
        self.end_headers()
        self.wfile.write(payload)
    def log_message(self, *args):
        pass

def compare(source, name, expected, folder):
    path = folder / f"{name}.ts"
    binary = folder / name
    path.write_text(source)
    compiled = subprocess.run([str(COMPILER), str(path), "--no-cache", "-o", str(binary)],
                              env=ENV, capture_output=True, text=True, timeout=180)
    assert compiled.returncode == 0, compiled.stderr
    oracle = subprocess.check_output(["node", "--experimental-strip-types", str(path)],
                                     text=True, timeout=15)
    actual = subprocess.run([str(binary)], env=dict(ENV, PERRY_LOOP_STATS="1"),
                            capture_output=True, text=True, timeout=15)
    assert actual.returncode == 0, (name, actual.stderr)
    assert actual.stdout == oracle == expected, (name, actual.stdout, oracle, actual.stderr)
    ticks = re.findall(r"native_ticks=(\d+)", actual.stderr)
    assert len(ticks) == 1 and int(ticks[0]) > 0, (name, "Tokio subject never ran", actual.stderr)
    print(f"PASS {name}: {actual.stdout.strip()}; native_ticks={ticks[0]}", flush=True)

def websocket_server(listener, errors, accepted):
    try:
        for _ in range(2):  # Node oracle plus Perry
            conn, _ = listener.accept()
            with conn:
                conn.settimeout(15)
                request = b""
                while b"\r\n\r\n" not in request:
                    request += conn.recv(4096)
                key = re.search(br"(?im)^sec-websocket-key:\s*(.*?)\r?$", request).group(1)
                accept = base64.b64encode(hashlib.sha1(
                    key + b"258EAFA5-E914-47DA-95CA-C5AB0DC85B11").digest())
                conn.sendall(b"HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\n"
                             b"Connection: Upgrade\r\nSec-WebSocket-Accept: " + accept + b"\r\n\r\n")
                payload = b"p0 websocket"
                conn.sendall(bytes([0x81, len(payload)]) + payload)
                accepted.append(True)
                close = conn.recv(4096)
                assert close and close[0] & 15 == 8, "client never closed the WebSocket"
                conn.sendall(b"\x88\x00")
    except BaseException as error:
        errors.append(repr(error))

with tempfile.TemporaryDirectory(prefix="perry-p0-native-") as directory:
    folder = Path(directory)
    server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    try:
        compare(f'async function main() {{ const r = await fetch("http://127.0.0.1:{server.server_port}/"); '
                'console.log(await r.text()); } main();',
                "fetch", "p0 fetch\n", folder)
        assert Handler.requests == 2, "both native HTTP subjects must reach the server"
    finally:
        server.shutdown()
        server.server_close()
        thread.join()
    with socket.socket() as listener:
        listener.bind(("127.0.0.1", 0))
        listener.listen()
        listener.settimeout(30)
        errors, accepted = [], []
        thread = threading.Thread(target=websocket_server, args=(listener, errors, accepted), daemon=True)
        thread.start()
        compare(f'const ws = new WebSocket("ws://127.0.0.1:{listener.getsockname()[1]}/"); '
                'ws.onmessage = (event) => { console.log(event.data); ws.close(); };',
                "websocket", "p0 websocket\n", folder)
        thread.join(timeout=20)
        assert not thread.is_alive() and not errors, errors
        assert len(accepted) == 2, "both native WebSocket subjects must handshake"
