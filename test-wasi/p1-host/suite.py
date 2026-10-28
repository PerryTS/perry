#!/usr/bin/env python3
"""Qualify wasm32-wasip1 programs under the reference host (Unix only).

Every fixture in cases/ is compiled with `--target wasip1`, must import nothing
but `wasi_snapshot_preview1`, and runs under the host in this directory with a
client (or a signal schedule) driving it from outside. A case passes when the
program's stdout and exit status match and the host's observations (poll
counts, descriptor numbers) satisfy the case's check.

    python3 test-wasi/p1-host/suite.py target/perry-dev/perry \\
        --host test-wasi/p1-host/target/debug/perry-p1-host \\
        --output target/wasi-validation/p1-host --min-cases 10
"""

from __future__ import annotations

import argparse
import http.client
import json
import os
import queue
import socket
import subprocess
import sys
import threading
import traceback
from dataclasses import dataclass, field
from pathlib import Path
from typing import Callable

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE.parent.parent / "scripts"))
import wasi_run  # noqa: E402

TIMEOUT = 120


class Run:
    """The host process: guest stdout as lines, the addresses it listens on."""

    def __init__(self, host: Path, wasm: Path, listen: int, signals: list[str], stats: Path,
                 listen_at: int | None = None, announce: bool = True):
        command = [str(host), "--stats", str(stats)]
        for _ in range(listen):
            command += ["--listen", f"{listen_at}=127.0.0.1:0" if listen_at else "127.0.0.1:0"]
        if not announce:
            command.append("--no-announce")
        for signal in signals:
            command += ["--signal", signal]
        self.process = subprocess.Popen(
            command + [str(wasm)], stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
        # The host announces every listener before the guest starts.
        self.addrs = [self.process.stderr.readline().split()[-1] for _ in range(listen)]
        self.lines: queue.Queue[str | None] = queue.Queue()
        self.stdout: list[str] = []
        self.stderr: list[str] = []
        self.threads = [
            threading.Thread(target=self._pump, args=(self.process.stdout, self.stdout, self.lines)),
            threading.Thread(target=self._pump, args=(self.process.stderr, self.stderr, None)),
        ]
        for thread in self.threads:
            thread.start()

    @staticmethod
    def _pump(stream, sink: list[str], lines) -> None:
        for line in stream:
            sink.append(line)
            if lines is not None:
                lines.put(line.rstrip("\n"))
        if lines is not None:
            lines.put(None)

    def expect(self, line: str) -> None:
        """Wait for the guest to print exactly `line`."""
        while True:
            try:
                got = self.lines.get(timeout=TIMEOUT)
            except queue.Empty:
                raise AssertionError(f"timed out waiting for {line!r}") from None
            if got is None:
                raise AssertionError(f"the guest ended before printing {line!r}")
            if got == line:
                return

    def connect(self, index: int = 0) -> socket.socket:
        host, port = self.addrs[index].rsplit(":", 1)
        return socket.create_connection((host, int(port)), timeout=TIMEOUT)

    def finish(self) -> int:
        try:
            status = self.process.wait(timeout=TIMEOUT)
        except subprocess.TimeoutExpired:
            self.process.kill()
            self.process.wait()
            raise AssertionError("the guest did not exit (a blocked or spinning loop)") from None
        finally:
            for thread in self.threads:
                thread.join(timeout=5)
        return status

    def abort(self) -> None:
        if self.process.poll() is None:
            self.process.kill()
            self.process.wait()


def echo_once(sock: socket.socket, data: bytes) -> None:
    sock.sendall(data)
    got = b""
    while len(got) < len(data):
        chunk = sock.recv(len(data) - len(got))
        assert chunk, f"connection closed after {got!r}"
        got += chunk
    assert got == data, f"echoed {got!r}, sent {data!r}"


def hang_up(sock: socket.socket) -> None:
    """Half-close and wait for the server's own end of stream."""
    sock.shutdown(socket.SHUT_WR)
    assert sock.recv(1) == b"", "the server kept the connection open"
    sock.close()


@dataclass
class Case:
    name: str
    drive: Callable[[Run], None] | None = None
    listen: int = 0
    # Place the listener on this descriptor, and leave it unannounced (the guest scans for it).
    listen_at: int | None = None
    announce: bool = True
    signals: list[str] = field(default_factory=list)
    stdout: Callable[[Run], str] | str = ""
    status: int = 0
    stats: Callable[[dict], None] | None = None


CASES: dict[str, Case] = {}


def add(name: str, **kwargs) -> None:
    CASES[name] = Case(name, **kwargs)


def case(name: str, **kwargs):
    """Register a case whose client side is the decorated function."""
    def register(drive):
        add(name, drive=drive, **kwargs)
        return drive
    return register


def check(ok: bool, detail: str) -> None:
    assert ok, detail


@case("echo", listen=1, stdout="listening\nclosed\nclosed\n",
      stats=lambda s: check(len(s["accepts"]) == 2, f"accepts {s['accepts']}"))
def echo(run: Run) -> None:
    run.expect("listening")
    a, b = run.connect(), run.connect()
    echo_once(a, b"alpha")
    echo_once(b, b"beta")
    hang_up(a)
    hang_up(b)


@case("fd_reuse", listen=1, stdout="listening\nclosed 1\nclosed 2\nclosed 3\n",
      stats=lambda s: check(len(s["accepts"]) == 3 and len(set(s["accepts"])) == 1,
                             f"the freed descriptor was not reused: {s['accepts']}"))
def fd_reuse(run: Run) -> None:
    run.expect("listening")
    for n in (1, 2, 3):
        sock = run.connect()
        echo_once(sock, f"round {n}".encode())
        hang_up(sock)
        run.expect(f"closed {n}")


@case("http_server", listen=1, stdout="listening\n")
def http_server(run: Run) -> None:
    run.expect("listening")
    host, port = run.addrs[0].rsplit(":", 1)
    for method, path, body, reply in (
        ("GET", "/hello", None, "GET /hello []"),
        ("POST", "/echo", b"payload", "POST /echo [payload]"),
    ):
        connection = http.client.HTTPConnection(host, int(port), timeout=TIMEOUT)
        connection.request(method, path, body, {"Connection": "close"})
        response = connection.getresponse()
        assert response.status == 200, response.status
        assert response.read().decode() == reply
        connection.close()


add("timers_idle_listener", listen=1,
      stdout="listening\ntick 1\ntick 2\ntick 3\ntick 4\ntick 5\nclosed\n")


add("signals", signals=["80:12", "120:99", "160:10", "200:2"], stdout="usr2\nint\ndone\n",
      stats=lambda s: (
          check(s["polls"] <= 80, f"{s['polls']} polls for a 400 ms run: the loop is not parked"),
          # Zero polls would pass the bound too: the guest must have parked, on both kinds of wake.
          check(s["fd_wakeups"] >= 1 and s["clock_wakeups"] >= 1,
                f"parked wakeups: fd {s['fd_wakeups']}, clock {s['clock_wakeups']}")))


@case("signal_exit", listen=1, signals=["150:15"], stdout="listening\n", status=143)
def signal_exit(run: Run) -> None:
    run.expect("listening")


add("listen_addrs", listen=2, stdout=lambda r: f"first {r.addrs[0]}\nsecond {r.addrs[1]}\n")


@case("stale_listener_address", listen=2,
      stdout="ready\nlocal port is not the closed listener's\n",
      stats=lambda s: check(s["accepts"] == [3], f"the closed listener's number was not reused: {s['accepts']}"))
def stale_listener_address(run: Run) -> None:
    run.expect("ready")
    sock = run.connect(1)
    assert sock.recv(1) == b"", "the server kept the connection open"
    sock.close()


@case("sparse_listener", listen=1, listen_at=12, announce=False, stdout="listening\n",
      stats=lambda s: check(len(s["accepts"]) == 1, f"accepts {s['accepts']}"))
def sparse_listener(run: Run) -> None:
    run.expect("listening")
    sock = run.connect()
    echo_once(sock, b"gap")
    hang_up(sock)


add("listen_exhausted", listen=1, stdout="first listening\nsecond EADDRNOTAVAIL\n")


add("prestat_scan", listen=2, signals=["100:99"], stdout="false\nup\n",
      stats=lambda s: check(1 <= s["prestat_calls"] <= 4, f"{s['prestat_calls']} fd_prestat_get calls"))


@case("mixed", listen=1, signals=["600:12", "2000:15"],
      stdout="listening\nusr2\nconnection closed\nterm\nticks ok\n",
      stats=lambda s: check(s["max_subs"] >= 4,
                             f"no poll had listener, connection, signals and a timer ({s['max_subs']})"))
def mixed(run: Run) -> None:
    run.expect("listening")
    sock = run.connect()
    echo_once(sock, b"mixed")
    run.expect("usr2")
    hang_up(sock)


def compile_case(perry: Path, source: Path, wasm: Path, log: Path) -> str | None:
    env = dict(os.environ, PERRY_NO_AUTO_OPTIMIZE="1", PERRY_NO_CACHE="1")
    done = subprocess.run(
        [str(perry), "compile", str(source), "--target", "wasip1", "-o", str(wasm)],
        env=env, capture_output=True, timeout=600)
    log.write_bytes(done.stdout + done.stderr)
    if done.returncode or not wasm.is_file():
        return "compile failed:\n" + done.stderr.decode(errors="replace")[-3000:]
    ok, report = wasi_run.check_imports(wasm)
    return None if ok else "imports check failed:\n" + report[-3000:]


def run_case(c: Case, host: Path, wasm: Path, out: Path) -> str | None:
    stats_path = out / f"{c.name}.stats.json"
    stats_path.unlink(missing_ok=True)
    run = Run(host, wasm, c.listen, c.signals, stats_path, c.listen_at, c.announce)
    try:
        if c.drive:
            c.drive(run)
        status = run.finish()
        stdout = "".join(run.stdout)
        expected = c.stdout(run) if callable(c.stdout) else c.stdout
        (out / f"{c.name}.stdout").write_text(stdout)
        (out / f"{c.name}.stderr").write_text("".join(run.stderr))
        if stdout != expected:
            return f"stdout {stdout!r}, expected {expected!r}\nstderr: {''.join(run.stderr)[-2000:]}"
        if status != c.status:
            return f"exit status {status}, expected {c.status}\nstderr: {''.join(run.stderr)[-2000:]}"
        stats = json.loads(stats_path.read_text())
        if c.stats:
            c.stats(stats)
        return None
    except AssertionError as exc:
        return f"{exc}\nstdout: {''.join(run.stdout)!r}\nstderr: {''.join(run.stderr)[-2000:]}"
    finally:
        run.abort()


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("perry", type=Path)
    parser.add_argument("--host", type=Path, required=True, help="the built perry-p1-host binary")
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--filter", action="append", default=[], help="case name substring")
    parser.add_argument("--min-cases", type=int, default=0,
                        help="fail unless at least this many cases ran (a gate must prove it is live)")
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=True)

    listed = {line.split("#", 1)[0].strip() for line in (HERE / "cases.txt").read_text().splitlines()}
    listed.discard("")
    fixtures = {p.stem for p in (HERE / "cases").glob("*.ts")}
    if not (listed == fixtures == set(CASES)):
        print(f"case list, fixtures and scenarios disagree: cases.txt={sorted(listed)} "
              f"cases/={sorted(fixtures)} suite.py={sorted(CASES)}")
        return 1

    selected = [c for name, c in sorted(CASES.items()) if not args.filter or any(f in name for f in args.filter)]
    failures, ran = [], 0
    for c in selected:
        wasm = args.output / f"{c.name}.wasm"
        try:
            problem = compile_case(args.perry, HERE / "cases" / f"{c.name}.ts", wasm,
                                   args.output / f"{c.name}.compile.log")
            if problem is None:
                problem = run_case(c, args.host, wasm, args.output)
        except Exception:  # a harness error is a failure, never a skip
            problem = traceback.format_exc()
        ran += 1
        print(f"{'ok  ' if problem is None else 'FAIL'} {c.name}", flush=True)
        if problem:
            failures.append(c.name)
            print("     " + problem.replace("\n", "\n     "), flush=True)
    print(f"{ran} cases ran, {len(failures)} failed")
    if ran < args.min_cases:
        print(f"expected at least {args.min_cases} cases to run")
        return 1
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())
