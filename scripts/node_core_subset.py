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

See test-compat/node-core/README.md.
"""

from __future__ import annotations

import argparse
import json
import os
import re
import shutil
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

# Lines that are pure environmental noise from either runtime — stripped
# before the stdout tiebreak so a warning never registers as a "diff".
_NOISE = re.compile(
    r"^\(node:\d+\) (ExperimentalWarning|Warning|\[DEP\d+\]|\[MODULE_TYPELESS)"
    r"|^\(Use `node --trace"
)


def normalize(text: str) -> str:
    out = []
    for raw in text.replace("\r\n", "\n").split("\n"):
        line = raw.rstrip()
        if _NOISE.search(line):
            continue
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


def run(cmd, env, timeout, cwd=None):
    """Return (exit_code, combined_stdout_stderr). exit_code 124 == timeout."""
    try:
        p = subprocess.run(
            cmd,
            env=env,
            cwd=cwd,
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            timeout=timeout,
        )
        return p.returncode, p.stdout.decode("utf-8", errors="replace")
    except subprocess.TimeoutExpired as e:
        out = e.stdout.decode("utf-8", errors="replace") if e.stdout else ""
        return 124, out
    except FileNotFoundError as e:
        return 127, str(e)


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


def main() -> int:
    ap = argparse.ArgumentParser(description="Node core test subset radar (#800)")
    ap.add_argument("--root", type=Path, default=REPO_ROOT / "vendor" / "nodejs",
                    help="path to a nodejs/node checkout (test/parallel + test/common)")
    ap.add_argument("--api", nargs="*", default=None,
                    help="restrict to these APIs (default: all in supported-apis.txt)")
    ap.add_argument("--max-per-api", type=int, default=0,
                    help="cap tests per API (0 = no cap)")
    ap.add_argument("--timeout", type=int, default=20, help="per-test timeout (s)")
    ap.add_argument("--perry-bin", type=Path,
                    default=REPO_ROOT / "target" / "release" / "perry")
    ap.add_argument("--report", type=Path, default=NODE_CORE_DIR / "report.json")
    ap.add_argument("--sample-cap", type=int, default=8,
                    help="failing-test samples to record per bucket per API report")
    ap.add_argument("--quiet", action="store_true")
    args = ap.parse_args()

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

    apis = args.api or read_api_list(NODE_CORE_DIR / "supported-apis.txt")
    pinned = (NODE_CORE_DIR / "pinned-version.txt").read_text().strip()

    base_env = dict(os.environ)
    base_env.update(FORCE_COLOR="0", NO_COLOR="1", NODE_DISABLE_COLORS="1")
    fixtures = root / "test" / "fixtures"
    if fixtures.is_dir():
        base_env["PERRY_NODE_CORE_FIXTURES"] = str(fixtures)

    buckets = {k: Bucket() for k in
               ("pass", "diff", "runtime-fail", "compile-fail", "node-skip")}
    per_api: dict[str, dict[str, int]] = {}

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

        for api in apis:
            tests = resolve_tests(root, api)
            if args.max_per_api > 0:
                tests = tests[: args.max_per_api]
            counts = {k: 0 for k in buckets}

            for tf in tests:
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
                #    PERRY_NO_AUTO_OPTIMIZE skips the per-compile runtime
                #    rebuild; cwd=bin_dir contains the `.o` litter perry emits.
                out_bin = bin_dir / (test_name + ".out")
                c_env = dict(base_env, PERRY_ALLOW_UNIMPLEMENTED="1",
                             PERRY_NO_AUTO_OPTIMIZE="1")
                c_exit, c_out = run(
                    [str(args.perry_bin), "compile", str(staged),
                     "-o", str(out_bin)],
                    c_env, args.timeout, cwd=str(bin_dir))
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
            if not args.quiet:
                judged = sum(counts[k] for k in
                             ("pass", "diff", "runtime-fail", "compile-fail"))
                rate = f"{100 * counts['pass'] / judged:.0f}%" if judged else "—"
                print(f"  {api:<16} pass={counts['pass']:<4} diff={counts['diff']:<4} "
                      f"rt-fail={counts['runtime-fail']:<4} "
                      f"compile-fail={counts['compile-fail']:<4} "
                      f"node-skip={counts['node-skip']:<4} parity={rate}")
    finally:
        shutil.rmtree(stage, ignore_errors=True)

    totals = {k: buckets[k].count for k in buckets}
    judged = sum(totals[k] for k in
                 ("pass", "diff", "runtime-fail", "compile-fail"))
    parity_pct = round(100 * totals["pass"] / judged, 1) if judged else 0.0

    report = {
        "generated_at": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
        "node_pinned": pinned,
        "node_runtime": run(["node", "--version"], base_env, 10)[1].strip(),
        "apis": apis,
        "totals": totals,
        "judged": judged,
        "parity_pct": parity_pct,
        "per_api": per_api,
        "samples": {
            k: [s.__dict__ for s in buckets[k].samples]
            for k in ("diff", "runtime-fail", "compile-fail", "node-skip")
        },
    }
    args.report.write_text(json.dumps(report, indent=2) + "\n")

    print()
    print("=" * 60)
    print(f"  Node-core subset radar (#800) — Node {pinned}")
    print("=" * 60)
    for k in ("pass", "diff", "runtime-fail", "compile-fail", "node-skip"):
        print(f"  {k:<14} {totals[k]}")
    print(f"  {'judged':<14} {judged}   (excludes node-skip)")
    print(f"  parity:        {parity_pct}%")
    print(f"  report:        {args.report}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
