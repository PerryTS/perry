#!/usr/bin/env python3
"""Run a subset of Node.js's own `test/parallel` corpus under both Perry and
Node, bucket the divergences, and write a JSON report (#800).

This is a *coverage radar*, not a gate. Where the hand-authored
`test-parity/node-suite` cases probe whatever a human thought to write, this
runner pulls Node's own tests for each API in `supported-apis.txt` — the
canonical definition of correct behaviour, and exactly the corpus Deno and
Bun lean on for their Node-compat suites.

Model
-----
Node's `test/parallel` cases are silent on success and `throw` (exit != 0) on
failure, so the primary signal is **exit-code parity**, with stdout as a
secondary tiebreak. Each case `require('../common')` — Node's ~1000-line test
harness that Perry can't compile — so we stage a Perry-compilable shim
(`test-compat/node-core/shim/`) as `common/` next to each test. BOTH runtimes
use the shim, so the differential still compares the two runtimes' *builtins*,
never their private harnesses.

Buckets
-------
- pass         — Node exits 0, Perry exits 0, stdout matches.
- diff         — both exit 0 but stdout differs.
- runtime-fail — Perry compiled but exited non-zero while Node passed.
- compile-fail — Perry refused to compile (parser / lower / codegen).
- node-skip    — Node itself failed under the shim (missing helper, needs a
                 flag/env, or genuinely env-dependent). Excluded from the
                 Perry verdict — never charged against Perry.

Usage
-----
    scripts/node_core_subset.py --root vendor/nodejs
    scripts/node_core_subset.py --root vendor/nodejs --api path url
    scripts/node_core_subset.py --root vendor/nodejs --max-per-api 25
    scripts/node_core_subset.py --root vendor/nodejs --api http net --auto-optimize

Feature-gated APIs (#1778, #2156)
---------------------------------
By default the radar compiles with `PERRY_NO_AUTO_OPTIMIZE=1`, a speed hack
that links the prebuilt full-feature `target/release/libperry_*.a` instead of
rebuilding a per-program runtime. But Perry's http/net/https/ws *servers*,
zlib, crypto and async_hooks live in `perry-ext-*` crates / Cargo features
that are only built + added to the link line by the **auto-optimize** path.
With that path skipped, those tests either fail to *link*
(`Undefined symbols: _js_node_http_create_server`, …) and get mis-bucketed
as `compile-fail` (#1778), OR — for the symbols compile.rs's stub-generator
covers — compile *successfully* with a stub returning `undefined`, then
fail at runtime with `undefined.listen` and land in `runtime-fail` (#2156).
Both shapes hide real parity for http/https/net/zlib/events (~570 tests in
the full sweep).

The radar runner therefore enables auto-optimize **per API** for the APIs
whose well-known binding routes to a `perry-ext-*` crate
(`_AUTO_OPTIMIZE_APIS` below — currently events / http / https / net / zlib).
`--auto-optimize` extends that to every API. Either flavour pre-warms the
ext-crate libs once (kitchen-sink import) so the first per-feature relink in
the sweep is incremental, and bumps the per-compile timeout to absorb the
cold cargo build (see `--compile-timeout`). Restrict with `--api` to keep
the sweep tractable.

Bounding the sweep (#6305)
--------------------------
The nightly radar never once completed: every scheduled run died mid-sweep with
exit 143, and the runner's own `system.txt` gave the reason —

    ##[error]The runner has received a shutdown signal.
    ##[error]Process completed with exit code 143.

i.e. the hosted VM was reclaimed out from under the job, not stopped by
`timeout-minutes`. When that happens *no* later step runs (even `if: always()`
ones), and since the report was only written after the final API, the run
uploaded nothing at all — the radar read as "no signal" rather than "broken".

Three things kept the sweep unbounded, and all three are fixed here:

1. **No wall-clock bound.** One serial job swept 23 APIs x <=25 tests, ~125 of
   which are auto-optimize compiles that each pay a cargo relink of the Perry
   runtime. `--shard I/N` splits the API list across N jobs (cost-aware: the
   auto-optimize APIs are round-robined FIRST so no shard draws two — a
   contiguous split would put http/https/net/zlib, adjacent in
   `supported-apis.txt`, in one shard), and `--time-budget SECS` stops the sweep
   at a deadline instead of running until something else kills it. `--merge`
   reduces the per-shard reports back to one aggregate.

2. **Leaked grandchildren.** `subprocess.run(timeout=...)` kills only the direct
   child. Node's core tests fork servers, workers and `child_process` helpers,
   so every timed-out test leaked a live process tree that kept holding CPU,
   memory and sockets for the rest of the sweep (the runner logs "Cleaning up
   orphan processes" at kill time). `run()` now puts each child in its own
   process group and SIGKILLs the whole group on timeout.

3. **Nothing on disk until the end.** The report is now rewritten after every
   API, so even a hard kill leaves a partial, mergeable report behind.

A sweep that is cut short reports `complete: false` and exits non-zero: an
incomplete radar must fail loudly, not quietly publish a number that looks whole.

See test-compat/node-core/README.md.
"""

from __future__ import annotations

import argparse
import json
import os
import re
import shutil
import signal
import subprocess
import sys
import tempfile
import time
from dataclasses import dataclass, field
from pathlib import Path

SCRIPT_DIR = Path(__file__).resolve().parent
REPO_ROOT = SCRIPT_DIR.parent
NODE_CORE_DIR = REPO_ROOT / "test-compat" / "node-core"
SHIM_DIR = NODE_CORE_DIR / "shim"

# APIs whose well-known binding (see `crates/perry/well_known_bindings.toml`)
# routes to a `perry-ext-*` crate. Under PERRY_NO_AUTO_OPTIMIZE the
# well-known flip is skipped, so symbols from these crates either link-fail
# (#1778) or fall through to the compile.rs symbol-stub generator and run as
# `undefined` (#2156). Either way the radar's bucket counts lie. For these
# APIs the runner drops PERRY_NO_AUTO_OPTIMIZE and pays the per-feature
# cargo rebuild (cached after the first compile, plus the global prewarm).
_AUTO_OPTIMIZE_APIS: frozenset[str] = frozenset({
    "events", "http", "https", "net", "zlib",
})

# Report buckets, and the subset of them that carry failing-test samples.
_BUCKETS = ("pass", "diff", "runtime-fail", "compile-fail", "node-skip")
_SAMPLE_BUCKETS = ("diff", "runtime-fail", "compile-fail", "node-skip")


# Lines that are pure environmental noise from either runtime — stripped
# before the stdout tiebreak so a warning never registers as a "diff".
_NOISE = re.compile(
    r"^\(node:\d+\) (ExperimentalWarning|Warning|\[DEP\d+\]|\[MODULE_TYPELESS)"
    r"|^\(Use `node --trace"
)

# The `(node:<pid>)` prefix on a process warning carries a per-run pid that is
# pure environment noise. Canonicalize it so a warning line that survives the
# `_NOISE` filter (e.g. a `TimeoutOverflowWarning`, which is not a generic
# `Warning`) compares by message content, not by pid. (#4910)
_PID_PREFIX = re.compile(r"^\(node:\d+\)")


def normalize(text: str) -> str:
    out = []
    for raw in text.replace("\r\n", "\n").split("\n"):
        line = raw.rstrip()
        if _NOISE.search(line):
            continue
        line = _PID_PREFIX.sub("(node:PID)", line)
        out.append(line)
    while out and out[-1] == "":
        out.pop()
    return "\n".join(out)


def read_api_list(path: Path) -> list[str]:
    apis = []
    for line in path.read_text().splitlines():
        line = line.strip()
        if line and not line.startswith("#"):
            apis.append(line)
    return apis


def resolve_tests(root: Path, api: str) -> list[Path]:
    """`test/parallel/test-<api>-*.js` plus `test/parallel/test-<api>.js`.

    `.mjs` (ESM) cases are excluded for v1 — the CJS corpus is the cleaner
    starting denominator. The over-match for short names (e.g. `os` →
    `test-os-*`) is acceptable; the report is per-API so noise stays scoped.
    """
    parallel = root / "test" / "parallel"
    # Node names test files with hyphens, but module names use underscores
    # (`string_decoder` → `test-string-decoder-*.js`, `perf_hooks` →
    # `test-perf-hooks-*.js`). Try both spellings.
    names = {api}
    if "_" in api:
        names.add(api.replace("_", "-"))
    hits: set[Path] = set()
    for n in names:
        hits.update(parallel.glob(f"test-{n}-*.js"))
        single = parallel / f"test-{n}.js"
        if single.exists():
            hits.add(single)
    return sorted(hits)


@dataclass
class Sample:
    api: str
    test: str
    reason: str


@dataclass
class Bucket:
    count: int = 0
    samples: list[Sample] = field(default_factory=list)

    def add(self, api: str, test: str, reason: str, sample_cap: int) -> None:
        self.count += 1
        if len(self.samples) < sample_cap:
            self.samples.append(Sample(api, test, reason[:300]))


def _kill_group(proc: subprocess.Popen) -> None:
    """SIGKILL the child's whole process group, then the child (#6305).

    Called only while `proc` is known to be unreaped, so its pid — and hence
    the group id it leads — cannot have been recycled onto some other process.
    """
    try:
        os.killpg(os.getpgid(proc.pid), signal.SIGKILL)
    except (ProcessLookupError, PermissionError, OSError):
        pass
    try:
        proc.kill()
    except OSError:
        pass


def run(cmd, env, timeout, cwd=None):
    """Return (exit_code, combined_stdout_stderr). exit_code 124 == timeout.

    The child gets its own process group (`start_new_session`) and on timeout
    the WHOLE group is killed. `subprocess.run(timeout=...)` kills only the
    direct child, and Node's core tests routinely fork servers, workers and
    `child_process` helpers — so every timed-out test used to leak a live
    process tree that kept burning CPU/memory and holding listening sockets for
    the remainder of the sweep. On the CI runner that accumulation is what
    eventually got the VM reclaimed mid-sweep (#6305).
    """
    try:
        p = subprocess.Popen(
            cmd,
            env=env,
            cwd=cwd,
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            start_new_session=True,
        )
    except FileNotFoundError as e:
        return 127, str(e)
    try:
        out, _ = p.communicate(timeout=timeout)
        return p.returncode, out.decode("utf-8", errors="replace")
    except subprocess.TimeoutExpired:
        _kill_group(p)
        # The group is gone, so the write ends of the pipe are all closed and
        # this drain cannot block for long; the bound is pure belt-and-braces.
        try:
            out, _ = p.communicate(timeout=30)
        except subprocess.TimeoutExpired:
            out = b""
        return 124, out.decode("utf-8", errors="replace")


def first_meaningful_line(text: str) -> str:
    for line in text.splitlines():
        s = line.strip()
        if s:
            return s
    return "(no output)"


def error_line(text: str) -> str:
    """Best diagnostic line from compiler output. Perry prints progress
    ("Collecting modules...") before the real error, so prefer a line that
    looks like an error and fall back to the last non-empty line."""
    lines = [ln.strip() for ln in text.splitlines() if ln.strip()]
    for ln in lines:
        low = ln.lower()
        if ("error" in low or "panic" in low or "unsupported" in low
                or "not supported" in low or "undefined symbol" in low
                or "not implemented" in low):
            return ln
    return lines[-1] if lines else "(no output)"


def parse_shard(spec: str) -> tuple[int, int]:
    """`"3/6"` -> (2, 6): a 0-based index and the shard count (#6305)."""
    m = re.fullmatch(r"\s*(\d+)\s*/\s*(\d+)\s*", spec)
    if not m:
        raise argparse.ArgumentTypeError(
            f"--shard must look like I/N (1-based), got {spec!r}")
    index, total = int(m.group(1)), int(m.group(2))
    if total < 1 or not 1 <= index <= total:
        raise argparse.ArgumentTypeError(
            f"--shard I/N needs 1 <= I <= N, got {spec!r}")
    return index - 1, total


def shard_apis(apis: list[str], index: int, total: int) -> list[str]:
    """Cost-aware deterministic split of `apis` across `total` shards (#6305).

    Every compile of an `_AUTO_OPTIMIZE_APIS` test pays a cargo relink of the
    Perry runtime, so those APIs dominate the sweep's wall clock. Round-robin
    them FIRST, so with total >= len(_AUTO_OPTIMIZE_APIS) no shard ever draws
    two — a contiguous split would drop http/https/net/zlib (adjacent in
    supported-apis.txt) into a single shard and leave it as slow as the
    unsharded job. Then round-robin the cheap APIs to even out the tail.
    """
    heavy = [a for a in apis if a in _AUTO_OPTIMIZE_APIS]
    light = [a for a in apis if a not in _AUTO_OPTIMIZE_APIS]
    mine = {a for i, a in enumerate(heavy) if i % total == index}
    mine |= {a for i, a in enumerate(light) if i % total == index}
    return [a for a in apis if a in mine]  # preserve supported-apis.txt order


def build_report(*, pinned: str, node_runtime: str, args, apis: list[str],
                 auto_optimize_apis_in_run: list[str],
                 buckets: dict[str, Bucket], per_api: dict[str, dict[str, int]],
                 per_api_seconds: dict[str, float], unreached: list[str],
                 partial: list[str], shard: str | None) -> dict:
    totals = {k: buckets[k].count for k in _BUCKETS}
    judged = sum(totals[k] for k in _BUCKETS if k != "node-skip")
    return {
        "generated_at": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
        "node_pinned": pinned,
        "node_runtime": node_runtime,
        "auto_optimize": args.auto_optimize,
        "auto_optimize_per_api": sorted(auto_optimize_apis_in_run),
        "shard": shard,
        # False => the sweep was cut short (time budget, or a hard kill that
        # left this file behind mid-write-cycle). The numbers below then cover
        # only `per_api`'s keys, and `partial_apis` only part of their tests,
        # so parity is a LOWER BOUND, not the sweep's verdict. #6305.
        "complete": not unreached and not partial,
        "unreached_apis": unreached,
        "partial_apis": partial,
        "apis": apis,
        "totals": totals,
        "judged": judged,
        "parity_pct": round(100 * totals["pass"] / judged, 1) if judged else 0.0,
        "per_api": per_api,
        "per_api_seconds": {k: round(v, 1) for k, v in per_api_seconds.items()},
        "samples": {
            k: [s.__dict__ for s in buckets[k].samples] for k in _SAMPLE_BUCKETS
        },
    }


def merge_reports(paths: list[Path], sample_cap: int) -> tuple[dict, list[Path]]:
    """Reduce per-shard reports to one aggregate (#6305).

    Shards partition the API list, so `per_api` is a plain dict union and the
    totals just sum. Returns (report, missing_paths); a report is "complete"
    only if every shard report exists AND every shard ran to the end of its
    own API list.
    """
    totals = {k: 0 for k in _BUCKETS}
    per_api: dict[str, dict[str, int]] = {}
    per_api_seconds: dict[str, float] = {}
    samples: dict[str, list] = {k: [] for k in _SAMPLE_BUCKETS}
    apis: list[str] = []
    unreached: list[str] = []
    partial: list[str] = []
    shards: list[dict] = []
    missing: list[Path] = []
    meta = {"node_pinned": "", "node_runtime": "", "auto_optimize": False,
            "auto_optimize_per_api": []}

    for path in sorted(paths):
        if not path.is_file():
            missing.append(path)
            continue
        try:
            r = json.loads(path.read_text())
        except (OSError, json.JSONDecodeError):
            missing.append(path)
            continue
        for k in ("node_pinned", "node_runtime"):
            meta[k] = meta[k] or r.get(k, "")
        meta["auto_optimize"] = meta["auto_optimize"] or r.get("auto_optimize", False)
        meta["auto_optimize_per_api"] = sorted(
            set(meta["auto_optimize_per_api"]) | set(r.get("auto_optimize_per_api", [])))
        for k in _BUCKETS:
            totals[k] += r.get("totals", {}).get(k, 0)
        per_api.update(r.get("per_api", {}))
        per_api_seconds.update(r.get("per_api_seconds", {}))
        for k in _SAMPLE_BUCKETS:
            samples[k].extend(r.get("samples", {}).get(k, []))
        apis.extend(a for a in r.get("apis", []) if a not in apis)
        unreached.extend(r.get("unreached_apis", []))
        partial.extend(r.get("partial_apis", []))
        shards.append({
            "shard": r.get("shard"),
            "complete": r.get("complete", False),
            "judged": r.get("judged", 0),
        })

    judged = sum(totals[k] for k in _BUCKETS if k != "node-skip")
    report = {
        "generated_at": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
        **meta,
        "shards": shards,
        "missing_shard_reports": [p.name for p in missing],
        "complete": (not missing and not unreached and not partial
                     and bool(shards)),
        "unreached_apis": unreached,
        "partial_apis": partial,
        "apis": apis,
        "totals": totals,
        "judged": judged,
        "parity_pct": round(100 * totals["pass"] / judged, 1) if judged else 0.0,
        "per_api": per_api,
        "per_api_seconds": per_api_seconds,
        "samples": {k: v[: sample_cap * 4] for k, v in samples.items()},
    }
    return report, missing


def run_merge(args) -> int:
    report, missing = merge_reports(list(args.merge), args.sample_cap)
    args.report.parent.mkdir(parents=True, exist_ok=True)
    args.report.write_text(json.dumps(report, indent=2) + "\n")

    print("=" * 60)
    print("  Node-core subset radar (#800) — merged shard reports")
    print("=" * 60)
    print(f"  shards:        {len(report['shards'])} "
          f"({sum(1 for s in report['shards'] if s['complete'])} complete)")
    for k in _BUCKETS:
        print(f"  {k:<14} {report['totals'][k]}")
    print(f"  {'judged':<14} {report['judged']}   (excludes node-skip)")
    print(f"  parity:        {report['parity_pct']}%")
    print(f"  report:        {args.report}")

    # Loud, not silent: an incomplete radar must not read as "no signal", and
    # must not quietly publish a partial number as if it were the whole sweep.
    # The report is written either way, so #4975 still gets a figure. #6305.
    if missing:
        print(f"\n  ERROR: {len(missing)} shard report(s) missing: "
              f"{', '.join(p.name for p in missing)}", file=sys.stderr)
    if report["unreached_apis"]:
        print(f"  ERROR: never reached: "
              f"{', '.join(report['unreached_apis'])}", file=sys.stderr)
    if report["partial_apis"]:
        print(f"  ERROR: cut off mid-API: "
              f"{', '.join(report['partial_apis'])}", file=sys.stderr)
    if not report["complete"]:
        print("  ERROR: radar did not fully run; the parity above is a LOWER "
              "BOUND over the tests that did run.", file=sys.stderr)
        return 1
    return 0


def main() -> int:
    ap = argparse.ArgumentParser(description="Node core test subset radar (#800)")
    ap.add_argument("--root", type=Path, default=REPO_ROOT / "vendor" / "nodejs",
                    help="path to a nodejs/node checkout (test/parallel + test/common)")
    ap.add_argument("--api", nargs="*", default=None,
                    help="restrict to these APIs (default: all in supported-apis.txt)")
    ap.add_argument("--max-per-api", type=int, default=0,
                    help="cap tests per API (0 = no cap)")
    ap.add_argument("--timeout", type=int, default=20, help="per-test timeout (s)")
    ap.add_argument("--auto-optimize", action="store_true",
                    help="link the per-program perry-ext-* crates / Cargo "
                         "features (drops PERRY_NO_AUTO_OPTIMIZE) so http/net/"
                         "https/ws servers, zlib, crypto and async_hooks are "
                         "measurable instead of mis-bucketed as compile-fail "
                         "link artifacts (#1778). Slower: the first compile per "
                         "import-set rebuilds the runtime (cached after).")
    ap.add_argument("--compile-timeout", type=int, default=0,
                    help="per-compile timeout (s) for auto-optimize APIs; "
                         "0 = use --timeout, or 600 under --auto-optimize to "
                         "absorb the cold ext-crate rebuild without "
                         "mis-bucketing it as compile-fail")
    ap.add_argument("--plain-compile-timeout", type=int, default=120,
                    help="per-compile timeout (s) for the NON-auto-optimize "
                         "APIs, which link the prebuilt libperry_*.a and so "
                         "never pay a cargo rebuild (#6305)")
    ap.add_argument("--shard", type=parse_shard, default=None, metavar="I/N",
                    help="run only shard I of N (1-based) of the API list; the "
                         "split is cost-aware, see shard_apis(). Reduce the "
                         "per-shard reports with --merge (#6305)")
    ap.add_argument("--time-budget", type=int, default=0, metavar="SECS",
                    help="stop the sweep after SECS and write a partial report "
                         "flagged `complete: false` rather than being killed "
                         "with nothing to show (0 = unbounded) (#6305)")
    ap.add_argument("--merge", nargs="*", type=Path, default=None,
                    metavar="REPORT",
                    help="merge these per-shard reports into --report and exit; "
                         "exits non-zero if any shard is missing or incomplete")
    ap.add_argument("--perry-bin", type=Path,
                    default=REPO_ROOT / "target" / "release" / "perry")
    ap.add_argument("--report", type=Path, default=NODE_CORE_DIR / "report.json")
    ap.add_argument("--sample-cap", type=int, default=8,
                    help="failing-test samples to record per bucket per API report")
    ap.add_argument("--quiet", action="store_true")
    args = ap.parse_args()

    if args.merge is not None:
        return run_merge(args)

    # Resolve early so the prewarm + timeout decisions cover both the
    # explicit `--auto-optimize` flag and the per-API auto-route (#2156).
    apis_pre = args.api or read_api_list(NODE_CORE_DIR / "supported-apis.txt")
    shard_label = None
    if args.shard is not None:
        shard_index, shard_total = args.shard
        shard_label = f"{shard_index + 1}/{shard_total}"
        apis_pre = shard_apis(apis_pre, shard_index, shard_total)
        if not args.quiet:
            print(f"  shard {shard_label}: "
                  f"{', '.join(apis_pre) if apis_pre else '(no APIs)'}",
                  flush=True)
    auto_optimize_apis_in_run = [a for a in apis_pre if a in _AUTO_OPTIMIZE_APIS]
    auto_optimize_needed = args.auto_optimize or bool(auto_optimize_apis_in_run)

    # The cold ext-crate rebuild on the first compile of each distinct
    # import-set can take minutes; without a longer compile budget it would
    # time out and land in compile-fail — the exact mis-bucketing #1778 is
    # about. Per-test execution still uses the tighter --timeout.
    #
    # #6305: this budget is PER-API, not global. It used to be one variable, so
    # the presence of a single auto-optimize API in the run raised the compile
    # ceiling to 600s for *every* API — including the ~314 tests that link the
    # prebuilt .a and compile in seconds. One pathological compile among those
    # could then burn 10 minutes of a budget that was already over-subscribed.
    ao_compile_timeout = args.compile_timeout or (
        max(args.timeout, 600) if auto_optimize_needed else args.timeout)
    plain_compile_timeout = args.compile_timeout or max(
        args.timeout, args.plain_compile_timeout)

    root = args.root.resolve()
    if not (root / "test" / "parallel").is_dir():
        print(f"error: {root}/test/parallel not found.\n"
              f"Vendor it first, e.g.:\n"
              f"  git clone --no-checkout --depth 1 --branch v22.x \\\n"
              f"    --filter=blob:none https://github.com/nodejs/node {root}\n"
              f"  (cd {root} && git sparse-checkout set test/parallel test/common "
              f"test/fixtures && git checkout)", file=sys.stderr)
        return 2
    if not args.perry_bin.exists():
        print(f"error: perry binary not found at {args.perry_bin} "
              f"(cargo build --release -p perry)", file=sys.stderr)
        return 2

    apis = apis_pre
    pinned = (NODE_CORE_DIR / "pinned-version.txt").read_text().strip()

    if not args.quiet:
        if args.auto_optimize:
            print(f"  auto-optimize: ON (#1778) — linking per-program perry-ext-* "
                  f"crates for every API; first compile per import-set rebuilds "
                  f"the runtime (compile-timeout={ao_compile_timeout}s).",
                  flush=True)
        elif auto_optimize_apis_in_run:
            print(f"  auto-optimize: ON per-API (#2156) for "
                  f"{', '.join(auto_optimize_apis_in_run)}; "
                  f"compile-timeout={ao_compile_timeout}s for those APIs.",
                  flush=True)
        if args.time_budget:
            print(f"  time budget: {args.time_budget}s (#6305)", flush=True)

    base_env = dict(os.environ)
    base_env.update(FORCE_COLOR="0", NO_COLOR="1", NODE_DISABLE_COLORS="1")
    fixtures = root / "test" / "fixtures"
    if fixtures.is_dir():
        base_env["PERRY_NODE_CORE_FIXTURES"] = str(fixtures)

    buckets = {k: Bucket() for k in _BUCKETS}
    per_api: dict[str, dict[str, int]] = {}
    per_api_seconds: dict[str, float] = {}
    # "Not finished yet", drained as each API completes — NOT "known to be
    # skipped". Seeding it with the whole list is what makes every snapshot of
    # the report honest by construction: a report written before the sweep (or
    # left behind by a kill mid-sweep) still lists everything outstanding, and
    # so can never claim `complete: true` over work that never ran. #6305.
    unreached: list[str] = list(apis)
    partial: list[str] = []
    node_runtime = run(["node", "--version"], base_env, 10)[1].strip()

    # The sweep can be cut short — by --time-budget, or by the job being killed
    # outright, which is what #6305 was: the report only existed after the last
    # API, so every interrupted run uploaded nothing. Write it after every API
    # so whatever we got is always on disk and mergeable.
    def emit_report() -> None:
        args.report.parent.mkdir(parents=True, exist_ok=True)
        args.report.write_text(json.dumps(build_report(
            pinned=pinned, node_runtime=node_runtime, args=args, apis=apis,
            auto_optimize_apis_in_run=auto_optimize_apis_in_run,
            buckets=buckets, per_api=per_api, per_api_seconds=per_api_seconds,
            unreached=unreached, partial=partial, shard=shard_label),
            indent=2) + "\n")

    # The budget covers the prewarm too — it is a cold cargo build that can run
    # for minutes, and a budget that only started counting afterwards would not
    # bound the job's wall clock, which is the whole point (#6305).
    deadline = time.monotonic() + args.time_budget if args.time_budget else None
    emit_report()  # a zero-work report beats no report if we die in the prewarm

    def _on_sigterm(_signum, _frame):
        """Land the report before we die.

        A CI job timeout / cancellation SIGTERMs the step process, and the
        default disposition would take us out without writing a thing. Whatever
        is still in `unreached` is exactly what we never finished, so the report
        this leaves behind is already correctly flagged incomplete.
        """
        try:
            emit_report()
        finally:
            os._exit(143)

    signal.signal(signal.SIGTERM, _on_sigterm)

    stage = Path(tempfile.mkdtemp(prefix="node-core-"))
    try:
        # Stage shared scaffolding: common/ (shim) + fixtures symlink.
        common_dst = stage / "common"
        common_dst.mkdir()
        for name, src in (("index.js", "index.js"),
                          ("tmpdir.js", "tmpdir.js"),
                          ("fixtures.js", "fixtures.js")):
            shutil.copy(SHIM_DIR / src, common_dst / name)
        if fixtures.is_dir():
            try:
                (stage / "fixtures").symlink_to(fixtures, target_is_directory=True)
            except OSError:
                pass
        parallel_stage = stage / "parallel"
        parallel_stage.mkdir()
        bin_dir = stage / "bin"
        bin_dir.mkdir()

        # #1842: under auto-optimize (global or per-API #2156), the first
        # compile that needs a given ext-crate / feature triggers a COLD
        # cargo build of heavy deps (hyper, tokio, openssl, flate2, …), which
        # can blow the per-test compile timeout and mis-bucket real-but-slow
        # http/net/crypto/zlib tests as compile-fail. Pre-warm ONCE with a
        # kitchen-sink that pulls in the server/client/crypto/zlib surface,
        # so every subsequent per-feature relink in the sweep is incremental
        # (fast) — not cold.
        if auto_optimize_needed:
            warm = parallel_stage / "_prewarm.ts"
            warm.write_text(
                "import * as http from 'node:http';\n"
                "import * as https from 'node:https';\n"
                "import * as net from 'node:net';\n"
                "import * as zlib from 'node:zlib';\n"
                "import * as crypto from 'node:crypto';\n"
                "http.createServer(() => {});\n"
                "https.createServer({}, () => {});\n"
                "net.createServer(() => {});\n"
                "zlib.createGzip();\n"
                "crypto.createHash('sha256');\n"
                "console.log('prewarm');\n"
            )
            if not args.quiet:
                print("  pre-warming ext-crate libs (one cold build; "
                      "makes per-feature relinks incremental, #1842)...",
                      flush=True)
            w_env = dict(base_env, PERRY_ALLOW_UNIMPLEMENTED="1")
            # cwd MUST be the perry workspace: auto-optimize locates the
            # Cargo workspace from cwd to (re)build the perry-ext-* crates. From
            # a temp cwd it silently skips the rebuild and link-fails. `.o`
            # litter in the repo root is gitignored (`*.o`). #1842.
            wc, _ = run([str(args.perry_bin), "compile", str(warm),
                         "-o", str(bin_dir / "_prewarm.out")],
                        w_env, max(args.timeout, 1800), cwd=str(REPO_ROOT))
            if not args.quiet:
                print(f"  pre-warm {'done' if wc == 0 else f'exit {wc} (continuing)'}",
                      flush=True)
            try:
                warm.unlink()
            except OSError:
                pass

        budget_hit = False
        for api in apis:
            # `unreached` already holds exactly the APIs we have not finished,
            # so there is nothing to recompute here — just stop.
            if deadline is not None and time.monotonic() >= deadline:
                print(f"  time budget exhausted — {len(unreached)} API(s) not "
                      f"reached: {', '.join(unreached)}", flush=True)
                break

            tests = resolve_tests(root, api)
            if args.max_per_api > 0:
                tests = tests[: args.max_per_api]
            counts = {k: 0 for k in buckets}
            api_started = time.monotonic()
            if not args.quiet:
                print(f"  {api} ({len(tests)} tests)...", flush=True)

            for tf in tests:
                # Check the budget per TEST, not just per API: one API is up to
                # `--max-per-api` compiles, and an auto-optimize compile may
                # take minutes, so an API-granular check would let a single API
                # overshoot the budget by hours. #6305.
                if deadline is not None and time.monotonic() >= deadline:
                    partial.append(api)
                    budget_hit = True
                    print(f"  time budget exhausted mid-{api} — "
                          f"{len(unreached) - 1} further API(s) not reached",
                          flush=True)
                    break

                test_name = tf.name
                staged = parallel_stage / test_name
                shutil.copy(tf, staged)

                # 1) Node is the oracle — with our shim in place.
                n_exit, n_out = run(["node", str(staged)], base_env,
                                    args.timeout)
                if n_exit != 0:
                    buckets["node-skip"].add(
                        api, test_name, first_meaningful_line(n_out),
                        args.sample_cap)
                    counts["node-skip"] += 1
                    continue

                # 2) Perry: compile (permissive — unimplemented APIs surface
                #    as runtime divergence, the gap signal). Raw CommonJS `.js`
                #    is handled natively now (require/module.exports rewritten
                #    to ESM); no .ts staging or external rewriter needed.
                #    By default PERRY_NO_AUTO_OPTIMIZE skips the per-compile
                #    runtime rebuild for speed, but that also skips linking
                #    the perry-ext-* server/feature crates — see #1778 and
                #    the --auto-optimize flag — AND lets compile.rs emit
                #    `undefined`-returning stubs for ext symbols (#2156). So
                #    for APIs whose well-known binding routes to an ext crate
                #    (`_AUTO_OPTIMIZE_APIS`), drop the flag even without
                #    --auto-optimize so the ext crates this program imports
                #    actually get linked.
                #    cwd=bin_dir contains the `.o` litter perry emits.
                out_bin = bin_dir / (test_name + ".out")
                effective_ao = args.auto_optimize or (api in _AUTO_OPTIMIZE_APIS)
                c_env = dict(base_env, PERRY_ALLOW_UNIMPLEMENTED="1")
                if not effective_ao:
                    c_env["PERRY_NO_AUTO_OPTIMIZE"] = "1"
                # Under auto-optimize, compile from the perry workspace so
                # auto-optimize can build/link the perry-ext-* crates (it
                # locates the workspace via cwd; a temp cwd silently skips the
                # ext-crate rebuild → link-fail). `.o` litter in the repo root
                # is gitignored. Without auto-optimize, keep cwd=bin_dir so the
                # `.o` files stay in the disposable stage dir. #1842.
                compile_cwd = str(REPO_ROOT) if effective_ao else str(bin_dir)
                c_exit, c_out = run(
                    [str(args.perry_bin), "compile", str(staged),
                     "-o", str(out_bin)],
                    c_env,
                    ao_compile_timeout if effective_ao else plain_compile_timeout,
                    cwd=compile_cwd)
                if c_exit != 0:
                    buckets["compile-fail"].add(
                        api, test_name, error_line(c_out), args.sample_cap)
                    counts["compile-fail"] += 1
                    continue

                # 3) Run the Perry binary.
                p_exit, p_out = run([str(out_bin)], base_env, args.timeout)
                try:
                    out_bin.unlink()
                except OSError:
                    pass
                if p_exit != 0:
                    buckets["runtime-fail"].add(
                        api, test_name, first_meaningful_line(p_out),
                        args.sample_cap)
                    counts["runtime-fail"] += 1
                elif normalize(p_out) == normalize(n_out):
                    buckets["pass"].add(api, test_name, "", args.sample_cap)
                    counts["pass"] += 1
                else:
                    buckets["diff"].add(
                        api, test_name, first_meaningful_line(p_out),
                        args.sample_cap)
                    counts["diff"] += 1

                staged.unlink()

            per_api[api] = counts
            per_api_seconds[api] = time.monotonic() - api_started
            if api in unreached:  # `in` guard: --api may repeat a name
                unreached.remove(api)  # done with it — partially or fully
            # Rewrite the report now, not at the end of the sweep: if the job
            # is killed (#6305 — the hosted runner was reclaimed every night)
            # this is the only thing that survives, and it is what the artifact
            # upload picks up.
            emit_report()
            if not args.quiet:
                judged = sum(counts[k] for k in
                             ("pass", "diff", "runtime-fail", "compile-fail"))
                rate = f"{100 * counts['pass'] / judged:.0f}%" if judged else "—"
                print(f"  {api:<16} pass={counts['pass']:<4} diff={counts['diff']:<4} "
                      f"rt-fail={counts['runtime-fail']:<4} "
                      f"compile-fail={counts['compile-fail']:<4} "
                      f"node-skip={counts['node-skip']:<4} parity={rate} "
                      f"({per_api_seconds[api]:.0f}s)", flush=True)
            if budget_hit:
                break
    finally:
        shutil.rmtree(stage, ignore_errors=True)

    emit_report()
    report = json.loads(args.report.read_text())

    print()
    print("=" * 60)
    print(f"  Node-core subset radar (#800) — Node {pinned}"
          + (f" — shard {shard_label}" if shard_label else ""))
    print("=" * 60)
    for k in _BUCKETS:
        print(f"  {k:<14} {report['totals'][k]}")
    print(f"  {'judged':<14} {report['judged']}   (excludes node-skip)")
    print(f"  parity:        {report['parity_pct']}%")
    print(f"  report:        {args.report}")

    # An incomplete sweep exits non-zero. The radar is advisory about *parity*
    # — compile-fail and runtime-fail never fail the job, that is the gap signal
    # it exists to publish — but it is NOT advisory about whether it actually
    # ran. A truncated sweep that exits 0 is how #6305 hid for 38 straight
    # nightlies. The report is written either way, so the number still lands.
    if not report["complete"]:
        print()
        if partial:
            print(f"  ERROR: cut off mid-API: {', '.join(partial)}",
                  file=sys.stderr)
        if unreached:
            print(f"  ERROR: never reached: {', '.join(unreached)}",
                  file=sys.stderr)
        print("  ERROR: sweep incomplete — parity above is a LOWER BOUND. "
              "Raise --time-budget / the job timeout, or add shards.",
              file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
