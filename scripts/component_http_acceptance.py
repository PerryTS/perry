#!/usr/bin/env python3
"""Exercise Perry's HTTP Promise bridge against a canonical WASM component.

Requires matching compiler/runtime/stdlib/component-engine archives and their
GC-effect sidecars. This qualifies the native TypeScript host; Perry-authored
HTTP guests have a separate acceptance boundary.
"""
import argparse
import http.client
import json
import os
from pathlib import Path
import queue
import re
import shutil
import socket
import ssl
import subprocess
import sys
import threading
import time


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ["perry", "runtime-dir", "wit", "wat-encoder", "output"]:
        parser.add_argument("--" + name, required=True, type=Path)
    parser.add_argument("--moving-gc", action="store_true")
    args = parser.parse_args()
    root = Path(__file__).resolve().parent.parent
    out = args.output.resolve()
    out.mkdir(parents=True, exist_ok=True)
    for name in ["host.ts", "world.wit"]:
        shutil.copyfile(root / "test-wasi/components/http" / name, out / name)
    env = os.environ.copy()
    env.update(PERRY_RUNTIME_DIR=str(args.runtime_dir.resolve()),
               PERRY_NO_AUTO_OPTIMIZE="1", PERRY_CODEGEN_CPU_SLOTS="4")
    results = []

    def record(name, status, started, **details):
        results.append(dict(name=name, exitCode=status,
                            seconds=round(time.monotonic() - started, 3), **details))
        (out / "results.json").write_text(json.dumps(results, indent=2) + "\n")
        print(f"{'PASS' if status == 0 else 'FAIL'} {name}", flush=True)
        if status:
            raise SystemExit(1)

    def run(name, command, run_env):
        started = time.monotonic()
        with (out / (name + ".log")).open("w") as log:
            try:
                status = subprocess.run([str(x) for x in command], cwd=out,
                                        env=run_env, stdout=log, stderr=subprocess.STDOUT,
                                        timeout=600).returncode
            except (OSError, subprocess.TimeoutExpired) as error:
                log.write(str(error) + "\n")
                status = 124
        record(name, status, started)

    run("bindings", [args.wit.resolve(), out / "world.wit", out / "generated"], env)
    component = out / "incoming.component.wasm"
    run("component", [args.wat_encoder.resolve(),
        root / "crates/perry-wasi/src/http/p2/incoming.component.wat", component], env)
    certificates = root / "crates/perry-tls-session/tests"
    expected_names = ["success", "explicit-error", "reused", "retire-1", "retire-2", "retire-4"]

    def exchange(executable, run_env, tls, moving):
        name = ("tls" if tls else "plain") + ("-moving" if moving else "")
        started = time.monotonic()
        lines = queue.Queue()
        replies = []
        failure = None
        with (out / (name + "-stderr.log")).open("w") as errors, \
                (out / (name + ".log")).open("w") as log:
            command = [str(executable), str(component), "tls" if tls else "plain",
                       str(certificates / "test-cert.pem"), str(certificates / "test-key.pem")]
            process = subprocess.Popen(command, cwd=out, env=run_env, text=True,
                                       stdout=subprocess.PIPE, stderr=errors, bufsize=1)

            def consume():
                try:
                    for line in process.stdout:
                        lines.put(line)
                finally:
                    lines.put(None)

            reader = threading.Thread(target=consume, daemon=True)
            reader.start()
            deadline = started + 60
            final = False
            completed = []
            try:
                while True:
                    remaining = deadline - time.monotonic()
                    if remaining <= 0:
                        raise TimeoutError("HTTP Promise/event-pump watchdog")
                    line = lines.get(timeout=remaining)
                    if line is None:
                        break
                    log.write(line)
                    log.flush()
                    if line.startswith("READY "):
                        _, case, address = line.strip().split()
                        if len(replies) >= len(expected_names) or case != expected_names[len(replies)]:
                            raise AssertionError("unexpected request order: " + case)
                        host, port = address.rsplit(":", 1)
                        connection = socket.create_connection((host, int(port)), timeout=10)
                        if tls:
                            config = ssl.create_default_context(cafile=str(certificates / "test-ca.pem"))
                            try:
                                connection = config.wrap_socket(connection, server_hostname="localhost")
                            except BaseException:
                                connection.close()
                                raise
                        with connection:
                            connection.sendall(b"POST / HTTP/1.1\r\nHost: localhost\r\nContent-Length: 4\r\n\r\npong")
                            response = http.client.HTTPResponse(connection)
                            response.begin()
                            body = response.read()
                            expected_status = 202 if case in ("success", "reused") else 500
                            if response.status != expected_status or (expected_status == 202 and body != b"pong"):
                                raise AssertionError(f"{case}: {response.status}, body={body!r}")
                            replies.append(dict(case=case, status=response.status, body=body.decode("utf8")))
                    elif line.startswith("RESULT "):
                        completed.append(line.strip().split()[1])
                    elif line.startswith("PASS component HTTP:"):
                        final = True
                status = process.wait(timeout=max(0.1, deadline - time.monotonic()))
                if status != 0 or not final or completed != expected_names or len(replies) != 6:
                    raise AssertionError(f"exit={status}, final={final}, completed={completed}")
            except (OSError, ValueError, AssertionError, TimeoutError, queue.Empty,
                    subprocess.TimeoutExpired, http.client.HTTPException) as error:
                failure = str(error) or "HTTP watchdog expired"
                log.write("FAIL: " + failure + "\n")
                if process.poll() is None:
                    process.kill()
                process.wait()
                status = 1
            finally:
                process.stdout.close()
                reader.join(timeout=1)
        diagnostics = (out / (name + "-stderr.log")).read_text()
        copied = sum(int(n) for n in re.findall(r"\[gc-copy-minor\][^\n]*\bcopied_objects=(\d+)", diagnostics))
        if moving and (copied == 0 or "[gc-fromspace-protect]" not in diagnostics or "evacuation_ok" not in diagnostics):
            status = 1
            failure = "moving GC, quarantine and evacuation verification witnesses required"
        record(name, status, started, responses=replies, copiedObjects=copied, failure=failure)

    for moving in ([False, True] if args.moving_gc else [False]):
        run_env = env.copy()
        if moving:
            for key in ["PERRY_GC_FORCE_EVACUATE", "PERRY_GC_VERIFY_EVACUATION",
                        "PERRY_GC_PROTECT_FROMSPACE", "PERRY_GC_DIAG", "PERRY_GC_INSTRUMENTS"]:
                run_env[key] = "1"
        executable = out / (("host-moving" if moving else "host") + (".exe" if sys.platform == "win32" else ""))
        run("compile-" + ("moving" if moving else "ordinary"),
            [args.perry.resolve(), "compile", out / "host.ts", "--no-auto-optimize",
             "--no-cache", "-o", executable], run_env)
        for tls in [False, True]:
            exchange(executable, run_env, tls, moving)


if __name__ == "__main__":
    main()
