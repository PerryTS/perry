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
    ap.add_argument("--auto-optimize", action="store_true",
                    help="link the per-program perry-ext-* crates / Cargo "
                         "features (drops PERRY_NO_AUTO_OPTIMIZE) so http/net/"
                         "https/ws servers, zlib, crypto and async_hooks are "
                         "measurable instead of mis-bucketed as compile-fail "
                         "link artifacts (#1778). Slower: the first compile per "
                         "import-set rebuilds the runtime (cached after).")
    ap.add_argument("--compile-timeout", type=int, default=0,
                    help="per-compile timeout (s); 0 = use --timeout, or 600 "
                         "under --auto-optimize to absorb the cold ext-crate "
                         "rebuild without mis-bucketing it as compile-fail")
    ap.add_argument("--perry-bin", type=Path,
                    default=REPO_ROOT / "target" / "release" / "perry")
    ap.add_argument("--report", type=Path, default=NODE_CORE_DIR / "report.json")
    ap.add_argument("--sample-cap", type=int, default=8,
                    help="failing-test samples to record per bucket per API report")
    ap.add_argument("--quiet", action="store_true")
    args = ap.parse_args()

    # Resolve early so the prewarm + timeout decisions cover both the
    # explicit `--auto-optimize` flag and the per-API auto-route (#2156).
    apis_pre = args.api or read_api_list(NODE_CORE_DIR / "supported-apis.txt")
    auto_optimize_apis_in_run = [a for a in apis_pre if a in _AUTO_OPTIMIZE_APIS]
    auto_optimize_needed = args.auto_optimize or bool(auto_optimize_apis_in_run)

    # The cold ext-crate rebuild on the first compile of each distinct
    # import-set can take minutes; without a longer compile budget it would
    # time out and land in compile-fail — the exact mis-bucketing #1778 is
    # about. Per-test execution still uses the tighter --timeout.
    compile_timeout = args.compile_timeout or (
        max(args.timeout, 600) if auto_optimize_needed else args.timeout)

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
                  f"the runtime (compile-timeout={compile_timeout}s).")
        elif auto_optimize_apis_in_run:
            print(f"  auto-optimize: ON per-API (#2156) for "
                  f"{', '.join(auto_optimize_apis_in_run)}; "
                  f"compile-timeout={compile_timeout}s for those APIs.")

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
                      "makes per-feature relinks incremental, #1842)...")
            w_env = dict(base_env, PERRY_ALLOW_UNIMPLEMENTED="1")
            # cwd MUST be the perry workspace: auto-optimize locates the
            # Cargo workspace from cwd to (re)build the perry-ext-* crates. From
            # a temp cwd it silently skips the rebuild and link-fails. `.o`
            # litter in the repo root is gitignored (`*.o`). #1842.
            wc, _ = run([str(args.perry_bin), "compile", str(warm),
                         "-o", str(bin_dir / "_prewarm.out")],
                        w_env, max(args.timeout, 1800), cwd=str(REPO_ROOT))
            if not args.quiet:
                print(f"  pre-warm {'done' if wc == 0 else f'exit {wc} (continuing)'}")
            try:
                warm.unlink()
            except OSError:
                pass

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
                    c_env, compile_timeout, cwd=compile_cwd)
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
        "auto_optimize": args.auto_optimize,
        "auto_optimize_per_api": sorted(auto_optimize_apis_in_run),
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
