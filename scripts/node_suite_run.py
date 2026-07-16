#!/usr/bin/env python3
"""Differential runner for the print-and-diff node-suite (test-parity/node-suite).

For every `test-parity/node-suite/<module>/**/*.ts`, run `node <t>` and
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

Usage: node_suite_run.py <perry-bin> <repo-root> [comma-separated-modules]
"""
import os, re, subprocess, sys, tempfile
from concurrent.futures import ThreadPoolExecutor
from collections import defaultdict

# Environment-variant tokens that defeat byte-for-byte stdout comparison without
# indicating any Perry defect. Scrubbed SYMMETRICALLY from both node and perry
# output before the equality check (see normalize()).
#
#   1. console.time/timeEnd/timeLog durations. Node's formatTime renders a live
#      hrtime delta as the raw float `${ms}ms` (or `${s}s`, or `m:ss.mmm`), so the
#      number is non-deterministic run-to-run AND has a variable decimal count —
#      node itself prints e.g. 0.004ms / 0.004ms / 0.003ms, and 2.59ms / 2.68ms,
#      across runs of the same program. The duration always follows the timer
#      label as `<label>: <dur>`, so we anchor on `: ` (a lookbehind) and mask the
#      numeric value while KEEPING the unit — a dropped `ms`, a wrong label, or a
#      missing trailing arg still surfaces as a diff, and the `: ` anchor keeps
#      ordinary numbers elsewhere in the output untouched.
#   2. Stack-trace frames. console.trace, thrown-error inspection, and `[cause]`
#      blocks print `    at <path>:<line>:<col>` plus node-internal ESM-loader
#      frames Perry cannot reproduce (and Perry's own native-symbol /
#      `(… N more identical frames)` placeholders). Paths and line numbers vary by
#      machine, so whole frame lines are dropped. Frames are always indented >=4
#      spaces, which keeps ordinary 2-space-per-level inspect output untouched.
_DUR_MS = re.compile(r"(?<=: )\d+(?:\.\d+)?(ms|s)\b")
_DUR_CLOCK = re.compile(r"\b\d+:\d{2}\.\d{3} \((h:mm|m):ss\.mmm\)")
_FRAME = re.compile(r"^\s{4,}(at\s|\d+:\s)|… \d+ more identical frames")


def normalize(text: str) -> str:
    out = []
    for line in text.split("\n"):
        if _FRAME.search(line):
            continue
        line = _DUR_MS.sub(lambda m: "<dur>" + m.group(1), line)
        line = _DUR_CLOCK.sub(lambda m: "<dur:" + m.group(1) + ">", line)
        out.append(line)
    return "\n".join(out)


PERRY = sys.argv[1]
ROOT = sys.argv[2]
MODS = sys.argv[3].split(",") if len(sys.argv) > 3 and sys.argv[3] else None
NODE = os.environ.get("NODE_BIN", "node")

# Modules that must run one-at-a-time (port binding / process spawn / event-loop
# or timer ordering). Parallelism corrupts their results.
SLOW_MODULES = {
    "http", "http2", "https", "net", "dgram", "tls", "cluster", "dns",
    "stream", "child_process", "worker_threads", "inspector",
    "inspector-promises", "repl", "diagnostics_channel", "timers", "perf_hooks", "fetch",
}

tests = []
base = os.path.join(ROOT, "test-parity", "node-suite")
for mod in (MODS or sorted(os.listdir(base))):
    md = os.path.join(base, mod)
    if not os.path.isdir(md):
        continue
    for dp, _, files in os.walk(md):
        for f in files:
            if f.endswith(".ts") and not f.endswith(".d.ts"):
                tests.append((mod, os.path.join(dp, f)))


def run_one(args):
    mod, path = args
    try:
        n = subprocess.run([NODE, path], capture_output=True, text=True, timeout=30)
    except Exception:
        return (mod, "node_err")
    # A non-zero node exit can be intentional (the test exercises an error path),
    # so we don't bucket it as node_err; we require Perry to match BOTH stdout and
    # the exit code below, which keeps genuine error-path parity counted as pass.
    with tempfile.TemporaryDirectory() as td:
        out = os.path.join(td, "o")
        try:
            c = subprocess.run([PERRY, path, "-o", out], capture_output=True, text=True, timeout=120)
            if c.returncode != 0:
                return (mod, "compile_fail")
            p = subprocess.run([out], capture_output=True, text=True, timeout=30)
        except Exception:
            return (mod, "perry_err")
    # Match stdout byte-for-byte (ignore only trailing-newline noise, not leading
    # whitespace) AND exit code — so a Perry crash that happened to print matching
    # output before dying is a diff, not a false pass.
    ok = (normalize(n.stdout.rstrip("\n")) == normalize(p.stdout.rstrip("\n"))) and (n.returncode == p.returncode)
    return (mod, "pass" if ok else "diff")


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
    for mod, outcome in ex.map(run_one, fast):
        res[mod][outcome] += 1
for t in slow:
    mod, outcome = run_one(t)
    res[mod][outcome] += 1

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
