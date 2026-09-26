#!/usr/bin/env python3
"""Native loopback probe: fetch and WebSocket run on turnloop.

Compile two loopback programs, compare their output against the pinned Node
oracle, and verify on the server side that both subjects made their requests.
Require nonzero turnloop turns and completions so a successful run also proves
that the native event loop actually ran.

Usage: scripts/turnloop_p0_native_probe.py [--perry target/perry-dev/perry] [--runtime-dir DIR]
"""

import argparse
import base64
import hashlib
import os
import re
import socket
import subprocess
import sys
import tempfile
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
STATS = re.compile(
    r"\[perry-loop\] driver=turnloop turns=(\d+) os_waits=(\d+) "
    r"zero_event_waits=(\d+) turn_errors=(\d+) completions=(\d+)"
)


class Handler(BaseHTTPRequestHandler):
    requests = 0

    def do_GET(self):  # noqa: N802 (http.server API)
        Handler.requests += 1
        payload = b"p0 fetch"
        self.send_response(200)
        self.send_header("Content-Length", str(len(payload)))
        self.send_header("Connection", "close")
        self.end_headers()
        self.wfile.write(payload)

    def log_message(self, *args):
        pass


def websocket_server(listener, sessions, errors):
    try:
        while True:
            try:
                conn, _ = listener.accept()
            except OSError:
                return
            with conn:
                conn.settimeout(20)
                request = b""
                while b"\r\n\r\n" not in request:
                    chunk = conn.recv(4096)
                    if not chunk:
                        break
                    request += chunk
                key = re.search(rb"(?im)^sec-websocket-key:\s*(\S+)", request).group(1)
                accept = base64.b64encode(
                    hashlib.sha1(key + b"258EAFA5-E914-47DA-95CA-C5AB0DC85B11").digest()
                )
                conn.sendall(
                    b"HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\n"
                    b"Connection: Upgrade\r\nSec-WebSocket-Accept: " + accept + b"\r\n\r\n"
                )
                payload = b"p0 websocket"
                conn.sendall(bytes([0x81, len(payload)]) + payload)
                sessions.append(True)
                try:
                    frame = conn.recv(4096)
                    if frame and frame[0] & 0x0F == 0x8:
                        conn.sendall(b"\x88\x00")
                except OSError:
                    pass
    except BaseException as error:  # surfaced by the main thread
        errors.append(repr(error))


def run_probe(perry, env, folder, name, source, expected):
    path = folder / f"{name}.ts"
    binary = folder / name
    path.write_text(source)
    compiled = subprocess.run(
        [str(perry), str(path), "--no-cache", "-o", str(binary)],
        env=env, capture_output=True, text=True, timeout=600,
    )
    if compiled.returncode != 0:
        return [f"{name}: compile failed\n{compiled.stdout}{compiled.stderr}"]
    oracle = subprocess.run(
        ["node", "--experimental-strip-types", str(path)],
        capture_output=True, text=True, timeout=60,
    ).stdout
    run = subprocess.run(
        [str(binary)], env=dict(env, PERRY_LOOP_STATS="1"),
        capture_output=True, text=True, timeout=60,
    )
    problems = []
    if run.returncode != 0:
        problems.append(f"exit {run.returncode}: {run.stderr!r}")
    if not (run.stdout == oracle == expected):
        problems.append(f"stdout {run.stdout!r}, node {oracle!r}, expected {expected!r}")
    found = STATS.findall(run.stderr)
    if len(found) != 1:
        problems.append(f"no turnloop stats line: {run.stderr!r}")
        line = "no stats"
    else:
        turns, os_waits, zero, errors, completions = map(int, found[0])
        line = (f"turns={turns} os_waits={os_waits} zero_event_waits={zero} "
                f"turn_errors={errors} completions={completions}")
        if turns == 0 or completions == 0:
            problems.append("the native loop never completed work")
        if errors:
            problems.append("turn errors")
    print(("PASS " if not problems else "FAIL ") + f"{name}: {line}", flush=True)
    return [f"{name}: {p}" for p in problems]


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--perry", default=str(ROOT / "target/perry-dev/perry"))
    parser.add_argument("--runtime-dir")
    args = parser.parse_args()
    perry = Path(args.perry).resolve()
    runtime_dir = Path(args.runtime_dir).resolve() if args.runtime_dir else perry.parent
    env = dict(os.environ, PERRY_RUNTIME_DIR=str(runtime_dir), PERRY_NO_AUTO_OPTIMIZE="1")
    env.pop("PERRY_LOOP_STATS", None)
    failures = []
    with tempfile.TemporaryDirectory(prefix="perry-turnloop-p0-native-") as directory:
        folder = Path(directory)

        server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
        thread = threading.Thread(target=server.serve_forever, daemon=True)
        thread.start()
        try:
            port = server.server_port
            failures += run_probe(
                perry, env, folder, "fetch",
                f'async function main() {{\n'
                f'  const response = await fetch("http://127.0.0.1:{port}/");\n'
                f'  console.log(await response.text());\n'
                f'  await new Promise((resolve) => setTimeout(resolve, 20));\n'
                f'  console.log("timer after fetch");\n'
                f'}}\nmain();\n',
                "p0 fetch\ntimer after fetch\n",
            )
        finally:
            server.shutdown()
            server.server_close()
            thread.join()
        if Handler.requests != 2:
            failures.append(f"fetch: server saw {Handler.requests} requests, expected 2 (node + perry)")

        listener = socket.socket()
        listener.bind(("127.0.0.1", 0))
        listener.listen()
        sessions, errors = [], []
        ws_thread = threading.Thread(
            target=websocket_server, args=(listener, sessions, errors), daemon=True
        )
        ws_thread.start()
        port = listener.getsockname()[1]
        try:
            failures += run_probe(
                perry, env, folder, "websocket",
                f'const ws = new WebSocket("ws://127.0.0.1:{port}/");\n'
                'ws.onmessage = (event) => { console.log(event.data); ws.close(); };\n',
                "p0 websocket\n",
            )
        finally:
            listener.close()
            ws_thread.join(timeout=5)
        if errors:
            failures.append(f"websocket server: {errors}")
        if len(sessions) != 2:
            failures.append(f"websocket: server completed {len(sessions)} handshakes, expected 2")

    for failure in failures:
        print(failure, file=sys.stderr)
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())
