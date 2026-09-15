#!/usr/bin/env python3
"""turnloop P0 server A/B harness: turnloop arm vs `tokio-wait-driver` arm.

Builds both arms from ONE commit into separate target dirs (prebuilt archives,
PERRY_NO_AUTO_OPTIMIZE=1 — the A/B feature does not survive auto-optimize),
compiles the same node:http server with each, then measures each arm in
interleaved fresh-process rounds:

  * load scenarios at fixed concurrency (default 1, 64, 1024 connections):
    throughput, p50/p99/p999 latency, CPU user/sys, wall, voluntary and
    involuntary context switches, syscalls/s, peak RSS;
  * idle-connection capacity (default 10k and 100k keep-alive connections):
    server RSS before/after, bytes per connection, idle CPU, connections
    still open after the hold;
  * the PERRY_LOOP_STATS wait metrics of every server process (tokio ticks vs
    turnloop turns, time parked per kind, fast drives, wake-latency histogram,
    zero-budget and spin-throttle hits), plus the arm marker line, which
    must match the arm or the sample is rejected.

Output: <work>/results/results.json (every raw sample), summary.json and
summary.md (one comparison table: per scenario and metric, median [min–max]
for each arm and the delta of medians).

Usage (Linux x86_64, e.g. perrymaster):
  scripts/turnloop/server_ab.py all --work /root/turnloop-ab
  scripts/turnloop/server_ab.py build --work DIR [--profile release] [--skip-cargo]
  scripts/turnloop/server_ab.py run --work DIR [--rounds 5] [--concurrency 1,64,1024]
      [--duration 15] [--warmup 3] [--idle 10000,100000] [--idle-hold 10]
      [--load-tool auto|oha|wrk|ab] [--syscalls auto|perf|strace|off]
  scripts/turnloop/server_ab.py report --work DIR
  scripts/turnloop/server_ab.py <any> --dry-run   # macOS-safe: plan + synthetic report

`--skip-cargo` is for a host with room for only one cargo target tree: build each
arm in turn into the same tree, copy `perry` and the five archives out into
`<work>/target-turnloop` and `<work>/target-tokio`, and the build step records
and verifies them without invoking cargo.

Load tools: `oha` preferred, then `wrk` (install instructions are printed when
neither exists). `ab` is accepted only when requested explicitly
(`--load-tool ab`), for smoke runs; it has no p999 and is single-threaded.

Wait metrics cover each server process's lifetime (startup, warmup, the
measured window and shutdown); every load scenario uses its own process.
"""

import argparse
import datetime
import hashlib
import http.client
import json
import os
import platform
import re
import resource
import selectors
import shutil
import signal
import socket
import statistics
import subprocess
import sys
import tempfile
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
APP = ROOT / "scripts/turnloop/apps/node_http_hello.ts"

ARMS = ("turnloop", "tokio")
ARM_FEATURES = {"turnloop": [], "tokio": ["perry-stdlib/tokio-wait-driver"]}
ARM_MARKER = {
    "turnloop": "[perry-loop] driver=turnloop",
    "tokio": "[perry-loop] driver=tokio-wait-driver",
}
ARM_WAITS = {"turnloop": "turnloop", "tokio": "tokio-wait-driver"}
PACKAGES = [
    "perry", "perry-runtime-static", "perry-stdlib-static",
    "perry-ext-http", "perry-ext-net", "perry-ext-ws",
]
FEATURES = ["perry-stdlib/external-http-server-pump", "perry-stdlib/external-http-client-pump"]
ARCHIVES = [
    "libperry_runtime.a", "libperry_stdlib.a",
    "libperry_ext_http.a", "libperry_ext_net.a", "libperry_ext_ws.a",
]
IS_LINUX = sys.platform.startswith("linux")
WAITS_RE = re.compile(r"^\[perry-loop-waits\] (.*)$", re.M)
CLK_TCK = os.sysconf("SC_CLK_TCK") if hasattr(os, "sysconf") else 100

INSTALL_HINTS = """\
No HTTP load generator found. Install one of:
  oha (preferred): cargo install oha --locked     # or: apt install oha / brew install oha
  wrk:             apt install wrk                # or: brew install wrk
Then re-run, or pass --load-tool with an explicit path via --oha/--wrk."""


def log(msg):
    print(f"[server_ab {datetime.datetime.now():%H:%M:%S}] {msg}", flush=True)


# ─── build ──────────────────────────────────────────────────────────────────


def profile_dir(profile):
    return "debug" if profile == "dev" else profile


def cargo_command(arm, profile):
    cmd = ["cargo", "build", "--locked", "--profile", profile]
    for package in PACKAGES:
        cmd += ["-p", package]
    cmd += ["--features", ",".join(FEATURES + ARM_FEATURES[arm])]
    return cmd


def sha256(path):
    digest = hashlib.sha256()
    with open(path, "rb") as handle:
        for chunk in iter(lambda: handle.read(1 << 20), b""):
            digest.update(chunk)
    return digest.hexdigest()


def git(*args):
    return subprocess.run(["git", "-C", str(ROOT), *args], capture_output=True, text=True).stdout.strip()


def build(args):
    work = Path(args.work).resolve()
    if not args.dry_run:
        work.mkdir(parents=True, exist_ok=True)
    commit = git("rev-parse", "HEAD")
    dirty = bool(git("status", "--porcelain", "--untracked-files=no"))
    commit_time = int(git("log", "-1", "--format=%ct") or 0)
    meta = {"commit": commit, "dirty": dirty, "profile": args.profile, "arms": {}}
    for arm in ARMS:
        target = work / f"target-{arm}"
        out = target / profile_dir(args.profile)
        env = dict(os.environ, CARGO_TARGET_DIR=str(target))
        if args.jobs:
            env["CARGO_BUILD_JOBS"] = str(args.jobs)
        cmd = cargo_command(arm, args.profile)
        if getattr(args, "skip_cargo", False):
            # The arm is already built (or was copied out of a shared target
            # dir, which is how a host without room for two target trees does
            # it). Everything else still runs: archive mtimes and hashes, the
            # app compile, and the marker verification.
            if not (out / "perry").is_file() and (target / "perry").is_file():
                out = target  # a flat directory of copied archives, not a target tree
            log(f"build {arm}: --skip-cargo, using {out}")
        else:
            log(f"build {arm}: CARGO_TARGET_DIR={target} {' '.join(cmd)}")
        started = time.time()
        if not args.dry_run and not getattr(args, "skip_cargo", False):
            subprocess.run(cmd, cwd=ROOT, env=env, check=True)
        arm_meta = {"target_dir": str(out), "cargo": None if getattr(args, "skip_cargo", False) else cmd,
                    "build_started": started, "archives": {}}
        if not args.dry_run:
            for name in ARCHIVES:
                path = out / name
                if not path.is_file():
                    raise SystemExit(f"{arm}: missing {path} after the build")
                st = path.stat()
                arm_meta["archives"][name] = {
                    "mtime": st.st_mtime,
                    "mtime_iso": datetime.datetime.fromtimestamp(st.st_mtime).isoformat(),
                    "bytes": st.st_size,
                    "sha256": sha256(path),
                    "older_than_commit": st.st_mtime < commit_time,
                }
                if st.st_mtime < commit_time:
                    # Recorded, not fatal: cargo legitimately skips a crate whose
                    # inputs did not change. It is fatal when EVERY archive and
                    # the binary match the other arm — see `assert_arms_differ`.
                    log(f"NOTE {arm}: {name} predates HEAD's commit time "
                        "(cargo cache hit, or a stale archive — check the arm diff below)")
            binary = compile_app(arm, out, work, dry_run=False)
            arm_meta["server_binary"] = str(binary)
            arm_meta["server_binary_bytes"] = binary.stat().st_size
            arm_meta["marker"] = verify_marker(arm, binary)
        else:
            compile_app(arm, out, work, dry_run=True)
        meta["arms"][arm] = arm_meta
    if args.dry_run:
        log("dry-run: skipped cargo, compile and marker verification")
        return meta
    assert_arms_differ(meta)
    (work / "results").mkdir(parents=True, exist_ok=True)
    (work / "build.json").write_text(json.dumps(meta, indent=2))
    log(f"wrote {work / 'build.json'}")
    return meta


def assert_arms_differ(meta):
    """The arms must not be byte-identical, or the A/B is vacuous.

    `tokio-wait-driver` changes perry-stdlib and perry-runtime, so both archives
    and the linked server must differ. Two identical arms is the failure mode
    CLAUDE.md warns about — a stale `.a`, or a feature that never reached the
    build — and it reads as "no regressions" instead of as "nothing measured".
    """
    a, b = (meta["arms"][arm] for arm in ARMS)
    same = [name for name in ("libperry_runtime.a", "libperry_stdlib.a")
            if a["archives"][name]["sha256"] == b["archives"][name]["sha256"]]
    if same:
        raise SystemExit(
            f"the two arms share identical {', '.join(same)}: the "
            "tokio-wait-driver feature did not reach the build, so any "
            "comparison would be vacuous")
    if a["server_binary_bytes"] == b["server_binary_bytes"] and sha256(
            Path(a["server_binary"])) == sha256(Path(b["server_binary"])):
        raise SystemExit("the two arms produced an identical server binary")
    log("arms differ: runtime, stdlib and the linked server are distinct builds")


def compile_app(arm, out, work, dry_run):
    binary = work / f"server-{arm}"
    cmd = [str(out / "perry"), str(APP), "--no-cache", "-o", str(binary)]
    env_desc = f"PERRY_RUNTIME_DIR={out} PERRY_NO_AUTO_OPTIMIZE=1"
    log(f"compile {arm}: {env_desc} {' '.join(cmd)}")
    if dry_run:
        return binary
    env = dict(os.environ, PERRY_RUNTIME_DIR=str(out), PERRY_NO_AUTO_OPTIMIZE="1")
    subprocess.run(cmd, cwd=ROOT, env=env, check=True)
    return binary


def verify_marker(arm, binary):
    logdir = Path(tempfile.mkdtemp(prefix="server-ab-verify-"))
    server = Server(binary, free_port(), logdir)
    server.start_or_kill()
    try:
        http_get(server.port)
    finally:
        server.stop()
        shutil.rmtree(logdir, ignore_errors=True)
    if ARM_MARKER[arm] not in server.stderr_text:
        raise SystemExit(f"{arm}: marker {ARM_MARKER[arm]!r} missing; stderr={server.stderr_text!r}")
    waits = server.waits()
    if waits.get("arm") != ARM_WAITS[arm]:
        raise SystemExit(f"{arm}: wait metrics line missing or wrong arm: {waits}")
    marker_line = next(line for line in server.stderr_text.splitlines() if ARM_MARKER[arm] in line)
    log(f"verified {arm}: {marker_line}")
    return marker_line


# ─── server process ─────────────────────────────────────────────────────────


def free_port():
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        return sock.getsockname()[1]


def http_get(port, timeout=2.0):
    conn = http.client.HTTPConnection("127.0.0.1", port, timeout=timeout)
    try:
        conn.request("GET", "/")
        response = conn.getresponse()
        response.read()
        return response.status
    finally:
        conn.close()


class Server:
    def __init__(self, binary, port, logdir):
        self.binary = Path(binary)
        self.port = port
        self.logdir = Path(logdir)
        self.proc = None
        self.rusage = None
        self.exit_status = None
        self.stderr_text = ""
        self.forced_kill = False

    def start_or_kill(self, timeout=30.0):
        """`start`, but never leave a running server behind on failure.

        The health check can time out with the process alive and holding its
        port; every caller starts the server BEFORE its try/finally, so an
        un-cleaned failure leaks an orphan for the rest of the run — and a
        contended host is exactly where the check times out.
        """
        try:
            self.start(timeout=timeout)
        except BaseException:
            try:
                self.stop(timeout=5.0)
            except BaseException:
                pass
            raise

    def start(self, timeout=30.0):
        env = dict(os.environ, PORT=str(self.port), PERRY_LOOP_STATS="1")
        self.stdout_path = self.logdir / f"server-{self.port}.out"
        self.stderr_path = self.logdir / f"server-{self.port}.err"
        with open(self.stdout_path, "wb") as out, open(self.stderr_path, "wb") as err:
            self.proc = subprocess.Popen([str(self.binary)], env=env, stdout=out, stderr=err)
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            if self.proc.poll() is not None:
                raise RuntimeError(f"server exited early: {self.stderr_path.read_text(errors='replace')[:400]}")
            try:
                if http_get(self.port, timeout=1.0) == 200:
                    return
            except OSError:
                time.sleep(0.05)
        raise RuntimeError("server did not answer GET / within the timeout")

    @property
    def pid(self):
        return self.proc.pid

    def stop(self, timeout=15.0):
        if self.proc is None or self.rusage is not None:
            return
        try:
            os.kill(self.pid, signal.SIGTERM)
        except ProcessLookupError:
            pass
        deadline = time.monotonic() + timeout
        while True:
            try:
                pid, status, rusage = os.wait4(self.pid, os.WNOHANG)
            except ChildProcessError:  # already reaped: no rusage to report
                self.exit_status = self.proc.returncode
                self.stderr_text = self.stderr_path.read_text(errors="replace")
                return
            if pid == self.pid:
                break
            if time.monotonic() > deadline:
                self.forced_kill = True
                try:
                    os.kill(self.pid, signal.SIGKILL)
                    pid, status, rusage = os.wait4(self.pid, 0)
                except (ProcessLookupError, ChildProcessError):
                    self.exit_status = self.proc.returncode
                    self.stderr_text = self.stderr_path.read_text(errors="replace")
                    return
                break
            time.sleep(0.02)
        self.proc.returncode = os.waitstatus_to_exitcode(status)
        self.exit_status = self.proc.returncode
        self.rusage = rusage
        self.stderr_text = self.stderr_path.read_text(errors="replace")

    def lifetime(self):
        ru = self.rusage
        if ru is None:
            return {"rusage_missing": True}
        maxrss_kb = ru.ru_maxrss if IS_LINUX else ru.ru_maxrss // 1024
        return {
            "cpu_user_s": ru.ru_utime, "cpu_sys_s": ru.ru_stime, "rss_peak_kb": maxrss_kb,
            "vcsw": ru.ru_nvcsw, "ivcsw": ru.ru_nivcsw,
        }

    def waits(self):
        match = WAITS_RE.search(self.stderr_text)
        if not match:
            return {}
        out = {}
        for pair in match.group(1).split():
            key, _, value = pair.partition("=")
            out[key] = int(value) if value.isdigit() else value
        return out

    def marker(self):
        for line in self.stderr_text.splitlines():
            if line.startswith("[perry-loop] driver="):
                return line
        return None


def proc_sample(pid):
    """Linux-only window counters; None elsewhere."""
    if not IS_LINUX:
        return None
    try:
        fields = Path(f"/proc/{pid}/stat").read_text().rsplit(")", 1)[1].split()
        utime, stime = int(fields[11]), int(fields[12])
        vcsw = ivcsw = 0
        for task in Path(f"/proc/{pid}/task").iterdir():
            try:
                for line in (task / "status").read_text().splitlines():
                    if line.startswith("voluntary_ctxt_switches:"):
                        vcsw += int(line.split()[1])
                    elif line.startswith("nonvoluntary_ctxt_switches:"):
                        ivcsw += int(line.split()[1])
            except OSError:
                pass
        status = Path(f"/proc/{pid}/status").read_text()
        rss = int(re.search(r"^VmRSS:\s+(\d+)", status, re.M).group(1))
        hwm = int(re.search(r"^VmHWM:\s+(\d+)", status, re.M).group(1))
        threads = int(re.search(r"^Threads:\s+(\d+)", status, re.M).group(1))
        return {"t": time.monotonic(), "utime_s": utime / CLK_TCK, "stime_s": stime / CLK_TCK,
                "vcsw": vcsw, "ivcsw": ivcsw, "rss_kb": rss, "hwm_kb": hwm, "threads": threads}
    except (OSError, AttributeError, IndexError, ValueError):
        return None


def rss_kb(pid):
    sample = proc_sample(pid)
    if sample:
        return sample["rss_kb"]
    out = subprocess.run(["ps", "-o", "rss=", "-p", str(pid)], capture_output=True, text=True).stdout.strip()
    return int(out) if out.isdigit() else None


def window_delta(before, after):
    if not before or not after:
        return {}
    return {
        "win_cpu_user_s": round(after["utime_s"] - before["utime_s"], 3),
        "win_cpu_sys_s": round(after["stime_s"] - before["stime_s"], 3),
        "win_vcsw": after["vcsw"] - before["vcsw"],
        "win_ivcsw": after["ivcsw"] - before["ivcsw"],
        "threads": after["threads"],
    }


# ─── load generators and syscalls ───────────────────────────────────────────


def pick_load_tool(args):
    if args.load_tool == "auto":
        for name in ("oha", "wrk"):
            path = getattr(args, name) or shutil.which(name)
            if path:
                return name, path
        return None, None
    path = getattr(args, args.load_tool, None) or shutil.which(args.load_tool)
    return (args.load_tool, path) if path else (None, None)


WRK_LUA = r"""
done = function(summary, latency, requests)
  local e = summary.errors
  io.write(string.format('WRKJSON {"requests":%d,"duration_us":%d,"errors":%d,"non2xx":%d,"p50_us":%d,"p99_us":%d,"p999_us":%d}\n',
    summary.requests, summary.duration, e.connect + e.read + e.write + e.timeout, e.status,
    latency:percentile(50), latency:percentile(99), latency:percentile(99.9)))
end
"""


def run_load(tool, path, port, conc, duration):
    url = f"http://127.0.0.1:{port}/"
    if tool == "oha":
        base = [path, "-z", f"{duration}s", "-c", str(conc), "-r", "0", "--no-tui"]
        # `--output-format json` on current oha, `-j` on older builds. Try the
        # new spelling and fall back rather than silently reporting nothing.
        data, errors = {}, []
        for json_flag in (["--output-format", "json"], ["-j"]):
            proc = subprocess.run(base + json_flag + [url], capture_output=True, text=True)
            try:
                data = json.loads(proc.stdout or "{}")
            except json.JSONDecodeError:
                data = {}
            if data:
                break
            errors.append(f"{' '.join(json_flag)}: {(proc.stdout + proc.stderr)[-200:]}")
        if not data:
            return {"tool": "oha", "error": " | ".join(errors)}
        summary = data.get("summary", {})
        pct = data.get("latencyPercentiles", {})
        codes = data.get("statusCodeDistribution", {}) or {}
        return {
            "rps": summary.get("requestsPerSec"),
            "requests": sum(codes.values()),
            "success_rate": summary.get("successRate"),
            "p50_ms": ms(pct.get("p50")), "p99_ms": ms(pct.get("p99")), "p999_ms": ms(pct.get("p99.9")),
            "tool": "oha",
        }
    if tool == "wrk":
        script = Path(tempfile.mkstemp(suffix=".lua")[1])
        script.write_text(WRK_LUA)
        threads = max(1, min(conc, os.cpu_count() or 1))
        cmd = [path, f"-t{threads}", f"-c{conc}", f"-d{duration}s", "-s", str(script), url]
        proc = subprocess.run(cmd, capture_output=True, text=True)
        match = re.search(r"WRKJSON (\{.*\})", proc.stdout)
        if not match:
            return {"tool": "wrk", "error": proc.stdout[-400:] + proc.stderr[-400:]}
        data = json.loads(match.group(1))
        secs = data["duration_us"] / 1e6
        return {
            "rps": data["requests"] / secs if secs else None,
            "requests": data["requests"],
            "success_rate": 1 - (data["errors"] + data["non2xx"]) / max(1, data["requests"]),
            "p50_ms": data["p50_us"] / 1000, "p99_ms": data["p99_us"] / 1000, "p999_ms": data["p999_us"] / 1000,
            "tool": "wrk",
        }
    if tool == "ab":
        cmd = [path, "-k", "-q", "-c", str(conc), "-t", str(duration), "-n", "100000000", url]
        proc = subprocess.run(cmd, capture_output=True, text=True)
        text = proc.stdout
        rps = re.search(r"Requests per second:\s+([\d.]+)", text)
        done = re.search(r"Complete requests:\s+(\d+)", text)
        failed = re.search(r"Failed requests:\s+(\d+)", text)
        p50 = re.search(r"^\s+50%\s+(\d+)", text, re.M)
        p99 = re.search(r"^\s+99%\s+(\d+)", text, re.M)
        if not rps:
            return {"tool": "ab", "error": (text + proc.stderr)[-400:]}
        requests = int(done.group(1))
        return {
            "rps": float(rps.group(1)), "requests": requests,
            "success_rate": 1 - int(failed.group(1)) / max(1, requests),
            "p50_ms": float(p50.group(1)) if p50 else None,
            "p99_ms": float(p99.group(1)) if p99 else None,
            "p999_ms": None, "tool": "ab (smoke only: ms resolution, no p999)",
        }
    raise ValueError(tool)


def ms(seconds):
    return None if seconds is None else seconds * 1000.0


class SyscallCounter:
    """`perf stat -e raw_syscalls:sys_enter` attached for the measured window."""

    def __init__(self, mode):
        self.mode = mode
        self.proc = None
        self.note = None

    @staticmethod
    def resolve(mode):
        if not IS_LINUX:
            return "off"
        if mode == "auto":
            if shutil.which("perf"):
                return "perf"
            return "strace" if shutil.which("strace") else "off"
        return mode

    def start(self, pid, duration):
        if self.mode != "perf":
            return
        self.duration = duration
        self.proc = subprocess.Popen(
            ["perf", "stat", "-x", ",", "-e", "raw_syscalls:sys_enter", "-p", str(pid), "--", "sleep", str(duration)],
            stdout=subprocess.DEVNULL, stderr=subprocess.PIPE, text=True,
        )

    def finish(self):
        if self.proc is None:
            return None
        _, err = self.proc.communicate()
        for line in err.splitlines():
            parts = line.split(",")
            if len(parts) > 2 and "raw_syscalls:sys_enter" in line and parts[0].strip().isdigit():
                return int(parts[0]) / self.duration
        self.note = f"perf failed: {err.strip()[-200:]}"
        return None


def strace_sample(binary, port_tool, conc, seconds, logdir):
    """Syscall rate from `strace -c -f` in a SEPARATE server process (strace
    perturbs the server, so its throughput is discarded)."""
    tool, path = port_tool
    server = Server(binary, free_port(), logdir)
    server.start_or_kill()
    out = logdir / f"strace-{server.port}.txt"
    tracer = subprocess.Popen(["strace", "-c", "-f", "-p", str(server.pid), "-o", str(out)],
                              stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    time.sleep(0.5)
    try:
        run_load(tool, path, server.port, conc, seconds)
    finally:
        tracer.send_signal(signal.SIGINT)
        tracer.wait(timeout=30)
        server.stop()
    match = re.search(r"^\s*[\d.]+\s+[\d.]+\s+\d*\s+(\d+)\s+(?:\d+\s+)?total", out.read_text(), re.M) if out.exists() else None
    return int(match.group(1)) / seconds if match else None


# ─── idle-connection capacity ───────────────────────────────────────────────

REQUEST = b"GET / HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: keep-alive\r\n\r\n"
IP_BIND_ADDRESS_NO_PORT = 24


def raise_nofile(wanted):
    soft, hard = resource.getrlimit(resource.RLIMIT_NOFILE)
    target = hard if hard != resource.RLIM_INFINITY else max(wanted, soft)
    if sys.platform == "darwin":
        target = min(target, 10240) if hard == resource.RLIM_INFINITY else target
    try:
        resource.setrlimit(resource.RLIMIT_NOFILE, (min(max(soft, wanted), target), hard))
    except (ValueError, OSError):
        pass
    return resource.getrlimit(resource.RLIMIT_NOFILE)[0]


def idle_client(args):
    """Subprocess: open N keep-alive connections, one request each, then hold.

    Prints one JSON line when opened; on `check` prints how many are still
    open; exits on `quit` (closing everything)."""
    limit = raise_nofile(args.count + 256)
    sources = args.sources.split(",")
    sel = selectors.DefaultSelector()
    held, failed = [], 0
    started = time.monotonic()
    index = 0
    while index < args.count:
        batch = []
        for _ in range(min(args.batch, args.count - index)):
            src = sources[index % len(sources)]
            index += 1
            sock = socket.socket()
            sock.setblocking(False)
            try:
                if len(sources) > 1:
                    if IS_LINUX:
                        sock.setsockopt(socket.IPPROTO_IP, IP_BIND_ADDRESS_NO_PORT, 1)
                    sock.bind((src, 0))
                sock.connect_ex(("127.0.0.1", args.port))
            except OSError:
                sock.close()
                failed += 1
                continue
            state = {"sock": sock, "buf": b"", "sent": False}
            sel.register(sock, selectors.EVENT_WRITE, state)
            batch.append(state)
        deadline = time.monotonic() + args.timeout
        pending = len(batch)
        while pending and time.monotonic() < deadline:
            for key, _ in sel.select(timeout=0.5):
                state = key.data
                sock = state["sock"]
                try:
                    if not state["sent"]:
                        err = sock.getsockopt(socket.SOL_SOCKET, socket.SO_ERROR)
                        if err:
                            raise OSError(err, "connect")
                        sock.send(REQUEST)
                        state["sent"] = True
                        sel.modify(sock, selectors.EVENT_READ, state)
                        continue
                    chunk = sock.recv(4096)
                    if not chunk:
                        raise OSError("closed")
                    state["buf"] += chunk
                    head, sep, rest = state["buf"].partition(b"\r\n\r\n")
                    if not sep:
                        continue
                    length = re.search(rb"(?i)content-length:\s*(\d+)", head)
                    if length and len(rest) < int(length.group(1)):
                        continue
                    sel.unregister(sock)
                    held.append(sock)
                    pending -= 1
                except OSError:
                    sel.unregister(sock)
                    sock.close()
                    failed += 1
                    pending -= 1
        for key in list(sel.get_map().values()):
            sel.unregister(key.fileobj)
            key.fileobj.close()
            failed += 1
    print(json.dumps({"phase": "opened", "open": len(held), "failed": failed,
                      "secs": round(time.monotonic() - started, 3), "nofile": limit}), flush=True)
    for line in sys.stdin:
        if line.strip() == "check":
            still = 0
            for sock in held:
                try:
                    if sock.recv(1, socket.MSG_PEEK) == b"":
                        continue
                    still += 1
                except BlockingIOError:
                    still += 1
                except OSError:
                    pass
            print(json.dumps({"phase": "held", "open": still}), flush=True)
        elif line.strip() == "quit":
            break
    for sock in held:
        sock.close()


def idle_sources(count):
    if not IS_LINUX:
        return "127.0.0.1"
    needed = max(1, -(-count // 25000))
    return ",".join(f"127.0.0.{i + 1}" for i in range(needed))


def measure_idle(arm, binary, count, hold, logdir):
    server = Server(binary, free_port(), logdir)
    server.start_or_kill()
    sample = {"scenario": f"idle-{count}", "arm": arm}
    client = None
    try:
        time.sleep(0.5)
        rss_before = rss_kb(server.pid)
        client = subprocess.Popen(
            [sys.executable, __file__, "idle-client", "--port", str(server.port), "--count", str(count),
             "--sources", idle_sources(count)],
            stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True,
        )
        opened = json.loads(client.stdout.readline() or "{}")
        # RSS with the connections open, BEFORE the hold: the per-connection
        # cost exists even if the server later reaps the sockets, and a
        # zero-survivor run must still report a number rather than a blank.
        rss_open = rss_kb(server.pid)
        before = proc_sample(server.pid)
        time.sleep(hold)
        after = proc_sample(server.pid)
        rss_after = rss_kb(server.pid)
        client.stdin.write("check\n")
        client.stdin.flush()
        held = json.loads(client.stdout.readline() or "{}")
        open_now = held.get("open", 0)
        sample.update({
            "requested": count, "opened": opened.get("open"), "open_after_hold": open_now,
            "failed": opened.get("failed"), "open_secs": opened.get("secs"),
            "client_nofile": opened.get("nofile"),
            "rss_before_kb": rss_before, "rss_open_kb": rss_open, "rss_after_kb": rss_after,
            "bytes_per_conn": ((rss_open - rss_before) * 1024 / (opened.get("open") or 0))
            if (opened.get("open") and rss_before is not None and rss_open is not None) else None,
            "bytes_per_conn_after_hold": ((rss_after - rss_before) * 1024 / open_now)
            if (open_now and rss_before is not None and rss_after is not None) else None,
            "idle_hold_s": hold,
        })
        if before and after:
            sample["idle_cpu_ms"] = round(
                (after["utime_s"] + after["stime_s"] - before["utime_s"] - before["stime_s"]) * 1000, 1)
            sample["idle_vcsw"] = after["vcsw"] - before["vcsw"]
            sample["threads"] = after["threads"]
    finally:
        if client:
            try:
                client.stdin.write("quit\n")
                client.stdin.flush()
                client.wait(timeout=120)
            except (OSError, subprocess.TimeoutExpired):
                client.kill()
        server.stop()
    finish_sample(sample, arm, server)
    return sample


# ─── load scenario ──────────────────────────────────────────────────────────


def measure_load(arm, binary, conc, args, tool, logdir, syscall_mode):
    server = Server(binary, free_port(), logdir)
    server.start_or_kill()
    sample = {"scenario": f"load-c{conc}", "arm": arm, "concurrency": conc}
    try:
        if args.warmup:
            run_load(tool[0], tool[1], server.port, conc, args.warmup)
        counter = SyscallCounter(syscall_mode)
        before = proc_sample(server.pid)
        load_before = os.getloadavg()[0]
        started = time.monotonic()
        counter.start(server.pid, args.duration)
        result = run_load(tool[0], tool[1], server.port, conc, args.duration)
        wall = time.monotonic() - started
        syscalls = counter.finish()
        after = proc_sample(server.pid)
        sample["loadavg_after"] = os.getloadavg()[0]
        sample.update(result)
        sample.update(window_delta(before, after))
        sample["load_wall_s"] = round(wall, 3)
        sample["loadavg_before"] = load_before
        if syscalls is not None:
            sample["syscalls_per_s"] = round(syscalls, 1)
            sample["syscalls_source"] = "perf raw_syscalls:sys_enter (measured window)"
        elif counter.note:
            sample["syscalls_note"] = counter.note
        if before and after and result.get("requests"):
            cpu = sum(sample[k] for k in ("win_cpu_user_s", "win_cpu_sys_s"))
            sample["cpu_us_per_req"] = round(cpu * 1e6 / result["requests"], 2)
    finally:
        server.stop()
    if syscall_mode == "strace":
        rate = strace_sample(binary, tool, conc, args.strace_seconds, logdir)
        sample["syscalls_per_s"] = rate
        sample["syscalls_source"] = "strace -c -f (separate process; perturbed)"
    finish_sample(sample, arm, server)
    return sample


def finish_sample(sample, arm, server):
    sample.update(server.lifetime())
    sample["exit_status"] = server.exit_status
    sample["forced_kill"] = server.forced_kill
    sample["marker"] = server.marker()
    waits = server.waits()
    sample["waits"] = waits
    problems = []
    if sample["marker"] is None or ARM_MARKER[arm] not in sample["marker"]:
        problems.append("arm marker missing or wrong")
    if waits.get("arm") != ARM_WAITS[arm]:
        problems.append("wait metrics missing or wrong arm")
    if server.forced_kill:
        problems.append("server needed SIGKILL")
    if sample.get("rusage_missing"):
        problems.append("server was reaped before rusage could be read")
    if "error" in sample:
        problems.append("load tool error")
    sample["valid"] = not problems
    sample["problems"] = problems


# ─── run and report ─────────────────────────────────────────────────────────


def host_info():
    info = {"platform": platform.platform(), "python": platform.python_version(),
            "cpus": os.cpu_count(), "nofile_soft": resource.getrlimit(resource.RLIMIT_NOFILE)[0]}
    if IS_LINUX:
        for path in ("/proc/sys/kernel/perf_event_paranoid", "/proc/sys/net/ipv4/ip_local_port_range",
                     "/proc/sys/net/core/somaxconn"):
            try:
                info[path] = Path(path).read_text().strip()
            except OSError:
                pass
    return info


def run(args):
    work = Path(args.work).resolve()
    tool = pick_load_tool(args)
    syscall_mode = SyscallCounter.resolve(args.syscalls)
    concurrency = [int(c) for c in args.concurrency.split(",") if c]
    idle = [int(n) for n in args.idle.split(",") if n]
    if tool[0] is None:
        print(INSTALL_HINTS, file=sys.stderr)
        if not args.dry_run:
            raise SystemExit(2)
    if args.dry_run:
        log(f"dry-run plan: rounds={args.rounds} arms={ARMS} concurrency={concurrency} idle={idle}")
        log(f"load tool: {tool[0] or 'NONE'} ({tool[1]}); syscalls: {syscall_mode}")
        for rnd in range(1, args.rounds + 1):
            order = ARMS if rnd % 2 else tuple(reversed(ARMS))
            for arm in order:
                for conc in concurrency:
                    log(f"  round {rnd} {arm}: load c={conc} warmup={args.warmup}s duration={args.duration}s")
                for count in idle:
                    log(f"  round {rnd} {arm}: idle {count} connections, hold {args.idle_hold}s "
                        f"(sources {idle_sources(count)})")
        synthetic_report(args)
        return
    build_meta = json.loads((work / "build.json").read_text())
    needed = max([1024] + [c * 2 + 256 for c in concurrency] + [n + 1024 for n in idle])
    nofile = raise_nofile(needed)
    if nofile < needed:
        log(f"WARNING: RLIMIT_NOFILE {nofile} < {needed}; raise it (ulimit -n 1048576) "
            "and see fs.nr_open / net.ipv4.ip_local_port_range for the 100k idle test")
    results_dir = work / "results"
    results_dir.mkdir(parents=True, exist_ok=True)
    logdir = results_dir / "logs"
    logdir.mkdir(exist_ok=True)
    doc = {"build": build_meta, "host": host_info(), "tool": tool[0], "syscalls": syscall_mode,
           "config": {"rounds": args.rounds, "concurrency": concurrency, "duration": args.duration,
                      "warmup": args.warmup, "idle": idle, "idle_hold": args.idle_hold},
           "started": datetime.datetime.now().isoformat(), "samples": []}
    out = results_dir / "results.json"
    for rnd in range(1, args.rounds + 1):
        order = ARMS if rnd % 2 else tuple(reversed(ARMS))
        for arm in order:
            binary = build_meta["arms"][arm]["server_binary"]
            jobs = [(f"load-c{c}", lambda c=c: measure_load(arm, binary, c, args, tool, logdir, syscall_mode))
                    for c in concurrency]
            jobs += [(f"idle-{n}", lambda n=n: measure_idle(arm, binary, n, args.idle_hold, logdir)) for n in idle]
            for scenario, job in jobs:
                log(f"round {rnd} {arm}: {scenario}")
                try:
                    sample = job()
                except Exception as error:  # record the failure, keep the other samples
                    sample = {"scenario": scenario, "arm": arm, "valid": False,
                              "problems": [f"exception: {error!r}"[:300]]}
                    log(f"  FAILED: {error!r}")
                sample.update({"round": rnd, "binary_bytes": build_meta["arms"][arm]["server_binary_bytes"]})
                doc["samples"].append(sample)
                out.write_text(json.dumps(doc, indent=2))
    doc["finished"] = datetime.datetime.now().isoformat()
    out.write_text(json.dumps(doc, indent=2))
    report_from(doc, results_dir)


LOAD_METRICS = [
    ("rps", "throughput (req/s)"), ("p50_ms", "p50 latency (ms)"), ("p99_ms", "p99 latency (ms)"),
    ("p999_ms", "p999 latency (ms)"), ("win_cpu_user_s", "CPU user, window (s)"),
    ("win_cpu_sys_s", "CPU sys, window (s)"), ("cpu_us_per_req", "CPU per request (µs)"),
    ("load_wall_s", "load wall (s)"), ("win_vcsw", "voluntary ctx switches, window"),
    ("win_ivcsw", "involuntary ctx switches, window"), ("syscalls_per_s", "syscalls/s"),
    ("rss_peak_kb", "RSS peak (KiB)"), ("cpu_user_s", "CPU user, lifetime (s)"),
    ("cpu_sys_s", "CPU sys, lifetime (s)"), ("vcsw", "voluntary ctx switches, lifetime"),
    ("ivcsw", "involuntary ctx switches, lifetime"), ("binary_bytes", "binary size (bytes)"),
]
IDLE_METRICS = [
    ("opened", "connections opened"), ("open_after_hold", "connections open after hold"),
    ("open_secs", "time to open them (s)"), ("rss_before_kb", "RSS before (KiB)"),
    ("rss_open_kb", "RSS with idle conns (KiB)"), ("rss_after_kb", "RSS after the hold (KiB)"),
    ("bytes_per_conn", "bytes per connection"),
    ("bytes_per_conn_after_hold", "bytes per surviving connection"),
    ("idle_cpu_ms", "CPU during hold (ms)"), ("idle_vcsw", "voluntary ctx switches during hold"),
    ("rss_peak_kb", "RSS peak (KiB)"), ("threads", "threads"),
]
WAIT_METRICS = [
    ("tokio_ticks", "tokio ticks"), ("tokio_tick_ns", "time in tokio ticks (ns)"),
    ("tokio_tick_max_ns", "longest tokio tick (ns)"), ("turnloop_waits", "turnloop turns"),
    ("turnloop_wait_ns", "time in turnloop turns (ns)"), ("turnloop_wait_max_ns", "longest turnloop turn (ns)"),
    ("condvar_waits", "condvar parks"), ("condvar_wait_ns", "time in condvar parks (ns)"),
    ("fast_drives", "fast drives"), ("fast_drive_ns", "time in fast drives (ns)"),
    ("zero_budget", "zero-budget returns"), ("throttle_sleeps", "spin-throttle sleeps"),
    ("wake_samples", "wake-latency samples"), ("wake_lt50us", "wakes <50µs"),
    ("wake_lt200us", "wakes <200µs"), ("wake_lt1ms", "wakes <1ms"), ("wake_lt5ms", "wakes <5ms"),
    ("wake_ge5ms", "wakes ≥5ms"), ("wake_max_ns", "slowest wake (ns)"),
]


def value_of(sample, key):
    if key in sample and isinstance(sample[key], (int, float)):
        return sample[key]
    waits = sample.get("waits") or {}
    value = waits.get(key)
    return value if isinstance(value, (int, float)) else None


def summarize(doc):
    scenarios = {}
    for sample in doc["samples"]:
        if not sample.get("valid"):
            continue
        scenarios.setdefault(sample["scenario"], []).append(sample)
    summary = {}
    for scenario, samples in sorted(scenarios.items(), key=lambda kv: scenario_key(kv[0])):
        metrics = (LOAD_METRICS if scenario.startswith("load") else IDLE_METRICS) + WAIT_METRICS
        rows = {}
        for key, label in metrics:
            row = {"label": label}
            for arm in ARMS:
                values = [v for s in samples if s["arm"] == arm and (v := value_of(s, key)) is not None]
                row[arm] = ({"median": statistics.median(values), "min": min(values), "max": max(values),
                             "n": len(values)} if values else None)
            if row["turnloop"] and row["tokio"] and row["tokio"]["median"]:
                row["delta_pct"] = (row["turnloop"]["median"] / row["tokio"]["median"] - 1) * 100
            else:
                row["delta_pct"] = None
            rows[key] = row
        summary[scenario] = rows
    invalid = [s for s in doc["samples"] if not s.get("valid")]
    return {"scenarios": summary, "invalid_samples": len(invalid),
            "invalid_reasons": sorted({p for s in invalid for p in s.get("problems", [])})}


def scenario_key(name):
    kind, _, num = name.partition("-")
    return (kind, int(re.sub(r"\D", "", num) or 0))


def fmt(cell):
    if not cell:
        return "–"

    def num(v):
        if isinstance(v, float) and not v.is_integer():
            return f"{v:.3g}" if abs(v) < 100 else f"{v:,.0f}"
        return f"{int(v):,}"
    return f"{num(cell['median'])} [{num(cell['min'])}–{num(cell['max'])}]"


def markdown(summary, doc):
    build = doc.get("build", {})
    lines = [
        "# turnloop server A/B", "",
        f"- commit `{build.get('commit', '?')}` (dirty={build.get('dirty')}), profile `{build.get('profile')}`",
        f"- host: {doc.get('host', {}).get('platform')}, cpus={doc.get('host', {}).get('cpus')}",
        f"- load tool: {doc.get('tool')}; syscalls: {doc.get('syscalls')}; config: {json.dumps(doc.get('config'))}",
        f"- invalid samples: {summary['invalid_samples']} {summary['invalid_reasons']}",
    ]
    for arm, meta in build.get("arms", {}).items():
        mt = {k: v.get("mtime_iso") for k, v in meta.get("archives", {}).items()}
        lines.append(f"- {arm}: marker `{meta.get('marker')}`, binary {meta.get('server_binary_bytes')} B, archives {mt}")
    lines += ["", "Median [min–max] over valid rounds; Δ = turnloop median vs tokio-wait-driver median.", ""]
    for scenario, rows in summary["scenarios"].items():
        lines += [f"## {scenario}", "", "| metric | turnloop | tokio-wait-driver | Δ % |", "|---|---|---|---|"]
        for key, row in rows.items():
            if not row["turnloop"] and not row["tokio"]:
                continue
            delta = "–" if row["delta_pct"] is None else f"{row['delta_pct']:+.1f}"
            lines.append(f"| {row['label']} | {fmt(row['turnloop'])} | {fmt(row['tokio'])} | {delta} |")
        lines.append("")
    return "\n".join(lines)


def report_from(doc, results_dir):
    summary = summarize(doc)
    (results_dir / "summary.json").write_text(json.dumps(summary, indent=2))
    (results_dir / "summary.md").write_text(markdown(summary, doc))
    log(f"wrote {results_dir / 'summary.md'} and summary.json")
    print(markdown(summary, doc))


def report(args):
    results_dir = Path(args.work).resolve() / "results"
    report_from(json.loads((results_dir / "results.json").read_text()), results_dir)


def synthetic_report(args):
    """Dry-run: drive the summary/markdown code on generated samples."""
    doc = {"build": {"commit": git("rev-parse", "HEAD"), "dirty": False, "profile": args.profile, "arms": {}},
           "host": host_info(), "tool": "synthetic", "syscalls": "synthetic", "config": {}, "samples": []}
    for rnd in range(1, 4):
        for arm in ARMS:
            base = 1.0 if arm == "turnloop" else 1.1
            waits = {"arm": ARM_WAITS[arm], "tokio_ticks": 1000 * rnd, "tokio_tick_ns": 5_000_000 * rnd,
                     "turnloop_waits": 10 if arm == "turnloop" else 0, "wake_samples": 900, "wake_lt50us": 800,
                     "wake_lt200us": 90, "wake_lt1ms": 10, "wake_lt5ms": 0, "wake_ge5ms": 0}
            doc["samples"].append({
                "scenario": "load-c64", "arm": arm, "round": rnd, "valid": True, "rps": 50000 / base + rnd,
                "p50_ms": 0.5 * base, "p99_ms": 2.0 * base, "p999_ms": 5.0 * base, "win_cpu_user_s": 10.0 * base,
                "win_cpu_sys_s": 3.0, "cpu_us_per_req": 17.0 * base, "load_wall_s": 15.0, "win_vcsw": 1000,
                "win_ivcsw": 10, "syscalls_per_s": 200000.0, "rss_peak_kb": 20000, "cpu_user_s": 11.0,
                "cpu_sys_s": 3.2, "vcsw": 1200, "ivcsw": 12, "binary_bytes": 10_000_000, "waits": waits})
            doc["samples"].append({
                "scenario": "idle-10000", "arm": arm, "round": rnd, "valid": True, "open_after_hold": 10000,
                "rss_before_kb": 8000, "rss_after_kb": 48000, "bytes_per_conn": 4096.0 * base,
                "idle_cpu_ms": 2.0, "idle_vcsw": 20, "rss_peak_kb": 50000, "threads": 4, "waits": waits})
    doc["samples"].append({"scenario": "load-c64", "arm": "tokio", "round": 9, "valid": False,
                           "problems": ["arm marker missing or wrong"]})
    summary = summarize(doc)
    text = markdown(summary, doc)
    assert summary["invalid_samples"] == 1
    assert summary["scenarios"]["load-c64"]["rps"]["turnloop"]["n"] == 3
    assert "| throughput (req/s) |" in text and "## idle-10000" in text
    log("dry-run: synthetic summary OK; first lines:")
    print("\n".join(text.splitlines()[:16]))


# ─── cli ────────────────────────────────────────────────────────────────────


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = parser.add_subparsers(dest="command", required=True)

    def common(p):
        p.add_argument("--work", default=str(ROOT / "target/turnloop-server-ab"))
        p.add_argument("--dry-run", action="store_true")
        p.add_argument("--profile", default="release")

    def run_options(p):
        p.add_argument("--rounds", type=int, default=5)
        p.add_argument("--concurrency", default="1,64,1024")
        p.add_argument("--duration", type=int, default=15)
        p.add_argument("--warmup", type=int, default=3)
        p.add_argument("--idle", default="10000,100000")
        p.add_argument("--idle-hold", type=int, default=10)
        p.add_argument("--load-tool", choices=["auto", "oha", "wrk", "ab"], default="auto")
        p.add_argument("--oha")
        p.add_argument("--wrk")
        p.add_argument("--ab")
        p.add_argument("--syscalls", choices=["auto", "perf", "strace", "off"], default="auto")
        p.add_argument("--strace-seconds", type=int, default=5)

    p_build = sub.add_parser("build")
    common(p_build)
    p_build.add_argument("--jobs", type=int)
    p_build.add_argument("--skip-cargo", action="store_true",
                         help="arms are already built: <work>/target-<arm> holds perry and the archives "
                              "(for a host with room for only one cargo target tree)")
    p_run = sub.add_parser("run")
    common(p_run)
    run_options(p_run)
    p_all = sub.add_parser("all")
    common(p_all)
    run_options(p_all)
    p_all.add_argument("--jobs", type=int)
    p_all.add_argument("--skip-cargo", action="store_true")
    p_report = sub.add_parser("report")
    common(p_report)
    p_idle = sub.add_parser("idle-client")
    p_idle.add_argument("--port", type=int, required=True)
    p_idle.add_argument("--count", type=int, required=True)
    p_idle.add_argument("--sources", default="127.0.0.1")
    p_idle.add_argument("--batch", type=int, default=512)
    p_idle.add_argument("--timeout", type=float, default=15.0)

    args = parser.parse_args()
    if args.command == "idle-client":
        idle_client(args)
    elif args.command == "build":
        build(args)
    elif args.command == "run":
        run(args)
    elif args.command == "report":
        report(args)
    elif args.command == "all":
        build(args)
        run(args)


if __name__ == "__main__":
    main()
