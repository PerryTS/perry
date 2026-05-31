#!/usr/bin/env python3
"""Fast, PARALLEL parity-gap radar over test262 + Node core, emitting ONE
severity-sorted JSON report of every failing case.

Why this exists
---------------
`scripts/test262_subset.py` and `scripts/node_core_subset.py` are strictly
serial (one `for` loop) and only keep `--sample-cap` example failures per
bucket. A full test262 sweep that way is ~16h and never lists every failure.

A single Perry compile is ~1.0s wall but only ~0.2s CPU (it's link/spawn
bound), so the machine's other cores sit idle. This driver fans the per-case
work out across a process pool and records EVERY non-pass case, then writes a
report sorted worst-first:

    compile-fail (3)  > runtime-fail (2)  > diff / wrong-output (1)

It REUSES the discovery / assembly / normalization helpers from the two
existing radars so the verdicts stay identical to the canonical runners — this
is purely a parallel, full-capture wrapper.

Usage
-----
    # representative fast pass (~15-20 min): sample each category + all node-core
    scripts/parity_gap_report.py --sample-per-cat 150

    # full exhaustive sweep (~1.5-2h): every applicable case
    scripts/parity_gap_report.py

    # just one corpus / scope
    scripts/parity_gap_report.py --corpus test262 --dir built-ins/RegExp
    scripts/parity_gap_report.py --corpus node-core --api buffer fs

Notes
-----
* node-core's auto-optimize APIs (http/net/https/zlib/crypto/events) need a
  workspace-cwd ext-crate relink that is NOT parallel-safe, so this quick radar
  runs every API under PERRY_NO_AUTO_OPTIMIZE. Those APIs may show inflated
  compile/runtime-fail counts (link artifacts, not real gaps) — each such
  failure is tagged `auto_optimize_api: true` in the report. For a *final*
  measurement of those, use scripts/node_core_subset.py --auto-optimize.
"""

from __future__ import annotations

import argparse
import json
import os
import shutil
import subprocess
import sys
import tempfile
import time
from concurrent.futures import ProcessPoolExecutor, as_completed
from pathlib import Path

SCRIPT_DIR = Path(__file__).resolve().parent
REPO_ROOT = SCRIPT_DIR.parent
sys.path.insert(0, str(SCRIPT_DIR))

import test262_subset as T262  # noqa: E402
import node_core_subset as NC  # noqa: E402

SEVERITY_RANK = {"compile-fail": 3, "runtime-fail": 2, "diff": 1}

# Process-global state set once per worker via the pool initializer.
_W: dict = {}


# --------------------------------------------------------------------------
# worker setup
# --------------------------------------------------------------------------
def _init_worker(perry_bin: str, timeout: int, use_lld: bool,
                 t262_root: str, t262_harness: str, t262_preamble: str,
                 nc_root: str, nc_fixtures: str) -> None:
    env = dict(os.environ)
    env.update(FORCE_COLOR="0", NO_COLOR="1", NODE_DISABLE_COLORS="1",
               PERRY_ALLOW_UNIMPLEMENTED="1", PERRY_NO_AUTO_OPTIMIZE="1",
               PERRY_NO_CACHE="1")
    if use_lld:
        env["PERRY_LLD_LINK"] = "1"
    _W["perry"] = perry_bin
    _W["timeout"] = timeout
    _W["base_env"] = env
    _W["t262_root"] = Path(t262_root)
    _W["t262_harness"] = Path(t262_harness)
    _W["t262_preamble"] = t262_preamble
    _W["nc_root"] = Path(nc_root)
    _W["nc_fixtures"] = nc_fixtures


def _judge_test262(rel: str) -> dict | None:
    """Compile+run one assembled test262 case. Returns a failure record or
    None on pass / skip. Mirrors test262_subset.main()'s classification."""
    test_root = _W["t262_root"] / "test"
    path = test_root / rel
    try:
        src = path.read_text(encoding="utf-8", errors="replace")
    except OSError as e:
        return None  # vanished between discovery and run
    meta = T262.parse_frontmatter(src)
    if meta is None:
        return None
    try:
        program = T262.assemble(src, meta, _W["t262_harness"], _W["t262_preamble"])
    except OSError:
        return None  # missing include -> skip (matches canonical runner)

    env = _W["base_env"]
    to = _W["timeout"]
    with tempfile.TemporaryDirectory(prefix="t262w-") as d:
        dp = Path(d)
        staged = dp / "case.js"
        staged.write_text(program)
        out_bin = dp / "case.out"

        n_exit, n_out = NC.run(["node", str(staged)], env, to)
        node_clean = n_exit == 0

        c_exit, c_out = NC.run(
            [_W["perry"], "compile", str(staged), "-o", str(out_bin)],
            env, to, cwd=str(dp))

        if c_exit != 0:
            if node_clean:
                return _rec("test262", T262.top_dir(rel), rel, "compile-fail",
                            T262.error_line(c_out))
            return None  # negative case, both reject -> pass

        p_exit, p_out = NC.run([str(out_bin)], env, to)
        perry_clean = p_exit == 0
        if node_clean and perry_clean:
            if T262.normalize(p_out) == T262.normalize(n_out):
                return None
            return _rec("test262", T262.top_dir(rel), rel, "diff",
                        T262.first_line(p_out))
        if node_clean and not perry_clean:
            return _rec("test262", T262.top_dir(rel), rel, "runtime-fail",
                        T262.first_line(p_out))
        if not node_clean and not perry_clean:
            return None  # negative case, both reject -> pass
        return _rec("test262", T262.top_dir(rel), rel, "runtime-fail",
                    "Perry ran clean; Node rejected (missed negative)")


def _judge_nodecore(api: str, test_file: str) -> dict | None:
    """Compile+run one Node core test under the common/ shim. Mirrors
    node_core_subset.main()'s classification (no auto-optimize)."""
    env = _W["base_env"]
    to = _W["timeout"]
    ao = api in NC._AUTO_OPTIMIZE_APIS
    with tempfile.TemporaryDirectory(prefix="ncw-") as d:
        dp = Path(d)
        common = dp / "common"
        common.mkdir()
        for name, src in (("index.js", "index.js"),
                          ("tmpdir.js", "tmpdir.js"),
                          ("fixtures.js", "fixtures.js")):
            shutil.copy(NC.SHIM_DIR / src, common / name)
        if _W["nc_fixtures"]:
            try:
                (dp / "fixtures").symlink_to(_W["nc_fixtures"],
                                             target_is_directory=True)
            except OSError:
                pass
        par = dp / "parallel"
        par.mkdir()
        staged = par / Path(test_file).name
        shutil.copy(test_file, staged)
        out_bin = dp / "case.out"

        run_env = dict(env)
        if _W["nc_fixtures"]:
            run_env["PERRY_NODE_CORE_FIXTURES"] = _W["nc_fixtures"]

        n_exit, n_out = NC.run(["node", str(staged)], run_env, to)
        if n_exit != 0:
            return None  # node-skip — never charged against Perry

        c_exit, c_out = NC.run(
            [_W["perry"], "compile", str(staged), "-o", str(out_bin)],
            run_env, to, cwd=str(par))
        if c_exit != 0:
            return _rec("node-core", api, Path(test_file).name, "compile-fail",
                        NC.error_line(c_out), auto_optimize_api=ao)

        p_exit, p_out = NC.run([str(out_bin)], run_env, to)
        if p_exit != 0:
            return _rec("node-core", api, Path(test_file).name, "runtime-fail",
                        NC.first_meaningful_line(p_out), auto_optimize_api=ao)
        if NC.normalize(p_out) == NC.normalize(n_out):
            return None
        return _rec("node-core", api, Path(test_file).name, "diff",
                    NC.first_meaningful_line(p_out), auto_optimize_api=ao)


def _rec(corpus: str, category: str, test: str, severity: str, reason: str,
         **extra) -> dict:
    r = {"corpus": corpus, "category": category, "test": test,
         "severity": severity, "severity_rank": SEVERITY_RANK[severity],
         "reason": (reason or "")[:300]}
    r.update(extra)
    return r


# dispatch one work item (picklable top-level fn for the pool)
def _run_item(item: tuple) -> dict | None:
    kind = item[0]
    if kind == "t262":
        return _judge_test262(item[1])
    return _judge_nodecore(item[1], item[2])


# --------------------------------------------------------------------------
# work-list construction (parent process)
# --------------------------------------------------------------------------
def build_test262_items(root: Path, dirs, all_features: bool,
                        sample_per_cat: int) -> list[tuple]:
    applicable = set(T262.read_list(T262.TEST262_DIR / "features-applicable.txt"))
    per_cat: dict[str, int] = {}
    items: list[tuple] = []
    for rel, _src, _meta in T262.discover(root, dirs, applicable, all_features):
        cat = T262.top_dir(rel)
        if sample_per_cat and per_cat.get(cat, 0) >= sample_per_cat:
            continue
        per_cat[cat] = per_cat.get(cat, 0) + 1
        items.append(("t262", rel))
    return items


def build_nodecore_items(root: Path, apis, max_per_api: int) -> list[tuple]:
    items: list[tuple] = []
    for api in apis:
        tests = NC.resolve_tests(root, api)
        if max_per_api:
            tests = tests[:max_per_api]
        for tf in tests:
            items.append(("nc", api, str(tf)))
    return items


def build_report(failures: list[dict], cases_judged: int, elapsed: float,
                 perry_bin: str, config: dict) -> dict:
    """Sort failures worst-first and roll up the severity/corpus/category
    breakdowns. Shared by the live run and `--rebuild-from`."""
    failures = sorted(failures, key=lambda r: (-r.get("severity_rank", 0),
                                               r.get("corpus", ""),
                                               r.get("category", ""),
                                               r.get("test", "")))
    by_sev: dict[str, int] = {}
    by_corpus: dict[str, int] = {}
    by_cat: dict[str, dict[str, int]] = {}
    for r in failures:
        by_sev[r["severity"]] = by_sev.get(r["severity"], 0) + 1
        by_corpus[r["corpus"]] = by_corpus.get(r["corpus"], 0) + 1
        c = by_cat.setdefault(f'{r["corpus"]}:{r["category"]}',
                              {"compile-fail": 0, "runtime-fail": 0, "diff": 0})
        c[r["severity"]] += 1
    return {
        "generated_unix": int(time.time()),
        "elapsed_seconds": round(elapsed, 1),
        "perry_bin": perry_bin,
        "config": config,
        "summary": {
            "cases_judged": cases_judged,
            "total_failures": len(failures),
            "by_severity": dict(sorted(by_sev.items(),
                                       key=lambda kv: -SEVERITY_RANK.get(kv[0], 0))),
            "by_corpus": by_corpus,
            "by_category": dict(sorted(by_cat.items(),
                                       key=lambda kv: -sum(kv[1].values()))),
        },
        "failures": failures,
    }


def main() -> int:
    ap = argparse.ArgumentParser(description="Parallel parity-gap radar (#799/#800)")
    ap.add_argument("--corpus", default="test262,node-core",
                    help="comma list: test262, node-core (default both)")
    ap.add_argument("--workers", type=int, default=max(2, (os.cpu_count() or 4) + 2),
                    help="parallel workers (default cpu+2; compiles are link-bound)")
    ap.add_argument("--timeout", type=int, default=5, help="per-step timeout (s)")
    ap.add_argument("--no-lld", action="store_true", help="don't force PERRY_LLD_LINK")
    # test262 scope
    ap.add_argument("--dir", nargs="*", default=list(T262.DEFAULT_DIRS))
    ap.add_argument("--all-features", action="store_true")
    ap.add_argument("--sample-per-cat", type=int, default=0,
                    help="cap test262 cases per top-dir category (0 = all)")
    # node-core scope
    ap.add_argument("--api", nargs="*", default=None)
    ap.add_argument("--max-per-api", type=int, default=0)
    # io
    ap.add_argument("--perry-bin", type=Path,
                    default=REPO_ROOT / "target" / "release" / "perry")
    ap.add_argument("--out", type=Path,
                    default=REPO_ROOT / "test-compat" / "parity_gaps_report.json")
    ap.add_argument("--rebuild-from", type=Path, default=None,
                    help="reconstruct the sorted report from a partial "
                         ".partial.jsonl (e.g. after a crash) and exit")
    args = ap.parse_args()

    if args.rebuild_from is not None:
        failures = []
        for line in args.rebuild_from.read_text().splitlines():
            line = line.strip()
            if line:
                failures.append(json.loads(line))
        report = build_report(failures, cases_judged=len(failures), elapsed=0.0,
                              perry_bin=str(args.perry_bin),
                              config={"rebuilt_from": str(args.rebuild_from)})
        args.out.parent.mkdir(parents=True, exist_ok=True)
        args.out.write_text(json.dumps(report, indent=2) + "\n")
        print(f"rebuilt {len(failures)} failures -> {args.out}", file=sys.stderr)
        print(f"  by severity: {report['summary']['by_severity']}", file=sys.stderr)
        return 0

    corpora = {c.strip() for c in args.corpus.split(",") if c.strip()}
    if not args.perry_bin.exists():
        print(f"error: perry binary not found at {args.perry_bin} "
              f"(cargo build --release -p perry)", file=sys.stderr)
        return 2

    t262_root = (REPO_ROOT / "vendor" / "test262").resolve()
    nc_root = (REPO_ROOT / "vendor" / "nodejs").resolve()
    nc_fixtures = nc_root / "test" / "fixtures"
    nc_fixtures_s = str(nc_fixtures) if nc_fixtures.is_dir() else ""

    items: list[tuple] = []
    if "test262" in corpora:
        if not (t262_root / "test").is_dir():
            print(f"warn: test262 not vendored at {t262_root}, skipping",
                  file=sys.stderr)
        else:
            items += build_test262_items(t262_root, args.dir, args.all_features,
                                         args.sample_per_cat)
    if "node-core" in corpora:
        apis = args.api or NC.read_api_list(NC.NODE_CORE_DIR / "supported-apis.txt")
        if not (nc_root / "test" / "parallel").is_dir():
            print(f"warn: node corpus not vendored at {nc_root}, skipping",
                  file=sys.stderr)
        else:
            items += build_nodecore_items(nc_root, apis, args.max_per_api)

    total = len(items)
    print(f"radar: {total} cases | {args.workers} workers | timeout {args.timeout}s "
          f"| lld {'off' if args.no_lld else 'on'}", file=sys.stderr)
    if total == 0:
        print("nothing to run", file=sys.stderr)
        return 1

    t0 = time.time()
    failures: list[dict] = []
    judged = 0
    init_args = (str(args.perry_bin), args.timeout, not args.no_lld,
                 str(t262_root), str(t262_root / "harness"),
                 (T262.PREAMBLE.read_text() if T262.PREAMBLE.exists() else ""),
                 str(nc_root), nc_fixtures_s)

    # Crash-safe streaming: every failure is flushed to a JSONL sidecar the
    # instant it's judged, so an OOM/kill mid-run loses nothing already found
    # (the previous all-in-memory design lost ~3.9k results to an OOM at 95%).
    # The final sorted report is rebuilt from this file at the end; if the run
    # dies, `--rebuild-from <jsonl>` reconstructs the report from the partial.
    args.out.parent.mkdir(parents=True, exist_ok=True)
    jsonl_path = args.out.with_suffix(".partial.jsonl")
    jf = jsonl_path.open("w")
    try:
        with ProcessPoolExecutor(max_workers=args.workers,
                                 initializer=_init_worker,
                                 initargs=init_args) as ex:
            futs = [ex.submit(_run_item, it) for it in items]
            for fut in as_completed(futs):
                judged += 1
                if judged % 250 == 0 or judged == total:
                    el = time.time() - t0
                    rate = judged / el if el else 0
                    eta = (total - judged) / rate if rate else 0
                    print(f"  {judged}/{total}  fails={len(failures)}  "
                          f"{rate:.1f}/s  eta {eta/60:.1f}m", file=sys.stderr)
                try:
                    rec = fut.result()
                except Exception:  # noqa: BLE001 — never let one case kill the run
                    rec = None
                if rec is not None:
                    failures.append(rec)
                    jf.write(json.dumps(rec) + "\n")
                    jf.flush()  # durable per-case so a crash loses nothing
    finally:
        jf.close()

    elapsed = time.time() - t0
    report = build_report(failures, cases_judged=total, elapsed=elapsed,
                          perry_bin=str(args.perry_bin),
                          config={
                              "corpus": sorted(corpora), "workers": args.workers,
                              "timeout": args.timeout, "lld": not args.no_lld,
                              "sample_per_cat": args.sample_per_cat,
                              "max_per_api": args.max_per_api,
                              "all_features": args.all_features,
                          })
    args.out.write_text(json.dumps(report, indent=2) + "\n")
    try:
        jsonl_path.unlink()  # full report written; partial no longer needed
    except OSError:
        pass

    print(f"\ndone in {elapsed/60:.1f}m | {len(failures)} failures "
          f"of {total} judged", file=sys.stderr)
    print(f"  by severity: {report['summary']['by_severity']}", file=sys.stderr)
    print(f"  report: {args.out}", file=sys.stderr)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
