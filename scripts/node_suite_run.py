#!/usr/bin/env python3
"""Differential runner for the print-and-diff node-suite (test-parity/node-suite).

For every `test-parity/node-suite/<module>/**/*.ts` outside a `fixtures/`
directory, run `node <t>` and
`perry <t> -o out && out`, then compare stdout (trailing whitespace ignored).
Prints a per-module pass/total table plus an overall figure.

Two correctness measures learned the hard way (see CHANGELOG / project memory):

1. Pre-warm pass — compile one test per module SERIALLY first so each module's
   auto-optimize runtime/stdlib cache (e.g. the crypto feature) is built before
   the timed run. Otherwise the first test of a crypto-feature module eats a
   multi-minute cold rebuild that blows the per-test compile timeout and the
   whole module is scored as `perry_err` (this once made dns look 0% and http
   47% when both are actually 100%).

2. Low-concurrency lane — server/timing modules bind ports, spawn processes, or
   assert on event-loop/timer ordering. Under the wide parallel pool they suffer
   port contention and timing races, producing false `perry_err`/`diff`. They
   run STRICTLY SEQUENTIALLY so their numbers are trustworthy; everything else
   stays parallel.

A `fixtures/` directory holds modules that tests import, never tests: its `.ts`
files print nothing, so counting them would only add trivial passes.

Usage: node_suite_run.py <perry-bin> <repo-root> [comma-separated-modules]
       node_suite_run.py --self-test
"""
import os, re, subprocess, sys, tempfile
from concurrent.futures import ThreadPoolExecutor
from collections import defaultdict

# Environment-variant tokens that defeat a byte-for-byte stdout compare without
# indicating any Perry defect are scrubbed SYMMETRICALLY from both sides before
# the equality check. That rule set is not private to this runner: it lives in
# `scripts/parity_normalize.py`, together with the (deliberately different) rule
# set `run_parity_tests.sh` uses, so the two answers to "did Perry print what
# Node printed" can never drift apart unnoticed. The `node_suite` profile keeps
# the console-timer, clock-format, stack-frame and node:test-reporter masking
# described in that module.
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from parity_normalize import normalize_node_suite_output as normalize  # noqa: E402


FIXTURE_DIR = "fixtures"


def discover(base, mods=None):
    """(module, path) for every test entry under `base`, skipping `fixtures/`."""
    found = []
    for mod in (mods or sorted(os.listdir(base))):
        md = os.path.join(base, mod)
        if not os.path.isdir(md):
            continue
        for dp, dirs, files in os.walk(md):
            dirs[:] = [d for d in dirs if d != FIXTURE_DIR]
            for f in files:
                if f.endswith(".ts") and not f.endswith(".d.ts"):
                    found.append((mod, os.path.join(dp, f)))
    return found


def self_test():
    with tempfile.TemporaryDirectory() as td:
        for rel in ("m/a.ts", "m/sub/b.ts", "m/sub/fixtures/dep.ts",
                    "m/fixtures/nested/dep.ts", "m/types.d.ts", "n/c.ts"):
            path = os.path.join(td, rel)
            os.makedirs(os.path.dirname(path), exist_ok=True)
            open(path, "w").close()
        got = sorted(os.path.relpath(p, td) for _, p in discover(td))
        want = ["m/a.ts", "m/sub/b.ts", "n/c.ts"]
        if got != want:
            print(f"FAIL: discovered {got}, want {want}")
            return 1
    print("OK: node-suite discovery skips fixtures/ and .d.ts")
    return 0


if len(sys.argv) > 1 and sys.argv[1] == "--self-test":
    sys.exit(self_test())

PERRY = sys.argv[1]
ROOT = sys.argv[2]
MODS = sys.argv[3].split(",") if len(sys.argv) > 3 and sys.argv[3] else None
NODE = os.environ.get("NODE_BIN", "node")
COMPILE_TIMEOUT = int(os.environ.get("PERRY_NODE_SUITE_COMPILE_TIMEOUT", "120"))

# Modules that must run one-at-a-time (port binding / process spawn / event-loop
# or timer ordering). Parallelism corrupts their results.
SLOW_MODULES = {
    "http", "http2", "https", "net", "dgram", "tls", "cluster", "dns", "async_hooks",
    "stream", "child_process", "worker_threads", "inspector",
    "inspector-promises", "repl", "diagnostics_channel", "timers", "perf_hooks", "fetch",
    "v8",
    "trace_events",
}

tests = discover(os.path.join(ROOT, "test-parity", "node-suite"), MODS)


def run_one(args):
    mod, path = args
    try:
        n = subprocess.run([NODE, path], capture_output=True, text=True, timeout=30)
    except Exception:
        return (mod, "node_err", path)
    # A non-zero node exit can be intentional (the test exercises an error path),
    # so we don't bucket it as node_err; we require Perry to match BOTH stdout and
    # the exit code below, which keeps genuine error-path parity counted as pass.
    with tempfile.TemporaryDirectory() as td:
        out = os.path.join(td, "o")
        try:
            c = subprocess.run(
                [PERRY, path, "-o", out],
                capture_output=True,
                text=True,
                timeout=COMPILE_TIMEOUT,
            )
            if c.returncode != 0:
                return (mod, "compile_fail", path)
            p = subprocess.run([out], capture_output=True, text=True, timeout=30)
        except Exception:
            return (mod, "perry_err", path)
    # Match stdout byte-for-byte (ignore only trailing-newline noise, not leading
    # whitespace) AND exit code — so a Perry crash that happened to print matching
    # output before dying is a diff, not a false pass.
    ok = (normalize(n.stdout.rstrip("\n")) == normalize(p.stdout.rstrip("\n"))) and (n.returncode == p.returncode)
    return (mod, "pass" if ok else "diff", path)


# --- pre-warm one test per module serially ---
seen = set()
warm = [t for t in tests if t[0] not in seen and not seen.add(t[0])]
sys.stderr.write(f"pre-warming auto-opt cache for {len(warm)} module(s)...\n")
sys.stderr.flush()
for mod, path in warm:
    with tempfile.TemporaryDirectory() as td:
        try:
            subprocess.run([PERRY, path, "-o", os.path.join(td, "o")], capture_output=True, text=True, timeout=600)
        except Exception:
            pass
sys.stderr.write("pre-warm done\n")
sys.stderr.flush()

# --- fast lane (parallel) + slow lane (sequential) ---
fast = [t for t in tests if t[0] not in SLOW_MODULES]
slow = [t for t in tests if t[0] in SLOW_MODULES]
res = defaultdict(lambda: defaultdict(int))
sys.stderr.write(f"fast lane: {len(fast)} tests @6, slow lane: {len(slow)} tests @1\n")
sys.stderr.flush()
with ThreadPoolExecutor(max_workers=6) as ex:
    for mod, outcome, path in ex.map(run_one, fast):
        res[mod][outcome] += 1
        if outcome not in {"pass", "diff"}:
            sys.stderr.write(f"{outcome}: {os.path.relpath(path, ROOT)}\n")
for t in slow:
    mod, outcome, path = run_one(t)
    res[mod][outcome] += 1
    if outcome not in {"pass", "diff"}:
        sys.stderr.write(f"{outcome}: {os.path.relpath(path, ROOT)}\n")

# --- report ---
tot_p = tot = 0
print("%-20s %6s %6s  %5s" % ("module", "pass", "total", "%"))
for mod in sorted(res):
    a = res[mod]
    p = a.get("pass", 0)
    t = sum(a.values())
    tot_p += p
    tot += t
    extra = " ".join(f"{k}={v}" for k, v in a.items() if k != "pass" and v)
    print("%-20s %6d %6d  %5.1f  %s" % (mod, p, t, 100 * p / t if t else 0, extra))
print("-" * 44)
print("OVERALL node-suite: %d/%d (%.1f%%)" % (tot_p, tot, 100 * tot_p / tot if tot else 0))
