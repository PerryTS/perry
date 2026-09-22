#!/usr/bin/env python3
"""turnloop server A/B harness: the turnloop arm vs a pre-migration commit.

The tokio arm was originally this same tree built with
`perry-stdlib/tokio-wait-driver`. That feature has been DELETED, so the only
tokio baseline left is a pre-migration checkout, passed with `--arm-tree
tokio=<path>`. See the cross-commit note below for why that is the RIGHT
baseline and not merely the surviving one.

Builds both arms from ONE commit into separate target dirs (prebuilt archives,
PERRY_NO_AUTO_OPTIMIZE=1 — the A/B feature does not survive auto-optimize),
compiles the same node:http server with each, then measures each arm in
interleaved fresh-process rounds:

  * load scenarios at fixed concurrency (default 1, 64, 1024 connections):
    throughput and p50/p99/p999 latency; `perf stat` over the same window for
    RETIRED instructions, cycles, IPC, task-clock, context switches, CPU
    migrations, page faults and the syscall tracepoint, each also normalised
    PER REQUEST so a throughput win cannot hide a per-request regression; CPU
    user/sys, wall, context switches, peak RSS;
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

Two different instruction counts, never conflated:

  * the load table's `instructions` is `perf stat` — instructions RETIRED on the
    real CPU during the measured window, with cache, branch and SMT effects.
    That is cost under load, and it moves with concurrency;
  * the `callgrind` subcommand is Valgrind `Ir` — instructions EXECUTED under a
    serialising simulator with no cache or branch model. Deterministic and
    load-independent BY CONSTRUCTION, which makes it a good exact A/B of one
    code path and no statement at all about cost under load. It gets its own
    section and its own caveat, and covers the microbenchmarks only (see
    `CALLGRIND_NOTE` for why the server workload is not run under it).

Hosts. The harness prints the host it ran on and decides per sample whether
TIMING may be quoted: above `--max-loadavg`, or on a host that looks like a
shared build box (or with `--shared-host`), throughput and latency are marked
ADVISORY in the table instead of being presented as authoritative. Counters are
per-process and stay valid on a busy host, so the perf group is not downgraded.
Run counters on the Linux box; run timing on the quiet machine.

Usage (Linux x86_64):
  scripts/turnloop/server_ab.py all --work /root/turnloop-ab
  scripts/turnloop/server_ab.py build --work DIR [--profile release] [--skip-cargo]
  scripts/turnloop/server_ab.py run --work DIR [--rounds 5] [--concurrency 1,64,1024]
      [--duration 15] [--warmup 3] [--idle 10000,100000] [--idle-hold 10]
      [--load-tool auto|oha|wrk|ab] [--perf auto|perf|strace|off]
      [--max-loadavg 1.5] [--shared-host]
  scripts/turnloop/server_ab.py callgrind --work DIR [--probes a,b] [--valgrind PATH]
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
# The tokio arm carries no extra features. It used to be selected in-tree by
# `perry-stdlib/tokio-wait-driver`; that feature is DELETED, so the tokio arm is
# now only reachable through cross-commit mode below, where it is built from a
# pre-migration checkout whose own tree still has whatever it needs.
ARM_FEATURES = {"turnloop": [], "tokio": []}
ARM_MARKER = {
    "turnloop": "[perry-loop] driver=turnloop",
    # Unreachable: a pre-migration tree prints no `[perry-loop]` line at all,
    # and its ABSENCE is what `verify_arm` asserts for the tokio arm instead.
    # Kept so the two tables stay keyed by ARMS rather than silently partial.
    "tokio": None,
}
ARM_WAITS = {"turnloop": "turnloop", "tokio": None}
# Cross-commit mode: the two arms are two COMMITS, not one commit built twice
# with a feature flag. That is what P1-P8 force -- once `node:net`, the servers,
# the clients and the database drivers stop going through tokio at all, the
# `tokio-wait-driver` feature no longer selects "Perry on tokio"; it selects
# "Perry on turnloop with the wait driver swapped and the migrated subjects
# falling back to inline", which is a third thing and not the baseline anyone
# wants. So the tokio arm becomes a pre-migration commit. It has NO
# `[perry-loop]` marker and NO wait-metrics line, because neither existed
# before P0 -- and their ABSENCE is what proves the arm is the tokio build,
# exactly as the marker's PRESENCE proves the turnloop one.
CROSS = {"enabled": False, "trees": {}, "commits": {}}


def cross_enabled():
    return CROSS["enabled"]
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
HOSTNAME = socket.gethostname()
# Hosts whose TIMING is never authoritative, whatever the loadavg says at the
# moment we look: shared build boxes. Their counters are still fine — retired
# instructions, syscalls and page faults are per-process.
SHARED_HOST_PATTERNS = ("perrybuilder", "builder", "buildbox", "ci-")
# Ambient 1-minute load, sampled ONCE before any round runs. Everything after
# that point includes our own load, which is the point of the exercise.
AMBIENT_LOADAVG = [0.0]
# Hosts that ARE the timing machine of record.
QUIET_HOST_PATTERNS = ("perry-macos", "perry-mini")
WAITS_RE = re.compile(r"^\[perry-loop-waits\] (.*)$", re.M)
CLK_TCK = os.sysconf("SC_CLK_TCK") if hasattr(os, "sysconf") else 100

INSTALL_HINTS = """\
No HTTP load generator found. Install one of:
  oha (preferred): cargo install oha --locked     # or: apt install oha / brew install oha
  wrk:             apt install wrk                # or: brew install wrk
Then re-run, or pass --load-tool with an explicit path via --oha/--wrk."""


def host_role():
    """('shared'|'quiet'|'unknown', hint) from the hostname alone."""
    lower = HOSTNAME.lower()
    for pattern in SHARED_HOST_PATTERNS:
        if pattern in lower:
            return "shared", f"hostname {HOSTNAME!r} matches {pattern!r}"
    for pattern in QUIET_HOST_PATTERNS:
        if pattern in lower:
            return "quiet", f"hostname {HOSTNAME!r} matches {pattern!r}"
    return "unknown", ""


HOST_ROLE, SHARED_HOST_HINT = host_role()
if HOST_ROLE != "shared":
    SHARED_HOST_HINT = ""


def log(msg):
    print(f"[server_ab {datetime.datetime.now():%H:%M:%S}] {msg}", flush=True)


# ─── build ──────────────────────────────────────────────────────────────────


def profile_dir(profile):
    return "debug" if profile == "dev" else profile


def cargo_command(arm, profile):
    if arm == "tokio" and not cross_enabled():
        # Fail loudly rather than silently building a SECOND turnloop arm and
        # reporting a 0% delta as if it meant something. The in-tree selector
        # for this arm was `perry-stdlib/tokio-wait-driver`, which no longer
        # exists; a pre-migration commit is the only tokio baseline there is.
        raise SystemExit(
            "the tokio arm can no longer be built from this tree: the "
            "`tokio-wait-driver` feature was deleted with the migration. Pass "
            "`--arm-tree tokio=<path-to-pre-migration-checkout>` to measure "
            "against a pre-migration commit instead.")
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
    return git_in(ROOT, *args)


def git_in(tree, *args):
    """git in an explicit checkout. Cross-commit arms live outside ROOT, and
    `git -C ROOT -C other` is cumulative rather than a replacement, so the two
    callers must not share one hardcoded -C."""
    return subprocess.run(["git", "-C", str(tree), *args],
                          capture_output=True, text=True).stdout.strip()


def build(args):
    work = Path(args.work).resolve()
    if not args.dry_run:
        work.mkdir(parents=True, exist_ok=True)
    commit = git("rev-parse", "HEAD")
    dirty = bool(git("status", "--porcelain", "--untracked-files=no"))
    commit_time = int(git("log", "-1", "--format=%ct") or 0)
    meta = {"commit": commit, "dirty": dirty, "profile": args.profile, "arms": {}}
    for arm in ARMS:
        if cross_enabled() and arm in CROSS["trees"]:
            # An already-built tree at another commit. Its layout is a normal
            # cargo target dir, so `out` is <tree>/target/<profile-dir>.
            target = Path(CROSS["trees"][arm]).resolve()
            out = target / "target" / profile_dir(args.profile)
            if not (out / "perry").is_file() and (target / "perry").is_file():
                out = target
        else:
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
        if cross_enabled() and arm in CROSS["trees"]:
            arm_meta["commit"] = CROSS["commits"].get(arm)
            arm_meta["tree"] = str(CROSS["trees"][arm])
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

    The arms are two different COMMITS, so both archives and the linked server
    must differ. Two identical arms is the failure mode CLAUDE.md warns about —
    a stale `.a`, or a tree that was never rebuilt — and it reads as "no
    regressions" instead of as "nothing measured".
    """
    a, b = (meta["arms"][arm] for arm in ARMS)
    same = [name for name in ("libperry_runtime.a", "libperry_stdlib.a")
            if a["archives"][name]["sha256"] == b["archives"][name]["sha256"]]
    if same:
        why = ("the two arms are supposed to be different COMMITS, so identical "
               "archives mean one tree was not rebuilt"
               if cross_enabled() else
               "the arms were built from the same tree")
        raise SystemExit(
            f"the two arms share identical {', '.join(same)}: {why}, so any "
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


def pick_marker(stderr_text, needle, startswith=False):
    """The PRIMARY agent's marker line, not merely the first one.

    turnloop P9 gave every JS agent its own loop, so a program with a
    `worker_threads` Worker prints one `[perry-loop] driver=turnloop ... agent=N`
    line per agent that owned one -- and a worker retires DURING the program
    while the primary retires at exit, so the worker's line comes first. The
    arm marker has to be the primary agent's or a multi-agent app would have its
    sample described by a worker's counters.

    `agent=` is absent on any build that predates P9, and on those the first
    match is the only match, so the fallback is exactly the old behaviour.
    """
    matches = [
        line for line in stderr_text.splitlines()
        if (line.startswith(needle) if startswith else needle in line)
    ]
    if not matches:
        return None
    for line in matches:
        if line.endswith(" agent=0") or " agent=0 " in line:
            return line
    return matches[0]


# `[perry-loop] p1 comp_read=… comp_write=…` — the per-CLASS census.
P1_CENSUS_RE = re.compile(r"\[perry-loop\] p1 (comp_[^\n]*)")


def net_completions(stderr_text):
    """Net completions actually delivered, from the `p1` census line.

    This exists because the aggregate `completions=` field does NOT mean what
    this harness used to claim it meant. It counts the driver's whole turn
    output summed across every class — P1 net, P2 process, P3 JS timers, P4
    pool — before routing. The runtime's doc comment said "completions
    dispatched to a P1 net subsystem" and this file quoted that as its
    justification, which made the check weaker than advertised: a server that
    declined its listener to hyper but armed a keep-alive deadline would have
    shown a non-zero count and passed. Until recently an HTTP keep-alive
    connection produced TWO timer completions per request, so that was not a
    hypothetical.

    Returns None when the line is absent (a binary older than the census), so
    the caller can fall back and say that it did.
    """
    match = P1_CENSUS_RE.search(stderr_text or "")
    if not match:
        return None
    fields = dict(
        pair.split("=", 1) for pair in match.group(1).split() if "=" in pair
    )
    # Only classes that mean "a socket moved bytes or was accepted". A timer
    # completion is exactly what must NOT count here.
    total = 0
    for key in ("comp_accept", "comp_read", "comp_write", "comp_connect"):
        value = fields.get(key, "0")
        total += int(value) if value.isdigit() else 0
    return total


def marker_completions(marker_line):
    """`completions=N` from a `[perry-loop]` marker line, or None if absent.

    The weak fallback: every class summed, not net alone. See
    `net_completions` for why that distinction matters.
    """
    if not marker_line:
        return None
    for pair in marker_line.split():
        key, _, value = pair.partition("=")
        if key == "completions":
            return int(value) if value.isdigit() else None
    return None


def assert_turnloop_carried_io(arm, marker_line, where, stderr_text=None):
    """The arm marker proves the WAIT DRIVER; this proves turnloop did the I/O.

    They are not the same claim, and the gap is exactly the shape CLAUDE.md warns
    about: a gate that runs while its subject never did. `try_listen_on_turnloop`
    legitimately DECLINES — a thread that cannot get a loop of its own keeps the
    hyper/tokio accept loop (the P1 coexistence rule, and the reason group A's
    edges survive). A server that declined still parks in a turnloop loop, so it
    still prints `driver=turnloop`, and every number we would then publish would
    describe a hyper server wearing the turnloop label.

    Necessary, not sufficient: a non-zero count proves turnloop carried SOME P1
    net I/O in this process, not specifically this server's listener. That is
    still the discriminating quantity the runtime exposes, and it is infinitely
    better than the marker alone. Only the turnloop arm is checked; the baseline
    prints no such line at all.
    """
    if arm != "turnloop":
        return None
    # Prefer the per-class census: it is the only one of the two that can tell
    # a socket from a timer.
    net = net_completions(stderr_text)
    if net is not None:
        if net == 0:
            return (f"{where}: turnloop parked but carried NO net I/O "
                    f"(p1 census: 0 accept/read/write/connect completions) — "
                    f"the server declined to the tokio path, so these numbers "
                    f"are not a turnloop measurement")
        return None
    completions = marker_completions(marker_line)
    if completions is None:
        return f"{where}: marker carries no completions= field: {marker_line!r}"
    if completions == 0:
        return (f"{where}: turnloop parked but carried NO net I/O "
                f"(completions=0) — the server declined to the tokio path, so "
                f"these numbers are not a turnloop measurement")
    return None


def verify_marker(arm, binary):
    logdir = Path(tempfile.mkdtemp(prefix="server-ab-verify-"))
    server = Server(binary, free_port(), logdir)
    server.start_or_kill()
    try:
        http_get(server.port)
    finally:
        server.stop()
        shutil.rmtree(logdir, ignore_errors=True)
    if cross_enabled() and arm == "tokio":
        # A pre-migration commit. Assert the NEGATIVE: no turnloop marker and no
        # wait-metrics line. If either appears, this tree is not the baseline we
        # think it is and the comparison would be against the wrong thing.
        stray = [line for line in server.stderr_text.splitlines() if "[perry-loop]" in line]
        if stray:
            raise SystemExit(
                f"tokio arm at {CROSS['commits'].get('tokio')} printed a turnloop "
                f"marker {stray!r}: this tree is not pre-migration")
        if server.waits():
            raise SystemExit(
                f"tokio arm at {CROSS['commits'].get('tokio')} emitted wait metrics: "
                "not a pre-migration tree")
        line = f"[baseline] commit={CROSS['commits'].get('tokio')} no turnloop driver present"
        log(f"verified tokio: {line}")
        return line
    if ARM_MARKER[arm] not in server.stderr_text:
        raise SystemExit(f"{arm}: marker {ARM_MARKER[arm]!r} missing; stderr={server.stderr_text!r}")
    waits = server.waits()
    if waits.get("arm") != ARM_WAITS[arm]:
        raise SystemExit(f"{arm}: wait metrics line missing or wrong arm: {waits}")
    marker_line = pick_marker(server.stderr_text, ARM_MARKER[arm])
    if marker_line is None:
        # Unreachable: the substring check above already proved a match exists.
        # Explicit anyway, because `pick_marker` returning None where `next(...)`
        # used to raise is exactly how a verification turns into a log line.
        raise SystemExit(f"{arm}: marker present but not selectable; stderr={server.stderr_text!r}")
    # `verify_marker` served one real request above, so a turnloop arm that
    # carried the listener MUST have completions by now.
    carried = assert_turnloop_carried_io(
        arm, marker_line, f"{arm} build verification", server.stderr_text
    )
    if carried:
        raise SystemExit(carried)
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
        return pick_marker(self.stderr_text, "[perry-loop] driver=", startswith=True)


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


# `perf stat` events collected over the measured window, in report order.
# `instructions` and `cycles` are HARDWARE counters: instructions RETIRED on the
# real machine, with cache, branch-prediction and SMT effects included. They are
# not Callgrind's Ir (see `callgrind` below and the report) and the two must
# never be added up or compared.
PERF_EVENTS = [
    ("instructions", "retired instructions"),
    ("cycles", "CPU cycles"),
    ("task-clock", "CPU time on task (ms)"),
    ("context-switches", "context switches"),
    ("cpu-migrations", "CPU migrations"),
    ("page-faults", "page faults"),
    ("raw_syscalls:sys_enter", "syscalls"),
]
PERF_KEY = {
    "instructions": "instructions", "cycles": "cycles", "task-clock": "task_clock_ms",
    "context-switches": "perf_ctx_switches", "cpu-migrations": "cpu_migrations",
    "page-faults": "page_faults", "raw_syscalls:sys_enter": "syscalls",
}
PARANOID = "/proc/sys/kernel/perf_event_paranoid"


def perf_paranoid():
    try:
        return int(Path(PARANOID).read_text().strip())
    except (OSError, ValueError):
        return None


def perf_probe(perf_path):
    """Which of PERF_EVENTS this host actually lets us count.

    Returns (usable, dropped). A denied or unsupported event is NAMED with its
    reason; it is never silently left out of the report. Probed once against
    `true`, because a single `perf stat` with one denied event fails as a whole
    and would take every other counter down with it.
    """
    usable, dropped = [], {}
    for event, _ in PERF_EVENTS:
        proc = subprocess.run(
            [perf_path, "stat", "-x", ",", "-e", event, "--", "true"],
            capture_output=True, text=True)
        text = proc.stdout + proc.stderr
        if proc.returncode != 0:
            lower = text.lower()
            if "permission" in lower or "access" in lower or "not permitted" in lower:
                reason = f"permission denied (perf_event_paranoid={perf_paranoid()})"
            elif "perf list" in lower or "unknown event" in lower or "invalid event" in lower:
                reason = "unknown or unsupported event on this kernel/PMU"
            else:
                reason = (text.strip().splitlines() or ["perf stat failed"])[-1][:120]
            dropped[event] = reason
        elif "<not supported>" in text:
            dropped[event] = "not supported by this PMU (virtualised host?)"
        elif "<not counted>" in text:
            dropped[event] = "not counted (multiplexing or permission)"
        else:
            usable.append(event)
    return usable, dropped


class PerfStat:
    """`perf stat -p <server pid> -- sleep <duration>` over the measured window.

    Collects retired instructions, cycles, task-clock, context switches, CPU
    migrations, page faults and the syscall tracepoint in ONE attachment, so
    every counter covers exactly the same window as the load run.
    """

    def __init__(self, mode, perf_path, events, dropped):
        self.mode = mode
        self.perf_path = perf_path
        self.events = events
        self.dropped = dropped
        self.proc = None
        self.duration = None
        self.note = None

    @staticmethod
    def resolve(mode):
        """(mode, perf_path, events, dropped, status).

        The status string is never empty and never a bare "off": whatever the
        outcome, the report states which counters this run has and why it does
        not have the rest.
        """
        if not IS_LINUX:
            return "off", None, [], {}, f"perf is Linux-only; this host is {sys.platform}"
        paranoid = perf_paranoid()
        if mode == "off":
            return "off", None, [], {}, "disabled with --perf off: no hardware counters in this run"
        if mode == "strace":
            if shutil.which("strace"):
                return "strace", None, [], {}, ("--perf strace: syscall counts only, from a separate "
                                                "perturbed server process; no hardware counters")
            return "off", None, [], {}, "--perf strace requested but strace is not on PATH"

        perf_path = shutil.which("perf")
        reason = None
        if perf_path:
            events, dropped = perf_probe(perf_path)
            if events:
                status = f"perf ok ({len(events)}/{len(PERF_EVENTS)} events, perf_event_paranoid={paranoid})"
                if dropped:
                    status += "; dropped " + ", ".join(f"{e} ({r})" for e, r in dropped.items())
                return "perf", perf_path, events, dropped, status
            reason = (f"perf is installed but counts nothing here (perf_event_paranoid={paranoid}): "
                      + "; ".join(f"{e}: {r}" for e, r in dropped.items()))
            if mode == "perf":
                return "off", None, [], dropped, reason
        else:
            reason = f"perf is not on PATH (perf_event_paranoid={paranoid})"
            if mode == "perf":
                return "off", None, [], {}, "--perf perf requested but perf is not on PATH"
        if shutil.which("strace"):
            return "strace", None, [], {}, (reason + "; falling back to strace -c -f "
                                            "(syscalls only, perturbing, separate process)")
        return "off", None, [], {}, reason + "; no strace either, so this run has no hardware counters"

    def start(self, pid, duration):
        if self.mode != "perf":
            return
        self.duration = duration
        self.proc = subprocess.Popen(
            [self.perf_path, "stat", "-x", ",", "-e", ",".join(self.events),
             "-p", str(pid), "--", "sleep", str(duration)],
            stdout=subprocess.DEVNULL, stderr=subprocess.PIPE, text=True,
        )

    def finish(self):
        """{metric: value} for the window; {} when perf did not run.

        `perf stat -x,` emits `count,unit,event,run_ns,enabled_pct,...`. The unit
        field is EMPTY on mainline perf and `task-clock` then arrives in
        NANOSECONDS, not milliseconds — reading it as ms silently inflated
        CPU-utilisation by a factor of a million, which is why the unit is
        honoured explicitly here rather than assumed.
        """
        if self.proc is None:
            return {}
        _, err = self.proc.communicate()
        out = {}
        for line in err.splitlines():
            parts = line.split(",")
            if len(parts) < 3:
                continue
            value, unit, event = parts[0].strip(), parts[1].strip().lower(), parts[2].strip()
            key = PERF_KEY.get(event)
            if key is None:
                continue
            try:
                number = float(value)
            except ValueError:
                self.note = (self.note or "") + f"{event}={value} "
                continue
            if key == "task_clock_ms" and not unit.startswith("msec"):
                number /= 1e6  # raw counter is nanoseconds
            out[key] = number
            # A counter that was time-sliced is an estimate; say so rather than
            # quoting it as if it had been counted for the whole window.
            if len(parts) > 4:
                try:
                    if float(parts[4]) < 99.0:
                        self.note = (self.note or "") + f"{event} multiplexed at {parts[4]}% "
                except ValueError:
                    pass
        if not out:
            self.note = f"perf produced no counters: {err.strip()[-200:]}"
        return out


def derive_perf(sample, counters, duration, requests):
    """Attach the counters plus the ratios a totals-only table would hide.

    A throughput win that costs more work per request looks like an improvement
    in `instructions` alone; instructions-per-request is what says otherwise.
    """
    for key, value in counters.items():
        sample[key] = int(value) if key != "task_clock_ms" else round(value, 1)
    inst, cycles = counters.get("instructions"), counters.get("cycles")
    if inst and cycles:
        sample["ipc"] = round(inst / cycles, 3)
    if counters.get("task_clock_ms") and duration:
        sample["cpu_utilisation"] = round(counters["task_clock_ms"] / (duration * 1000.0), 3)
    if counters.get("syscalls") and duration:
        sample["syscalls_per_s"] = round(counters["syscalls"] / duration, 1)
        sample["syscalls_source"] = "perf stat -e raw_syscalls:sys_enter (measured window)"
    if not requests:
        return
    for src, dst in (("instructions", "instructions_per_req"), ("cycles", "cycles_per_req"),
                     ("syscalls", "syscalls_per_req"), ("perf_ctx_switches", "ctx_switches_per_req"),
                     ("page_faults", "page_faults_per_req")):
        if counters.get(src) is not None:
            sample[dst] = round(counters[src] / requests, 3)


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


def measure_load(arm, binary, conc, args, tool, logdir, perf):
    server = Server(binary, free_port(), logdir)
    server.start_or_kill()
    sample = {"scenario": f"load-c{conc}", "arm": arm, "concurrency": conc}
    try:
        if args.warmup:
            run_load(tool[0], tool[1], server.port, conc, args.warmup)
        counter = PerfStat(perf["mode"], perf["path"], perf["events"], perf["dropped"])
        before = proc_sample(server.pid)
        load_before = os.getloadavg()[0]
        started = time.monotonic()
        counter.start(server.pid, args.duration)
        result = run_load(tool[0], tool[1], server.port, conc, args.duration)
        wall = time.monotonic() - started
        counters = counter.finish()
        after = proc_sample(server.pid)
        sample["loadavg_after"] = os.getloadavg()[0]
        sample.update(result)
        sample.update(window_delta(before, after))
        sample["load_wall_s"] = round(wall, 3)
        sample["loadavg_before"] = load_before
        sample["perf_mode"] = perf["mode"]
        derive_perf(sample, counters, args.duration, result.get("requests"))
        if counter.note:
            sample["perf_note"] = counter.note.strip()
        if before and after and result.get("requests"):
            cpu = sum(sample[k] for k in ("win_cpu_user_s", "win_cpu_sys_s"))
            sample["cpu_us_per_req"] = round(cpu * 1e6 / result["requests"], 2)
        timing_verdict(sample, args, load_before)
    finally:
        server.stop()
    if perf["mode"] == "strace":
        rate = strace_sample(binary, tool, conc, args.strace_seconds, logdir)
        sample["syscalls_per_s"] = rate
        sample["syscalls_source"] = "strace -c -f (separate process; perturbed)"
        if rate is not None and sample.get("requests"):
            sample["syscalls_per_req"] = round(rate * args.strace_seconds / sample["requests"], 3)
    finish_sample(sample, arm, server)
    return sample


def timing_verdict(sample, args, load_before):
    """Decide whether this sample's TIMING may be quoted as authoritative.

    Counters (instructions, syscalls, page faults) are per-process and survive a
    busy host. Throughput and latency do not. A shared box can produce a clean
    `perf` table and a latency column that is pure scheduling noise, so the two
    are judged separately and the verdict travels with the sample.
    """
    reasons = []
    if args.shared_host:
        reasons.append("--shared-host: this box is shared, timing is not its job")
    elif SHARED_HOST_HINT:
        reasons.append(f"host looks like a shared build box ({SHARED_HOST_HINT})")
    # The question this gate answers is "was ANOTHER tenant competing with us",
    # not "was the machine busy" -- a load test makes the machine busy on
    # purpose. The 1-minute average does not decay between back-to-back rounds,
    # so `load_before` carries OUR previous round and gating on it directly
    # stamps every sample after the first as advisory on an idle machine. Judge
    # against the ambient load measured once, before any round ran, and allow
    # this run's own expected contribution on top of it.
    # Loadavg DURING a load test is this harness's own doing, and the 1-minute
    # average carries the previous round into the next one's "before" reading,
    # so a per-sample loadavg threshold cannot separate a neighbour from us --
    # it only measures how hard we just pushed. The honest question is whether
    # the HOST was ours alone, and that is answered by the ambient load sampled
    # before any round ran (and again after the last one settles, in run()).
    # Per-sample loadavg stays in the record as a diagnostic, not as a verdict.
    ambient = AMBIENT_LOADAVG[0]
    if ambient > args.max_loadavg:
        reasons.append(
            f"host was already at loadavg {ambient:.2f} before this run started, "
            f"above --max-loadavg {args.max_loadavg}: something else was running")
    sample["ambient_loadavg"] = ambient
    sample["timing_authoritative"] = not reasons
    sample["timing_reasons"] = reasons


def finish_sample(sample, arm, server):
    sample.update(server.lifetime())
    sample["exit_status"] = server.exit_status
    sample["forced_kill"] = server.forced_kill
    sample["marker"] = server.marker()
    waits = server.waits()
    sample["waits"] = waits
    problems = []
    if cross_enabled() and arm == "tokio":
        if sample["marker"] is not None and "[perry-loop]" in sample["marker"]:
            problems.append("baseline arm printed a turnloop marker")
        if waits:
            problems.append("baseline arm emitted wait metrics")
    else:
        if sample["marker"] is None or ARM_MARKER[arm] not in sample["marker"]:
            problems.append("arm marker missing or wrong")
        if waits.get("arm") != ARM_WAITS[arm]:
            problems.append("wait metrics missing or wrong arm")
        carried = assert_turnloop_carried_io(
            arm, sample["marker"], "sample", server.stderr_text
        )
        if carried:
            problems.append(carried)
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
    info = {"hostname": HOSTNAME, "host_role": HOST_ROLE,
            "platform": platform.platform(), "python": platform.python_version(),
            "cpus": os.cpu_count(), "loadavg": [round(v, 2) for v in os.getloadavg()],
            "nofile_soft": resource.getrlimit(resource.RLIMIT_NOFILE)[0]}
    if IS_LINUX:
        try:
            model = [l.split(":", 1)[1].strip() for l in Path("/proc/cpuinfo").read_text().splitlines()
                     if l.startswith("model name")]
            info["cpu_model"] = model[0] if model else None
        except OSError:
            pass
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
    mode, perf_path, events, dropped, perf_status = PerfStat.resolve(args.perf)
    perf = {"mode": mode, "path": perf_path, "events": events, "dropped": dropped, "status": perf_status}
    concurrency = [int(c) for c in args.concurrency.split(",") if c]
    idle = [int(n) for n in args.idle.split(",") if n]
    if tool[0] is None:
        print(INSTALL_HINTS, file=sys.stderr)
        if not args.dry_run:
            raise SystemExit(2)
    AMBIENT_LOADAVG[0] = os.getloadavg()[0]
    log(f"host: {HOSTNAME} (role {HOST_ROLE}), ambient loadavg {AMBIENT_LOADAVG[0]:.2f} "
        f"(sampled before any round; every later reading includes this run's own load)")
    if args.dry_run:
        log(f"dry-run plan: rounds={args.rounds} arms={ARMS} concurrency={concurrency} idle={idle}")
        log(f"load tool: {tool[0] or 'NONE'} ({tool[1]})")
        log(f"perf: {perf_status}")
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
    log(f"host: {HOSTNAME} (role {HOST_ROLE}); perf: {perf_status}")
    doc = {"build": build_meta, "host": host_info(), "tool": tool[0],
           "perf": {"mode": mode, "status": perf_status, "events": events, "dropped": dropped},
           "config": {"rounds": args.rounds, "concurrency": concurrency, "duration": args.duration,
                      "warmup": args.warmup, "idle": idle, "idle_hold": args.idle_hold,
                      "max_loadavg": args.max_loadavg, "shared_host": bool(args.shared_host)},
           "started": datetime.datetime.now().isoformat(), "samples": []}
    out = results_dir / "results.json"
    for rnd in range(1, args.rounds + 1):
        order = ARMS if rnd % 2 else tuple(reversed(ARMS))
        for arm in order:
            binary = build_meta["arms"][arm]["server_binary"]
            jobs = [(f"load-c{c}", lambda c=c: measure_load(arm, binary, c, args, tool, logdir, perf))
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
                if sample.get("timing_reasons"):
                    log(f"  timing ADVISORY: {'; '.join(sample['timing_reasons'])}")
                sample.update({"round": rnd, "binary_bytes": build_meta["arms"][arm]["server_binary_bytes"]})
                doc["samples"].append(sample)
                out.write_text(json.dumps(doc, indent=2))
    doc["finished"] = datetime.datetime.now().isoformat()
    # Ambient load again, after our own has had a minute to decay. Together with
    # the reading taken before the first round this brackets the whole run: quiet
    # at both ends means nobody else showed up in between, which is the claim the
    # timing verdict actually rests on.
    # 70s was not enough and this check was crying wolf. A 1-minute load average
    # has a ~60s time constant, so after 70s roughly e^-(70/60) — about a third —
    # of THIS RUN'S OWN load is still in the reading. Five consecutive runs were
    # marked ADVISORY on their own tail: ambient 1.82 before, 2.12 after, against
    # a 2.0 threshold, with nothing else having arrived.
    #
    # Two fixes, because the settle time alone would not be enough. Wait three
    # time constants so self-load decays to ~5%, and then judge the reading
    # against the PRE-RUN ambient rather than against the absolute ceiling. The
    # question this check exists to answer is "did another tenant arrive mid-run",
    # which is a CHANGE from ambient; an absolute threshold cannot tell a
    # neighbour from our own decay, and on a host whose baseline sits near the
    # ceiling it can never pass at all. The absolute ceiling still guards the
    # pre-run reading, where there is no self-load to confuse it.
    log("settling for 180s to re-read ambient load (three 1-minute time constants, so this run's own load decays to ~5%)")
    time.sleep(180)
    settled = os.getloadavg()[0]
    doc["ambient_loadavg_before"] = AMBIENT_LOADAVG[0]
    doc["ambient_loadavg_after"] = settled
    log(f"ambient loadavg: {AMBIENT_LOADAVG[0]:.2f} before the run, {settled:.2f} after it settled")
    ARRIVAL_MARGIN = 1.0
    if settled > AMBIENT_LOADAVG[0] + ARRIVAL_MARGIN:
        note = (f"host was at loadavg {settled:.2f} after the run settled, "
                f"{settled - AMBIENT_LOADAVG[0]:.2f} above the {AMBIENT_LOADAVG[0]:.2f} "
                f"it started at: another tenant arrived mid-run")
        for sample in doc["samples"]:
            sample.setdefault("timing_reasons", []).append(note)
            sample["timing_authoritative"] = False
    out.write_text(json.dumps(doc, indent=2))
    callgrind_json = results_dir / "callgrind.json"
    if callgrind_json.is_file():
        doc["callgrind"] = json.loads(callgrind_json.read_text())
    report_from(doc, results_dir)
    advisory = sorted({r for s in doc["samples"] for r in s.get("timing_reasons", [])})
    if advisory:
        log("VERDICT: throughput and latency in this run are ADVISORY — " + "; ".join(advisory))
        log(f"         the perf counters are per-process and stand. Host was {HOSTNAME} "
            f"(role {HOST_ROLE}); re-run timing on the quiet timing host.")
    else:
        log(f"VERDICT: timing is authoritative for this run (host {HOSTNAME}, role {HOST_ROLE}).")


# Timing: load-sensitive, and only quotable from a quiet host.
TIMING_METRICS = [
    ("rps", "throughput (req/s)"), ("p50_ms", "p50 latency (ms)"), ("p99_ms", "p99 latency (ms)"),
    ("p999_ms", "p999 latency (ms)"), ("load_wall_s", "load wall (s)"),
]
# `perf stat` over the measured window. ALWAYS rendered, even when perf could
# not run: a missing hardware column must say why, not disappear.
PERF_METRICS = [
    ("instructions", "retired instructions (perf)"),
    ("instructions_per_req", "retired instructions / request"),
    ("cycles", "cycles (perf)"), ("cycles_per_req", "cycles / request"),
    ("ipc", "IPC (instructions/cycle)"),
    ("task_clock_ms", "task-clock (ms)"), ("cpu_utilisation", "CPU utilisation (task-clock/wall)"),
    ("syscalls", "syscalls (perf tracepoint)"), ("syscalls_per_req", "syscalls / request"),
    ("syscalls_per_s", "syscalls/s"),
    ("perf_ctx_switches", "context switches (perf)"),
    ("ctx_switches_per_req", "context switches / request"),
    ("cpu_migrations", "CPU migrations (perf)"),
    ("page_faults", "page faults (perf)"), ("page_faults_per_req", "page faults / request"),
]
LOAD_METRICS = [
    ("win_cpu_user_s", "CPU user, window (s)"),
    ("win_cpu_sys_s", "CPU sys, window (s)"), ("cpu_us_per_req", "CPU per request (µs)"),
    ("requests", "requests completed"),
    ("win_vcsw", "voluntary ctx switches, window"),
    ("win_ivcsw", "involuntary ctx switches, window"),
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
        is_load = scenario.startswith("load")
        groups = ([("timing", TIMING_METRICS), ("perf", PERF_METRICS), ("resources", LOAD_METRICS)]
                  if is_load else [("resources", IDLE_METRICS)]) + [("waits", WAIT_METRICS)]
        rows, group_of = {}, {}
        for group, metrics in groups:
            for key, label in metrics:
                row = {"label": label, "group": group}
                for arm in ARMS:
                    values = [v for s in samples if s["arm"] == arm and (v := value_of(s, key)) is not None]
                    row[arm] = ({"median": statistics.median(values), "min": min(values), "max": max(values),
                                 "n": len(values)} if values else None)
                if row["turnloop"] and row["tokio"] and row["tokio"]["median"]:
                    row["delta_pct"] = (row["turnloop"]["median"] / row["tokio"]["median"] - 1) * 100
                else:
                    row["delta_pct"] = None
                rows[key] = row
                group_of[key] = group
        # Timing is authoritative only if EVERY contributing sample said so.
        advisory = sorted({r for s in samples for r in s.get("timing_reasons", [])})
        summary[scenario] = {"rows": rows,
                             "timing_authoritative": not advisory,
                             "timing_advisory_reasons": advisory}
    invalid = [s for s in doc["samples"] if not s.get("valid")]
    return {"scenarios": summary, "invalid_samples": len(invalid),
            "invalid_reasons": sorted({p for s in invalid for p in s.get("problems", [])}),
            "perf": doc.get("perf", {}), "host": doc.get("host", {})}


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


GROUP_HEADINGS = {
    "timing": "Timing (load-sensitive — quotable only from the quiet timing host)",
    "perf": "`perf stat` over the measured window — RETIRED instructions on real hardware",
    "resources": "Resources",
    "waits": "`PERRY_LOOP_STATS` waits",
}
MEASUREMENT_NOTE = """\
**What these numbers are.** The `perf stat` group counts instructions *retired*
on the real CPU during the measured window, with cache misses, branch
mispredictions, SMT and interrupts all included — that is cost *under load*, and
it moves with concurrency. Callgrind's `Ir` (the separate `callgrind` section,
if present) counts instructions *executed* under Valgrind's serialising
simulator with no cache or branch model, which is deterministic and
load-independent *by construction* — useful for an exact A/B of the same code
path, useless as a statement about cost under load. The two are different
quantities: never add them, never compare them, and never quote one where the
other was asked for."""


def markdown(summary, doc):
    build = doc.get("build", {})
    host = doc.get("host", {})
    perf = doc.get("perf", {})
    lines = [
        "# turnloop server A/B", "",
        f"- commit `{build.get('commit', '?')}` (dirty={build.get('dirty')}), profile `{build.get('profile')}`",
        f"- **host: `{host.get('hostname')}` (role {host.get('host_role')})** — "
        f"{host.get('platform')}, cpus={host.get('cpus')}, loadavg at start {host.get('loadavg')}"
        + (f", {host.get('cpu_model')}" if host.get("cpu_model") else ""),
        f"- load tool: {doc.get('tool')}; config: {json.dumps(doc.get('config'))}",
        f"- perf: {perf.get('status', 'not recorded')}",
        f"- invalid samples: {summary['invalid_samples']} {summary['invalid_reasons']}",
    ]
    for arm, meta in build.get("arms", {}).items():
        mt = {k: v.get("mtime_iso") for k, v in meta.get("archives", {}).items()}
        lines.append(f"- {arm}: marker `{meta.get('marker')}`, binary {meta.get('server_binary_bytes')} B, archives {mt}")
    lines += ["", MEASUREMENT_NOTE, "",
              "Median [min–max] over valid rounds; Δ = turnloop median vs pre-migration median.", ""]
    for scenario, block in summary["scenarios"].items():
        rows = block["rows"] if isinstance(block, dict) and "rows" in block else block
        lines += [f"## {scenario}", ""]
        if isinstance(block, dict) and not block.get("timing_authoritative", True):
            lines += [f"> **Timing below is ADVISORY, not authoritative** (host "
                      f"`{host.get('hostname')}`, role {host.get('host_role')}). "
                      f"{'; '.join(block.get('timing_advisory_reasons', []))}. "
                      f"Re-run throughput and latency on the quiet timing host; the "
                      f"`perf` counters are per-process and stay valid here.", ""]
        seen = set()
        for group in ("timing", "perf", "resources", "waits"):
            group_rows = [(k, r) for k, r in rows.items() if r.get("group", "resources") == group]
            if not group_rows:
                continue
            title = GROUP_HEADINGS[group]
            if group == "timing" and isinstance(block, dict) and not block.get("timing_authoritative", True):
                title += " — ADVISORY"
            lines += [f"### {title}", "", "| metric | turnloop | tokio (pre-migration) | Δ % |", "|---|---|---|---|"]
            for key, row in group_rows:
                if key in seen:
                    continue
                seen.add(key)
                empty = not row["turnloop"] and not row["tokio"]
                # A perf row is NEVER dropped for being empty: a missing hardware
                # counter has to say why, not vanish from the table.
                if empty and group != "perf":
                    continue
                if empty:
                    reason = (perf.get("dropped") or {}).get(key) or perf.get("status") or "not collected"
                    lines.append(f"| {row['label']} | n/a | n/a | not collected: {reason} |")
                    continue
                delta = "–" if row["delta_pct"] is None else f"{row['delta_pct']:+.1f}"
                lines.append(f"| {row['label']} | {fmt(row['turnloop'])} | {fmt(row['tokio'])} | {delta} |")
            if group == "perf" and perf.get("mode") != "perf":
                lines.append("")
                lines.append(f"*No hardware counters in this run: {perf.get('status', 'perf did not run')}.*")
            lines.append("")
    if doc.get("callgrind"):
        lines += callgrind_markdown(doc["callgrind"])
    return "\n".join(lines)


# ─── callgrind: instructions EXECUTED, deterministic, microbenchmarks only ───

CALLGRIND_PROBES = sorted((ROOT / "test-files").glob("test_turnloop_p0_*.ts"))
IR_RE = re.compile(r"I\s+refs:\s+([\d,]+)")

CALLGRIND_NOTE = """\
Callgrind `Ir` = instructions **executed** under Valgrind's serialising
simulator. No cache model, no branch predictor, no SMT, no interrupts, one
thread at a time — so it is deterministic and **load-independent by
construction**. That is exactly what makes it a good A/B of the same code path
and a bad statement about cost under load. It is NOT the `instructions` counter
in the load table, which is instructions *retired* on real hardware under real
concurrency. Do not add them and do not substitute one for the other.

**Server workload: not run under Callgrind, deliberately.** Valgrind costs
roughly 50-100x, so a load generator's connections time out and the event loop's
time moves almost entirely into waits that scale with wall-clock rather than
with request handling. The resulting `Ir` would describe an artificial wait
pattern, not the server. Callgrind here covers the timer/promise
microbenchmarks, where the measured code path is the park itself and the run is
short enough to simulate honestly."""


def callgrind_run(arm, perry, runtime_dir, probe, work, valgrind):
    """One probe under callgrind for one arm. Returns a row (never raises)."""
    binary = work / f"cg-{arm}-{probe.stem}"
    env = dict(os.environ, PERRY_RUNTIME_DIR=str(runtime_dir), PERRY_NO_AUTO_OPTIMIZE="1")
    compile_proc = subprocess.run([str(perry), str(probe), "--no-cache", "-o", str(binary)],
                                  cwd=ROOT, env=env, capture_output=True, text=True)
    if compile_proc.returncode != 0:
        return {"arm": arm, "probe": probe.stem, "error": (compile_proc.stdout + compile_proc.stderr)[-300:]}
    out = work / f"cg-{arm}-{probe.stem}.callgrind"
    proc = subprocess.run(
        [valgrind, "--tool=callgrind", "--callgrind-out-file=" + str(out), str(binary)],
        capture_output=True, text=True, env=dict(os.environ, PERRY_LOOP_STATS="1"))
    match = IR_RE.search(proc.stderr)
    row = {"arm": arm, "probe": probe.stem, "exit": proc.returncode}
    if match:
        row["ir"] = int(match.group(1).replace(",", ""))
    else:
        row["error"] = (proc.stderr or proc.stdout)[-300:]
    return row


def callgrind(args):
    work = Path(args.work).resolve()
    valgrind = args.valgrind or shutil.which("valgrind")
    probes = [p for p in CALLGRIND_PROBES
              if not args.probes or p.stem in args.probes.split(",")]
    if args.dry_run:
        log(f"dry-run: callgrind on {len(probes)} probe(s) x {len(ARMS)} arms; "
            f"valgrind={valgrind or 'NOT FOUND'}")
        for probe in probes:
            log(f"  {valgrind or 'valgrind'} --tool=callgrind <arm compiler output of {probe.name}>")
        print(CALLGRIND_NOTE)
        return
    if not valgrind:
        raise SystemExit(
            "valgrind is not installed (and does not exist for arm64 macOS).\n"
            "Run this on the Linux box: apt install valgrind. "
            "The load table's perf counters are the load-dependent measurement; "
            "this arm is the deterministic one and is optional.")
    build_meta = json.loads((work / "build.json").read_text())
    rows = []
    for probe in probes:
        for arm in ARMS:
            meta = build_meta["arms"][arm]
            out_dir = Path(meta["target_dir"])
            log(f"callgrind {arm}: {probe.name}")
            rows.append(callgrind_run(arm, out_dir / "perry", out_dir, probe, work, valgrind))
    doc = {"tool": "callgrind", "valgrind": valgrind, "host": host_info(),
           "commit": build_meta.get("commit"), "rows": rows,
           "when": datetime.datetime.now().isoformat()}
    results_dir = work / "results"
    results_dir.mkdir(parents=True, exist_ok=True)
    (results_dir / "callgrind.json").write_text(json.dumps(doc, indent=2))
    text = "\n".join(callgrind_markdown(doc))
    (results_dir / "callgrind.md").write_text(text)
    log(f"wrote {results_dir / 'callgrind.md'}")
    print(text)


def callgrind_markdown(doc):
    lines = ["## Callgrind arm — instructions EXECUTED (Valgrind `Ir`), NOT retired", "",
             CALLGRIND_NOTE, "",
             f"- valgrind: `{doc.get('valgrind')}`, host `{doc.get('host', {}).get('hostname')}`, "
             f"commit `{doc.get('commit')}`", "",
             "| probe | turnloop Ir | tokio (pre-migration) Ir | Δ % |", "|---|---|---|---|"]
    by_probe = {}
    for row in doc.get("rows", []):
        by_probe.setdefault(row["probe"], {})[row["arm"]] = row
    for probe, arms in sorted(by_probe.items()):
        a, b = arms.get("turnloop", {}), arms.get("tokio", {})
        if "ir" in a and "ir" in b and b["ir"]:
            delta = f"{(a['ir'] / b['ir'] - 1) * 100:+.2f}"
        else:
            delta = "–"
        def cell(row):
            return f"{row['ir']:,}" if "ir" in row else f"FAILED ({row.get('error', '?')[:60]})"
        lines.append(f"| {probe} | {cell(a)} | {cell(b)} | {delta} |")
    lines.append("")
    return lines


def report_from(doc, results_dir):
    summary = summarize(doc)
    (results_dir / "summary.json").write_text(json.dumps(summary, indent=2))
    (results_dir / "summary.md").write_text(markdown(summary, doc))
    log(f"wrote {results_dir / 'summary.md'} and summary.json")
    print(markdown(summary, doc))


def report(args):
    results_dir = Path(args.work).resolve() / "results"
    doc = json.loads((results_dir / "results.json").read_text())
    # Fold in a Callgrind run if one exists. It stays its OWN section with its
    # own caveat; it is never merged into the perf numbers.
    callgrind_json = results_dir / "callgrind.json"
    if callgrind_json.is_file():
        doc["callgrind"] = json.loads(callgrind_json.read_text())
        log("including the separate Callgrind (instructions executed) section")
    report_from(doc, results_dir)


def synthetic_report(args, perf_status="synthetic"):
    """Dry-run: drive the summary/markdown code on generated samples.

    Covers both halves of the perf column deliberately: `load-c64` has full
    counters, `load-c1` has none, so the "an absent hardware counter must say
    why instead of vanishing" path is exercised on macOS where perf cannot run.
    """
    doc = {"build": {"commit": git("rev-parse", "HEAD"), "dirty": False, "profile": args.profile, "arms": {}},
           "host": host_info(),
           "tool": "synthetic",
           "perf": {"mode": "perf", "status": perf_status,
                    "dropped": {"raw_syscalls:sys_enter": "permission denied"}},
           "config": {}, "samples": []}
    for rnd in range(1, 4):
        for arm in ARMS:
            base = 1.0 if arm == "turnloop" else 1.1
            waits = {"arm": ARM_WAITS[arm], "tokio_ticks": 1000 * rnd, "tokio_tick_ns": 5_000_000 * rnd,
                     "turnloop_waits": 10 if arm == "turnloop" else 0, "wake_samples": 900, "wake_lt50us": 800,
                     "wake_lt200us": 90, "wake_lt1ms": 10, "wake_lt5ms": 0, "wake_ge5ms": 0}
            requests = int(750000 / base)
            counters = {"instructions": 3.0e11 * base, "cycles": 1.5e11 * base, "task_clock_ms": 14000.0,
                        "perf_ctx_switches": 12000, "cpu_migrations": 40, "page_faults": 9000,
                        "syscalls": 3.0e6 * base}
            load = {
                "scenario": "load-c64", "arm": arm, "round": rnd, "valid": True, "rps": 50000 / base + rnd,
                "requests": requests,
                "p50_ms": 0.5 * base, "p99_ms": 2.0 * base, "p999_ms": 5.0 * base, "win_cpu_user_s": 10.0 * base,
                "win_cpu_sys_s": 3.0, "cpu_us_per_req": 17.0 * base, "load_wall_s": 15.0, "win_vcsw": 1000,
                "win_ivcsw": 10, "rss_peak_kb": 20000, "cpu_user_s": 11.0,
                "cpu_sys_s": 3.2, "vcsw": 1200, "ivcsw": 12, "binary_bytes": 10_000_000,
                "timing_authoritative": True, "timing_reasons": [], "waits": waits}
            derive_perf(load, counters, 15.0, requests)
            doc["samples"].append(load)
            # A second scenario with NO perf counters and a busy host: proves the
            # advisory banner and the "n/a, and here is why" perf rows render.
            doc["samples"].append({
                "scenario": "load-c1", "arm": arm, "round": rnd, "valid": True, "rps": 9000 / base,
                "requests": int(135000 / base), "p50_ms": 0.1 * base, "p99_ms": 0.4 * base,
                "load_wall_s": 15.0, "rss_peak_kb": 19000, "binary_bytes": 10_000_000,
                "timing_authoritative": False,
                "timing_reasons": ["loadavg 9.10 > --max-loadavg 1.5"], "waits": waits})
            doc["samples"].append({
                "scenario": "idle-10000", "arm": arm, "round": rnd, "valid": True, "open_after_hold": 10000,
                "rss_before_kb": 8000, "rss_after_kb": 48000, "bytes_per_conn": 4096.0 * base,
                "idle_cpu_ms": 2.0, "idle_vcsw": 20, "rss_peak_kb": 50000, "threads": 4, "waits": waits})
    doc["samples"].append({"scenario": "load-c64", "arm": "tokio", "round": 9, "valid": False,
                           "problems": ["arm marker missing or wrong"]})
    doc["callgrind"] = {
        "valgrind": "/usr/bin/valgrind", "host": host_info(), "commit": "synthetic",
        "rows": [{"arm": "turnloop", "probe": "test_turnloop_p0_idle", "ir": 41_000_000},
                 {"arm": "tokio", "probe": "test_turnloop_p0_idle", "ir": 41_500_000}],
    }
    summary = summarize(doc)
    text = markdown(summary, doc)
    assert summary["invalid_samples"] == 1
    assert summary["scenarios"]["load-c64"]["rows"]["rps"]["turnloop"]["n"] == 3
    assert summary["scenarios"]["load-c64"]["timing_authoritative"] is True
    assert summary["scenarios"]["load-c1"]["timing_authoritative"] is False
    assert "| throughput (req/s) |" in text and "## idle-10000" in text
    # per-request normalisation, both directions
    assert "| retired instructions / request |" in text and "| syscalls / request |" in text
    assert "| IPC (instructions/cycle) |" in text
    # a perf row with no data must still appear, with its reason
    assert "| retired instructions (perf) | n/a | n/a |" in text
    # the advisory banner and the Callgrind separation
    assert "Timing below is ADVISORY" in text
    assert "instructions EXECUTED (Valgrind `Ir`), NOT retired" in text
    assert "RETIRED instructions" in text
    log("dry-run: synthetic summary OK (perf rows, per-request rows, advisory banner, "
        "callgrind section all rendered); first lines:")
    print("\n".join(text.splitlines()[:18]))


# ─── cli ────────────────────────────────────────────────────────────────────


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = parser.add_subparsers(dest="command", required=True)

    def common(p):
        p.add_argument("--work", default=str(ROOT / "target/turnloop-server-ab"))
        p.add_argument("--dry-run", action="store_true")
        p.add_argument("--profile", default="release")
        p.add_argument("--arm-tree", action="append", default=[], metavar="ARM=PATH",
                       help="cross-commit mode: build ARM from an already-built tree at "
                            "PATH instead of from HEAD with a feature flag. Give it twice "
                            "(tokio=... and turnloop=...). The tokio arm is then expected "
                            "to be a PRE-MIGRATION commit: it must print no [perry-loop] "
                            "marker and no wait metrics, and that absence is verified.")

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
        p.add_argument("--perf", choices=["auto", "perf", "strace", "off"], default="auto",
                       help="hardware counters over the measured window (default auto: perf, "
                            "else strace for syscalls only, else nothing — always stated in the report)")
        p.add_argument("--strace-seconds", type=int, default=5)
        p.add_argument("--max-loadavg", type=float, default=2.0,
                       help="above this 1-minute loadavg at the start of a window, that sample's "
                            "THROUGHPUT and LATENCY are marked advisory (counters stay valid). "
                            "The default leaves room for a host's own daemons; measure your "
                            "timing host at rest and set it just above that")
        p.add_argument("--shared-host", action="store_true",
                       help="this box is shared: never quote its timing as authoritative "
                            "(auto-detected for hosts named like a build box)")

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
    p_cg = sub.add_parser("callgrind", help="deterministic instructions EXECUTED (Valgrind Ir) "
                                            "for the timer/promise microbenchmarks; Linux only")
    common(p_cg)
    p_cg.add_argument("--probes", default="",
                      help="comma-separated probe stems (default: every test_turnloop_p0_*.ts)")
    p_cg.add_argument("--valgrind")
    p_idle = sub.add_parser("idle-client")
    p_idle.add_argument("--port", type=int, required=True)
    p_idle.add_argument("--count", type=int, required=True)
    p_idle.add_argument("--sources", default="127.0.0.1")
    p_idle.add_argument("--batch", type=int, default=512)
    p_idle.add_argument("--timeout", type=float, default=15.0)

    args = parser.parse_args()
    for spec in getattr(args, "arm_tree", []) or []:
        if "=" not in spec:
            raise SystemExit(f"--arm-tree wants ARM=PATH, got {spec!r}")
        arm, _, path = spec.partition("=")
        if arm not in ARMS:
            raise SystemExit(f"--arm-tree: unknown arm {arm!r}; want one of {ARMS}")
        tree = Path(path).expanduser().resolve()
        if not (tree / ".git").exists() and not (tree / "target").exists():
            raise SystemExit(f"--arm-tree {arm}: {tree} is neither a checkout nor a target tree")
        CROSS["enabled"] = True
        CROSS["trees"][arm] = tree
        CROSS["commits"][arm] = git_in(tree, "rev-parse", "--short", "HEAD") or "unknown"
        dirty = git_in(tree, "status", "--porcelain", "--untracked-files=no")
        if dirty:
            raise SystemExit(
                f"--arm-tree {arm}: {tree} has uncommitted changes, so the commit it "
                "reports is not what it would measure")
    if CROSS["enabled"] and set(CROSS["trees"]) != set(ARMS):
        raise SystemExit(f"--arm-tree: give a tree for BOTH arms, got {sorted(CROSS['trees'])}")
    if CROSS["enabled"]:
        # Cross-commit arms are already built, at two different commits. Building
        # here would be wrong twice over: it would rebuild trees that are the
        # measured artifact, and for the tokio arm it would ADD
        # perry-stdlib/tokio-wait-driver to a pre-migration commit that has no
        # such feature. Force the skip rather than trusting the caller.
        if not getattr(args, "skip_cargo", False):
            log("cross-commit: forcing --skip-cargo (the arms are prebuilt at their own commits)")
        args.skip_cargo = True
        log("cross-commit arms: " + ", ".join(
            f"{a}={CROSS['commits'][a]} ({CROSS['trees'][a]})" for a in ARMS))
    if args.command == "idle-client":
        idle_client(args)
    elif args.command == "build":
        build(args)
    elif args.command == "run":
        run(args)
    elif args.command == "report":
        report(args)
    elif args.command == "callgrind":
        callgrind(args)
    elif args.command == "all":
        build(args)
        run(args)


if __name__ == "__main__":
    main()
