#!/usr/bin/env python3
"""Run a Perry-authored WASI HTTP component with real peers and GC witnesses.

The component must be built from test-wasi/components/http-guest using matching
Perry compiler and WASI runtime archives. Native archives must also match the
compiler. Results distinguish guest moving GC from native moving GC.
"""
import argparse
import asyncio
import hashlib
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import os
from pathlib import Path
import queue
import re
import shutil
import ssl
import subprocess
import threading
import time

LENGTHS = [0, 7, 3145865, 17]
CASES = ["success", "large", "explicit-error", "reused", "retire-1", "retire-2", "retire-4"]


def payload(length):
    return (bytes((i * 31 + 7) % 256 for i in range(256)) * ((length + 255) // 256))[:length]


async def incoming(address, case, tls, certificates):
    host, port = address.rsplit(":", 1)
    config = ssl.create_default_context(cafile=str(certificates / "test-ca.pem")) if tls else None
    reader, writer = await asyncio.open_connection(host, int(port), ssl=config,
                                                    server_hostname="localhost" if tls else None)
    body = payload(3145865 if case == "large" else 7)

    async def send():
        writer.write(f"POST /echo HTTP/1.1\r\nHost: localhost\r\nContent-Length: {len(body)}\r\n\r\n".encode())
        for offset in range(0, len(body), 16384):
            writer.write(body[offset:offset + 16384])
            await writer.drain()

    async def receive():
        head = await reader.readuntil(b"\r\n\r\n")
        lines = head.decode("ascii").split("\r\n")
        status = int(lines[0].split()[1])
        headers = dict(line.lower().split(":", 1) for line in lines[1:] if line)
        output = bytearray()
        if headers.get("transfer-encoding", "").strip() == "chunked":
            while True:
                length = int((await reader.readline()).split(b";", 1)[0], 16)
                if not length:
                    while True:
                        trailer = await reader.readline()
                        if trailer == b"\r\n":
                            break
                        assert trailer, "truncated response trailers"
                    break
                output.extend(await reader.readexactly(length))
                assert await reader.readexactly(2) == b"\r\n"
        elif "content-length" in headers:
            output.extend(await reader.readexactly(int(headers["content-length"])))
        else:
            output.extend(await reader.read())
        expected = 202 if case in ["success", "large", "reused"] else 500
        assert status == expected, (case, status, bytes(output[:200]))
        if status == 202:
            assert output == body, (case, len(output), len(body))
        return dict(case=case, status=status, bytes=len(output), sha256=hashlib.sha256(output).hexdigest())

    try:
        _, result = await asyncio.wait_for(asyncio.gather(send(), receive()), timeout=45)
        return result
    finally:
        writer.close()
        await writer.wait_closed()


class Echo(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def log_message(self, *args):
        pass

    def do_POST(self):
        try:
            assert self.path == "/echo"
            self.connection.settimeout(30)
            body = bytearray()
            if self.headers.get("Transfer-Encoding", "").lower() == "chunked":
                while True:
                    length = int(self.rfile.readline().split(b";", 1)[0], 16)
                    if not length:
                        while True:
                            trailer = self.rfile.readline()
                            if trailer == b"\r\n":
                                break
                            assert trailer, "truncated request trailers"
                        break
                    chunk = self.rfile.read(length)
                    assert len(chunk) == length
                    body.extend(chunk)
                    assert self.rfile.read(2) == b"\r\n"
            else:
                length = int(self.headers.get("Content-Length", "0"))
                body.extend(self.rfile.read(length))
                assert len(body) == length
            assert body == payload(len(body)), "unexpected or unauthorized outgoing payload"
            self.server.receipts.put(dict(bytes=len(body), sha256=hashlib.sha256(body).hexdigest()))
            self.send_response(200)
            self.send_header("Content-Length", str(len(body)))
            self.send_header("Connection", "close")
            self.end_headers()
            self.wfile.write(body)
            self.close_connection = True
        except Exception as error:
            self.server.errors.put(repr(error))
            self.close_connection = True


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ["perry", "runtime-dir", "wit", "component", "output", "tsc"]:
        parser.add_argument("--" + name, type=Path, required=True)
    parser.add_argument("--moving-gc", action="store_true")
    args = parser.parse_args()
    root = Path(__file__).resolve().parent.parent
    out = args.output.resolve()
    out.mkdir(parents=True, exist_ok=False)
    fixture = root / "test-wasi/components/http-guest"
    shutil.copytree(fixture, out / "fixture")
    env = os.environ.copy()
    env.update(PERRY_RUNTIME_DIR=str(args.runtime_dir.resolve()), PERRY_NO_AUTO_OPTIMIZE="1",
               PERRY_CODEGEN_CPU_SLOTS="4")
    assert not any(key.startswith("PERRY_GC_") for key in env), "set GC modes through this harness"
    results = []

    def record(name, code, started, **details):
        results.append(dict(name=name, exitCode=code, seconds=round(time.monotonic() - started, 3), **details))
        (out / "results.json").write_text(json.dumps(results, indent=2) + "\n")
        print(("PASS " if code == 0 else "FAIL ") + name, flush=True)
        if code:
            raise SystemExit(1)

    def run(name, command, run_env):
        started = time.monotonic()
        with (out / (name + ".log")).open("w") as log:
            try:
                code = subprocess.run(list(map(str, command)), cwd=out, env=run_env,
                                      stdout=log, stderr=subprocess.STDOUT, timeout=600).returncode
            except (OSError, subprocess.TimeoutExpired) as error:
                log.write(str(error) + "\n")
                code = 124
        record(name, code, started)

    run("bindings", [args.wit.resolve(), out / "fixture", out / "fixture/generated", "plugin"], env)
    config = {
        "compilerOptions": {
            "strict": True, "noEmit": True, "target": "ES2022", "module": "ESNext",
            "moduleResolution": "Bundler", "baseUrl": str(root),
            "paths": {"perry/*": ["types/perry/*"]},
            "typeRoots": [str(args.tsc.resolve().parents[2] / "@types")],
        },
        "files": [str(out / "fixture/guest.ts"), str(out / "fixture/host.ts")],
    }
    (out / "tsconfig.json").write_text(json.dumps(config, indent=2) + "\n")
    run("typecheck", ["node", args.tsc.resolve(), "--project", out / "tsconfig.json"], env)
    certificates = root / "crates/perry-tls-session/tests"
    for native in ([False, True] if args.moving_gc else [False]):
        run_env = env.copy()
        if native:
            for key in ["PERRY_GC_FORCE_EVACUATE", "PERRY_GC_VERIFY_EVACUATION", "PERRY_GC_DIAG",
                        "PERRY_GC_PROTECT_FROMSPACE", "PERRY_GC_INSTRUMENTS"]:
                run_env[key] = "1"
        executable = out / ("host-moving" if native else "host")
        run("compile-" + executable.name, [args.perry.resolve(), "compile", out / "fixture/host.ts",
            "--no-cache", "--no-auto-optimize", "-o", executable], run_env)
        for guest in ([False, True] if args.moving_gc else [False]):
            for tls in [False, True]:
                name = f"{'tls' if tls else 'plain'}-native{int(native)}-guest{int(guest)}"
                started = time.monotonic()
                backend = ThreadingHTTPServer(("127.0.0.1", 0), Echo)
                backend.receipts = queue.Queue()
                backend.errors = queue.Queue()
                thread = threading.Thread(target=backend.serve_forever, daemon=True)
                thread.start()
                lines = queue.Queue()
                replies, completed, outgoing = [], [], []
                failure = None
                code = 0
                with (out / (name + ".log")).open("w") as log, (out / (name + "-stderr.log")).open("w") as errors:
                    process = subprocess.Popen([str(executable), str(args.component.resolve()),
                        "tls" if tls else "plain", str(certificates / "test-cert.pem"),
                        str(certificates / "test-key.pem"), str(backend.server_port), "moving" if guest else "ordinary"],
                        cwd=out, env=run_env, stdout=subprocess.PIPE, stderr=errors, text=True, bufsize=1)

                    def consume():
                        for line in process.stdout:
                            lines.put(line)
                        lines.put(None)

                    reader = threading.Thread(target=consume, daemon=True)
                    reader.start()
                    final, denied = False, False
                    # Each case creates independent capability-limited stores;
                    # compilation and collection stress dominate on slow hosts.
                    deadline = started + 600
                    try:
                        while True:
                            line = lines.get(timeout=max(0.1, deadline - time.monotonic()))
                            if line is None:
                                break
                            log.write(line)
                            log.flush()
                            if line.startswith("READY "):
                                _, case, address = line.strip().split()
                                assert case == CASES[len(replies)], case
                                replies.append(asyncio.run(incoming(address, case, tls, certificates)))
                            elif line.startswith("RESULT "):
                                completed.append(line.strip().split()[1])
                            elif line.startswith("OUTGOING "):
                                outgoing.append(int(line.strip().split()[1]))
                            elif line.strip() == "DENIED 11":
                                denied = True
                            elif line.strip() == "PASS Perry HTTP guest":
                                final = True
                        code = process.wait(timeout=max(0.1, deadline - time.monotonic()))
                        assert code == 0 and final and denied, (code, final, denied)
                        assert completed == CASES and len(replies) == len(CASES), completed
                        assert outgoing == LENGTHS, outgoing
                    except Exception as error:
                        failure = repr(error)
                        code = 1
                        log.write("FAIL " + failure + "\n")
                    finally:
                        if process.poll() is None:
                            process.kill()
                        process.wait()
                        process.stdout.close()
                        reader.join(timeout=1)
                        backend.shutdown()
                        backend.server_close()
                        thread.join(timeout=1)
                receipts = list(backend.receipts.queue)
                backend_errors = list(backend.errors.queue)
                if [item["bytes"] for item in receipts] != LENGTHS or backend_errors:
                    code = 1
                    failure = failure or "backend receipts mismatch or unauthorized requests"
                diagnostics = (out / (name + "-stderr.log")).read_text()
                scopes = re.findall(r"HTTP-GUEST-GC-START\n(.*?)HTTP-GUEST-GC-END", diagnostics, re.S)
                guest_diag = "\n".join(scopes)
                native_diag = re.sub(r"HTTP-GUEST-GC-START\n.*?HTTP-GUEST-GC-END", "", diagnostics, flags=re.S)

                def copied(text):
                    return sum(map(int, re.findall(r"\[gc-copy-minor\][^\n]*\bcopied_objects=(\d+)", text)))

                guest_copies, native_copies = copied(guest_diag), copied(native_diag)
                if guest and (guest_copies == 0 or "evacuation_ok" not in guest_diag):
                    code, failure = 1, failure or "guest moving GC witness missing"
                if native and (native_copies == 0 or "evacuation_ok" not in native_diag or "[gc-fromspace-protect]" not in native_diag):
                    code, failure = 1, failure or "native moving GC/quarantine witness missing"
                record(name, code, started, incoming=replies, outgoing=receipts, backendErrors=backend_errors,
                       guestCollections=len(scopes), guestCopiedObjects=guest_copies,
                       outsideGuestCollectionCopiedObjects=native_copies, failure=failure)


if __name__ == "__main__":
    main()
