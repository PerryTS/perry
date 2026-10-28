#!/usr/bin/env python3
"""Qualify wasm32-wasip1 programs under the reference host (Unix only).

Every fixture in cases/ is compiled with `--target wasip1`, must import nothing
but `wasi_snapshot_preview1` (a case built with `PERRY_WASM_NET=1` may also
import `perry_net`, and must; one built with `PERRY_WASM_DISCARD=1` may also
import `perry_host.discard_pages`, and must), and runs under the host in this directory with a
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
import re
import socket
import subprocess
import sys
import threading
import time
import traceback
from dataclasses import dataclass, field
from pathlib import Path
from typing import Callable

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE.parent.parent / "scripts"))
import wasi_imports_check  # noqa: E402
import wasi_run  # noqa: E402

TIMEOUT = 120


class Run:
    """The host process: guest stdout as lines, the addresses it listens on."""

    def __init__(self, host: Path, wasm: Path, listen: int, signals: list[str], stats: Path,
                 listen_at: int | None = None, announce: bool = True,
                 dns: list[str] = (), peer_port: int = 0, peer: socket.socket | None = None,
                 host_args: list[str] = ()):
        command = [str(host), "--stats", str(stats), *host_args]
        for _ in range(listen):
            command += ["--listen", f"{listen_at}=127.0.0.1:0" if listen_at else "127.0.0.1:0"]
        if not announce:
            command.append("--no-announce")
        for signal in signals:
            command += ["--signal", signal]
        for rule in dns:
            command += ["--dns", rule]
        # The program dials this port; it is the listening peer, or a closed port.
        self.peer = peer
        if peer_port:
            command += ["--env", f"PEER_PORT={peer_port}"]
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

    def serve_once(self, reply: bytes = b"") -> bytes:
        """Accept the program's one outbound connection, answer it, and return
        what it sent up to its end of stream."""
        self.peer.settimeout(TIMEOUT)
        conn, _ = self.peer.accept()
        conn.settimeout(TIMEOUT)
        try:
            if reply:
                # Wait for the request first, so the reply answers something.
                got = conn.recv(4096)
                conn.sendall(reply)
            else:
                got = b""
            while chunk := conn.recv(4096):
                got += chunk
            return got
        finally:
            conn.close()

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
    # Built with PERRY_WASM_NET=1: the module imports perry_net.
    net: bool = False
    dns: list[str] = field(default_factory=list)
    # The program dials PEER_PORT: "listen" is a peer the case serves, "closed"
    # a port nothing listens on.
    peer: str = ""
    # The fixture to compile when it is not the case's own name.
    source: str = ""
    # Built with PERRY_WASM_DISCARD=1: the module imports perry_host.discard_pages.
    discard: bool = False
    # Extra options for the host (`--no-discard` answers 0 to discard_pages).
    host_args: list[str] = field(default_factory=list)
    # Checks the guest's stderr.
    log: Callable[[str], None] | None = None


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


def attempts(n: int) -> Callable[[dict], None]:
    return lambda s: check(s["connect_attempts"] == n, f"{s['connect_attempts']} connect attempts, expected {n}")


@case("net_dns_delayed", net=True, listen=1, peer="listen", dns=["slow.test=127.0.0.1,delay=400"],
      stdout="listening\ndialing\nconnected ticks ok\nclosed\nclient closed\n",
      stats=attempts(1))
def net_dns_delayed(run: Run) -> None:
    # A name whose lookup takes 400 ms: the loop keeps ticking meanwhile (the
    # program counts its 25 ms timer) and an existing client is still served.
    run.expect("listening")
    a = run.connect()
    run.expect("dialing")
    began = time.monotonic()
    echo_once(a, b"during the lookup")
    assert time.monotonic() - began < 0.3, "the echo waited for the lookup"
    assert run.serve_once() == b"bye"
    run.expect("closed")
    hang_up(a)


@case("net_connect_roundtrip", net=True, peer="listen", dns=["echo.test=127.0.0.1,delay=300"],
      stdout="got pong\nclosed\n",
      stats=lambda s: (attempts(1)(s),
                       # The program has no timers: during the 300 ms lookup it sleeps in
                       # poll_oneoff instead of asking after the pending connection again and again.
                       check(s["polls"] >= 1 and s["sock_errors"] <= 10,
                             f"{s['polls']} polls, {s['sock_errors']} sock_error calls during a delayed lookup")))
def net_connect_roundtrip(run: Run) -> None:
    assert run.serve_once(b"pong") == b"ping"


@case("net_destroy_resolving", net=True, peer="listen", dns=["slow.test=127.0.0.1,delay=300"],
      stdout="first closed\nliteral connected\ngot pong after the lookup's deadline\nliteral closed\n",
      stats=lambda s: (attempts(1)(s),
                       check(len(s["connects"]) == 2 and s["connects"][0] == s["connects"][1],
                             f"the freed descriptor was not reused: {s['connects']}")))
def net_destroy_resolving(run: Run) -> None:
    # Only the literal reaches the network: the destroyed lookup never connects,
    # not even when its delay runs out while the replacement is in use.
    assert run.serve_once(b"pong") == b"ping"


add("net_notfound", net=True, dns=["gone.test=notfound,delay=200"],
    stdout="ENOTFOUND getaddrinfo after delay\n", stats=attempts(0))


add("net_refused", net=True, peer="closed", dns=["refuse.test=127.0.0.1,delay=100"],
    stdout="refuse.test ECONNREFUSED connect\n127.0.0.1 ECONNREFUSED connect\n", stats=attempts(2))


add("net_dns_lookup", net=True, dns=["lookup.test=127.0.0.5,delay=300"],
    stdout="null 127.0.0.5 4 after delay\nENOTFOUND\n")


# 70 addresses: more than the 64 records the runtime's first buffer holds.
add("net_dns_many", net=True, dns=["many.test=" + "+".join(f"10.0.0.{n}" for n in range(1, 71))],
    stdout="null 70 10.0.0.1 10.0.0.70\n")


# listen() after an outbound connect: the connection is not mistaken for the
# listener (no announce, so the runtime scans for stream sockets).
@case("net_before_listen", net=True, peer="listen", listen=1, listen_at=12, announce=False,
      stdout="connected\nlistening\ngot pong\nclosed\n",
      stats=lambda s: (attempts(1)(s),
                       check(len(s["accepts"]) == 1 and s["connects"] == [3],
                             f"accepts {s['accepts']}, connects {s['connects']}")))
def net_before_listen(run: Run) -> None:
    run.expect("listening")
    sock = run.connect()
    echo_once(sock, b"gap")
    hang_up(sock)
    assert run.serve_once(b"pong") == b"ping"


# One fixture, built both ways: the default build imports only preview 1 and
# outbound is not supported; PERRY_WASM_NET=1 adds perry_net and it connects.
@case("net_both_ways", net=True, peer="listen", stdout="connected\n", stats=attempts(1))
def net_both_ways(run: Run) -> None:
    run.serve_once()


add("net_both_ways_default", source="net_both_ways", peer="closed", stdout="error ENOTSUP\n", stats=attempts(0))


# perry_host.discard_pages (PERRY_WASM_DISCARD=1). The first fixture shrinks a
# resizable ArrayBuffer by 1 MiB six times, one host call each; it runs three
# ways: the host discards, the host answers 0 (the runtime must stop asking after
# that one call), and a build without the import (the host sees no call and the
# module imports no perry_host).
RESIZE_ROUNDS = 6


def discarded(s: dict) -> None:
    check(s["discard_calls"] >= RESIZE_ROUNDS and s["discard_bytes"] >= RESIZE_ROUNDS * 900_000,
          f"discard calls {s['discard_calls']}, bytes {s['discard_bytes']}")


add("discard_resizable", discard=True, stdout="intact 1048576\n", stats=discarded)
add("discard_declined", source="discard_resizable", discard=True, host_args=["--no-discard"],
    stdout="intact 1048576\n",
    stats=lambda s: check(s["discard_calls"] == 1 and s["discard_bytes"] == 0,
                          f"after a 0 the runtime asked again: calls {s['discard_calls']}, bytes {s['discard_bytes']}"))
add("discard_default", source="discard_resizable", stdout="intact 1048576\n",
    stats=lambda s: check(s["discard_calls"] == 0, f"discard calls {s['discard_calls']}"))
# Freed blocks come back from the keep list or from malloc; what was written
# into them reads back unchanged, zero-initialised storage allocated after a
# collection reads as zero, and some blocks were given to the host. The GC log
# (PERRY_GC_LOG) carries the allocator's counters: at least one held-back block
# must have been handed out to a zeroed allocation, or the zero check proved
# nothing.
def zeroed_reuse(log: str) -> None:
    counts = [int(m) for m in re.findall(r"zeroed_reused=(\d+)", log)]
    check(counts and max(counts) >= 1, f"no held-back block was reused for zeroed storage: {counts}")


add("discard_reuse", discard=True, stdout="intact\n", host_args=["--env", "PERRY_GC_LOG=1"],
    stats=lambda s: check(s["discard_calls"] >= 1 and s["discard_bytes"] >= 1 << 20,
                          f"discard calls {s['discard_calls']}, bytes {s['discard_bytes']}"),
    log=zeroed_reuse)


def compile_case(perry: Path, source: Path, wasm: Path, log: Path, net: bool,
                 discard: bool = False) -> str | None:
    env = dict(os.environ, PERRY_NO_AUTO_OPTIMIZE="1", PERRY_NO_CACHE="1")
    env.pop("PERRY_WASM_NET", None)
    env.pop("PERRY_WASM_DISCARD", None)
    if net:
        env["PERRY_WASM_NET"] = "1"
    if discard:
        env["PERRY_WASM_DISCARD"] = "1"
    done = subprocess.run(
        [str(perry), "compile", str(source), "--target", "wasip1", "-o", str(wasm)],
        env=env, capture_output=True, timeout=600)
    log.write_bytes(done.stdout + done.stderr)
    if done.returncode or not wasm.is_file():
        return "compile failed:\n" + done.stderr.decode(errors="replace")[-3000:]
    ok, report = wasi_run.check_imports(wasm, allow_perry_net=net, allow_perry_host=discard)
    if not ok:
        return "imports check failed:\n" + report[-3000:]
    imported = {module for module, *_ in wasi_imports_check.module_imports(wasm.read_bytes())}
    if net != (wasi_imports_check.PERRY_NET in imported):
        return f"PERRY_WASM_NET={'1' if net else 'unset'} but the module imports {sorted(imported)}"
    if discard != (wasi_imports_check.PERRY_HOST in imported):
        return f"PERRY_WASM_DISCARD={'1' if discard else 'unset'} but the module imports {sorted(imported)}"
    return None


def run_case(c: Case, host: Path, wasm: Path, out: Path) -> str | None:
    stats_path = out / f"{c.name}.stats.json"
    stats_path.unlink(missing_ok=True)
    peer, peer_port = None, 0
    if c.peer:
        peer = socket.socket()
        peer.bind(("127.0.0.1", 0))
        peer_port = peer.getsockname()[1]
        if c.peer == "listen":
            peer.listen(4)
        else:
            peer.close()  # the port stays unbound: connecting to it is refused
            peer = None
    run = Run(host, wasm, c.listen, c.signals, stats_path, c.listen_at, c.announce,
              c.dns, peer_port, peer, c.host_args)
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
        if c.log:
            c.log("".join(run.stderr))
        return None
    except AssertionError as exc:
        return f"{exc}\nstdout: {''.join(run.stdout)!r}\nstderr: {''.join(run.stderr)[-2000:]}"
    finally:
        run.abort()
        if peer is not None:
            peer.close()


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
    if not (listed == fixtures == {c.source or c.name for c in CASES.values()}):
        print(f"case list, fixtures and scenarios disagree: cases.txt={sorted(listed)} "
              f"cases/={sorted(fixtures)} suite.py={sorted({c.source or c.name for c in CASES.values()})}")
        return 1

    selected = [c for name, c in sorted(CASES.items()) if not args.filter or any(f in name for f in args.filter)]
    failures, ran = [], 0
    for c in selected:
        wasm = args.output / f"{c.name}.wasm"
        try:
            problem = compile_case(args.perry, HERE / "cases" / f"{c.source or c.name}.ts", wasm,
                                   args.output / f"{c.name}.compile.log", c.net, c.discard)
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
