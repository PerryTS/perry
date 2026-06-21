#!/usr/bin/env python3
"""Run the TS-subset-applicable slice of ECMAScript Test262 under both Perry and
Node, bucket the divergences, and write a JSON report (#799).

Companion to `node_core_subset.py` (#800): where that runner pulls Node's own
`test/parallel` corpus, this one pulls TC39's Test262 — the canonical
conformance suite for the *language* (not the Node APIs). Same spirit: a
**coverage radar**, not a gate. It points at the biggest language gaps; it does
not block merges.

Differential model
------------------
Test262 cases are silent on success and `throw` on failure, so exit-code parity
is the primary signal (stdout is a secondary tiebreak for positive cases). Each
case relies on a harness host that defines `Test262Error`, `assert`, etc. We
assemble each case the way TC39's own runner does — concatenate the default
harness (`sta.js` + `assert.js`), a tiny host `preamble.js` (`print` /
`$DONOTEVALUATE`), any `includes:` files, and the test source — then run that
single script under BOTH runtimes. The differential therefore compares the two
runtimes' *builtins*, never their private harnesses.

Crucial difference from the Node-core radar: Test262 is full of **negative
tests**, where the correct behaviour is to *reject* (SyntaxError at parse, or a
thrown error at runtime). So we do NOT drop "Node exited non-zero" cases the way
the Node radar drops `node-skip`. Instead we bucket by Perry-vs-Node
*agreement*: if Node rejects and Perry also rejects (at compile OR runtime),
that's a `pass`. The only `skip` here is a case we couldn't even assemble
(missing include, unsupported flag, or a `$262`-host dependency).

Self-validating mode (#4792)
----------------------------
The differential above needs an oracle that can *run* the feature. For things
Perry implements but the Node oracle does not — `Temporal` is the motivating
case (Node v22/v25 ship no `Temporal` global) — a differential would score
every Perry success as a `runtime-fail` (Node throws `Temporal is not defined`,
Perry runs clean), making real work look like regressions and excluding it from
the denominator entirely.

Test262 cases are *self-checking*: they `assert.*`-throw on failure and exit 0
on success. So for any feature on `self-validate-features.txt` we drop the
oracle and judge Perry on its own verdict: a positive case **passes** iff its
Perry binary runs to completion without throwing (exit 0); a negative case
passes iff it threw (exit != 0). These cases land in the normal
`pass`/`runtime-fail`/`compile-fail` buckets — so `built-ins/Temporal` shows up
as its own per-dir cluster — and a `self_validated` tally in the report records
how many of the judged cases were scored this way.

Buckets
-------
- pass         — Perry agrees with Node:
                   * both run clean (exit 0) and stdout matches  (positive), or
                   * both reject (Node exit != 0 and Perry compile- or
                     runtime-rejects)                            (negative).
- diff         — both run clean (exit 0) but stdout differs.
- runtime-fail — verdict mismatch on a case Perry *compiled*: Node ran clean but
                 Perry threw, OR Node rejected but Perry ran clean (a missed
                 negative).
- compile-fail — Perry refused to compile a case Node ran clean. (If Node also
                 rejected, the compile rejection is *correct* and lands in
                 `pass` instead.)
- skip         — couldn't assemble / needs an unsupported flag or `$262` host
                 API. Excluded from the parity verdict — never charged against
                 Perry.

Usage
-----
    scripts/test262_subset.py --root vendor/test262
    scripts/test262_subset.py --root vendor/test262 --dir language/expressions
    scripts/test262_subset.py --root vendor/test262 --max 500
    scripts/test262_subset.py --root vendor/test262 --all-features
    scripts/test262_subset.py --root vendor/test262 --dir staging   # TC39 proposals (#5299)

See test-compat/test262/README.md.
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
from concurrent.futures import ThreadPoolExecutor, as_completed
from dataclasses import dataclass, field
from pathlib import Path

SCRIPT_DIR = Path(__file__).resolve().parent
REPO_ROOT = SCRIPT_DIR.parent
TEST262_DIR = REPO_ROOT / "test-compat" / "test262"
PREAMBLE = TEST262_DIR / "preamble.js"
# Node host shim: runs an assembled case as a *global script* (via
# vm.runInThisContext) rather than a CommonJS module, matching a real Test262
# host and Perry. Without it, module-scoped harness intrinsics are invisible to
# global-scope indirect eval, so Node spuriously rejects Annex B eval cases
# (#5346).
HOST_RUNNER = TEST262_DIR / "host-run.cjs"
# Feature tags Perry implements but the Node oracle cannot run (e.g. Temporal).
# Cases tagged with one of these are judged in self-validating mode (#4792):
# Perry-only, scored on whether the case's own `assert.*` self-checks throw.
SELF_VALIDATE_FEATURES_FILE = TEST262_DIR / "self-validate-features.txt"

# Default subtrees to walk (relative to <root>/test). Language + builtins are
# intl402 is now measured (Perry has substantial Intl: ~78% — the old "no ICU"
# assumption was stale). staging (TC39 proposals) stays out of scope.
DEFAULT_DIRS = ("language", "built-ins", "intl402")

# Subtrees skipped wholesale — out of scope for Perry's TS subset regardless of
# feature tags (some cases in these dirs carry no `features:`).
#
# History: `eval`, `intl402`, and `RegExp/property-escapes` USED to be skipped
# here as "AOT can't eval" / "no ICU" / "Rust regex crate gap". Those are all
# stale — Perry measures eval-code ~94%, intl402 ~78%, property-escapes ~87%, so
# they are no longer skipped (the eval cases run with PERRY_ALLOW_EVAL=1, set in
# the compile env below). `RegExp/lookbehind` was vestigial (no such subdir).
# Only genuinely-out-of-scope `staging` (proposals) remains skipped by default;
# pass `--dir staging` to measure it anyway (the guard is bypassed for any
# subtree the user names explicitly — see `discover`, #5299).
# NB: Temporal is judged in self-validating mode (#4792) — the Node oracle lacks
# it; under intl402/ the Temporal locale-format cases are now measured too.
_PATH_SKIP = re.compile(
    r"(?:^|/)(?:"
    r"staging"
    r")(?:/|$)"
)
# NB: Atomics/SharedArrayBuffer are now in scope (#4794). The agent-based cases
# (the bulk of them) still skip out via _HOST_DEP (`$262.agent`) and the
# CanBlock* flags in _SKIP_FLAGS, so only the single-thread cases run here.

# Cases that lean on $262 host intrinsics we don't provide: they'd throw under
# BOTH runtimes (a false "both reject" pass), so we skip them outright.
_HOST_DEP = re.compile(
    r"\$262\b|detachArrayBuffer|createRealm|evalScript|IsHTMLDDA|\bagent\."
)

# Frontmatter flags that make a case un-runnable as a plain script under this
# differential. `module` needs ESM loader semantics; the agent/Can-block flags
# need a multi-realm host.
_SKIP_FLAGS = {"module", "CanBlockIsFalse", "CanBlockIsTrue"}

# Environmental noise stripped before the stdout tiebreak.
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


def read_list(path: Path) -> list[str]:
    items = []
    for line in path.read_text().splitlines():
        line = line.strip()
        if line and not line.startswith("#"):
            items.append(line)
    return items


# --- frontmatter ----------------------------------------------------------

_FM = re.compile(r"/\*---(.*?)---\*/", re.DOTALL)
_INLINE_LIST = re.compile(r"\[(.*?)\]", re.DOTALL)


@dataclass
class Meta:
    flags: set[str] = field(default_factory=set)
    features: list[str] = field(default_factory=list)
    includes: list[str] = field(default_factory=list)
    negative: bool = False
    self_validate: bool = False  # judge Perry-only (oracle lacks the feature)


def parse_frontmatter(src: str) -> Meta | None:
    """Parse the `/*--- ... ---*/` YAML block. Returns None if absent.

    Deliberately a tolerant hand-rolled parser (no PyYAML dependency, matching
    the Node-core radar's stdlib-only style) for just the four fields we need:
    flags, features, includes, negative.
    """
    m = _FM.search(src)
    if not m:
        return None
    body = m.group(1)
    meta = Meta()

    def inline_items(line: str) -> list[str]:
        lm = _INLINE_LIST.search(line)
        if not lm:
            return []
        return [x.strip() for x in lm.group(1).split(",") if x.strip()]

    lines = body.splitlines()
    i = 0
    while i < len(lines):
        line = lines[i]
        stripped = line.strip()
        if stripped.startswith("flags:"):
            meta.flags = set(inline_items(stripped))
        elif stripped.startswith("features:"):
            # features may be inline `[a, b]` or a block list of `- a` lines.
            items = inline_items(stripped)
            if not items:
                j = i + 1
                while j < len(lines) and lines[j].lstrip().startswith("-"):
                    items.append(lines[j].lstrip()[1:].strip())
                    j += 1
                i = j - 1
            meta.features = items
        elif stripped.startswith("includes:"):
            items = inline_items(stripped)
            if not items:
                j = i + 1
                while j < len(lines) and lines[j].lstrip().startswith("-"):
                    items.append(lines[j].lstrip()[1:].strip())
                    j += 1
                i = j - 1
            meta.includes = items
        elif stripped.startswith("negative:"):
            meta.negative = True
        i += 1
    return meta


# --- assembly -------------------------------------------------------------


def assemble(src: str, meta: Meta, harness: Path, preamble_text: str) -> str:
    """Concatenate harness + preamble + includes + test the way TC39's runner
    does. Returns the full script text to hand to both runtimes."""
    if "raw" in meta.flags:
        return src  # raw: no harness, no strict prologue, source verbatim.

    parts: list[str] = []
    if "onlyStrict" in meta.flags:
        # The directive prologue must be the program's first token.
        parts.append('"use strict";')
    parts.append((harness / "sta.js").read_text())
    parts.append((harness / "assert.js").read_text())
    parts.append(preamble_text)
    if "async" in meta.flags:
        parts.append((harness / "doneprintHandle.js").read_text())
    for inc in meta.includes:
        parts.append((harness / inc).read_text())
    parts.append(src)
    return "\n".join(parts)


# --- discovery ------------------------------------------------------------


def discover(root: Path, dirs: list[str], applicable: set[str],
             all_features: bool, self_validate: set[str]):
    """Yield (relpath, src, meta) for every applicable, runnable case.

    `self_validate` is the set of feature tags the Node oracle can't run; a case
    carrying one is kept regardless of the `--all-features`/applicable gate and
    flagged `meta.self_validate` so the judge scores it Perry-only (#4792)."""
    test_root = root / "test"
    for d in dirs:
        base = test_root / d
        if not base.is_dir():
            continue
        # `_PATH_SKIP` guards the *default* wholesale walk (it keeps `staging`
        # out of the headline parity number). When the user deliberately names
        # a normally-skipped subtree — `--dir staging` (#5299) — honor it: the
        # whole point of the request is to measure that subtree, so don't let
        # the same-named guard filter every case back out.
        dir_opt_in = bool(_PATH_SKIP.search(d.strip("/") + "/"))
        for path in sorted(base.rglob("*.js")):
            rel = path.relative_to(test_root).as_posix()
            if path.name.endswith("_FIXTURE.js") or "_FIXTURE" in path.name:
                continue
            if not dir_opt_in and _PATH_SKIP.search(rel):
                continue
            try:
                src = path.read_text(encoding="utf-8", errors="replace")
            except OSError:
                continue
            meta = parse_frontmatter(src)
            if meta is None:
                continue  # not a Test262 case
            if meta.flags & _SKIP_FLAGS:
                continue
            if _HOST_DEP.search(src):
                continue
            meta.self_validate = bool(set(meta.features) & self_validate)
            # Self-validating cases bypass the applicable gate: the whole point
            # is to measure a feature (e.g. Temporal) the oracle can't run and
            # that is therefore absent from features-applicable.txt.
            if not meta.self_validate and not all_features and meta.features:
                if any(f not in applicable for f in meta.features):
                    continue
            yield rel, src, meta


# --- runner plumbing (shared shape with node_core_subset.py) --------------


@dataclass
class Sample:
    test: str
    reason: str


@dataclass
class Bucket:
    count: int = 0
    samples: list[Sample] = field(default_factory=list)

    def add(self, test: str, reason: str, cap: int) -> None:
        self.count += 1
        if len(self.samples) < cap:
            self.samples.append(Sample(test, reason[:300]))


def run(cmd, env, timeout, cwd=None):
    """Return (exit_code, combined stdout+stderr). 124 == timeout, 127 == ENOENT."""
    try:
        p = subprocess.run(cmd, env=env, cwd=cwd, stdout=subprocess.PIPE,
                           stderr=subprocess.STDOUT, timeout=timeout)
        return p.returncode, p.stdout.decode("utf-8", errors="replace")
    except subprocess.TimeoutExpired as e:
        out = e.stdout.decode("utf-8", errors="replace") if e.stdout else ""
        return 124, out
    except FileNotFoundError as e:
        return 127, str(e)


def first_line(text: str) -> str:
    for line in text.splitlines():
        s = line.strip()
        if s:
            return s
    return "(no output)"


def error_line(text: str) -> str:
    lines = [ln.strip() for ln in text.splitlines() if ln.strip()]
    for ln in lines:
        low = ln.lower()
        if ("error" in low or "panic" in low or "unsupported" in low
                or "not supported" in low or "undefined symbol" in low
                or "not implemented" in low):
            return ln
    return lines[-1] if lines else "(no output)"


def top_dir(rel: str) -> str:
    """`language/expressions/foo/bar.js` -> `language/expressions` for the
    per-category breakdown (depth-2, so the table stays readable)."""
    parts = rel.split("/")
    return "/".join(parts[:2]) if len(parts) >= 2 else parts[0]


def main() -> int:
    ap = argparse.ArgumentParser(description="Test262 subset radar (#799)")
    ap.add_argument("--root", type=Path, default=REPO_ROOT / "vendor" / "test262",
                    help="path to a tc39/test262 checkout (has test/ + harness/)")
    ap.add_argument("--dir", nargs="*", default=list(DEFAULT_DIRS),
                    help=f"subtrees under test/ to walk (default: {' '.join(DEFAULT_DIRS)})")
    ap.add_argument("--max", type=int, default=0,
                    help="cap total cases run (0 = no cap)")
    ap.add_argument("--all-features", action="store_true",
                    help="ignore features-applicable.txt (run every discovered case)")
    ap.add_argument("--timeout", type=int, default=20, help="per-test timeout (s)")
    ap.add_argument("--perry-bin", type=Path,
                    default=REPO_ROOT / "target" / "release" / "perry")
    ap.add_argument("--report", type=Path, default=TEST262_DIR / "report.json")
    ap.add_argument("--sample-cap", type=int, default=8,
                    help="failing-test samples recorded per bucket")
    ap.add_argument("--quiet", action="store_true")
    ap.add_argument("--jobs", type=int, default=1,
                    help="parallel workers (each test is an independent "
                         "compile+run; ~8x on an 8-core box)")
    ap.add_argument("--shard", type=str, default=None,
                    help="run only shard i of N as 'i/N' (0-based, strided over "
                         "the sorted case list) — for splitting across machines")
    args = ap.parse_args()

    shard_i = shard_n = None
    if args.shard:
        try:
            shard_i, shard_n = (int(x) for x in args.shard.split("/"))
            assert 0 <= shard_i < shard_n
        except (ValueError, AssertionError):
            print(f"error: --shard must be 'i/N' with 0<=i<N (got {args.shard!r})",
                  file=sys.stderr)
            return 2

    root = args.root.resolve()
    harness = root / "harness"
    if not (root / "test").is_dir() or not harness.is_dir():
        print(f"error: {root} is not a test262 checkout (need test/ + harness/).\n"
              f"Vendor it first:\n"
              f"  git clone --depth 1 https://github.com/tc39/test262 {root}",
              file=sys.stderr)
        return 2
    if not args.perry_bin.exists():
        print(f"error: perry binary not found at {args.perry_bin} "
              f"(cargo build --release -p perry)", file=sys.stderr)
        return 2

    applicable = set(read_list(TEST262_DIR / "features-applicable.txt"))
    self_validate = (set(read_list(SELF_VALIDATE_FEATURES_FILE))
                     if SELF_VALIDATE_FEATURES_FILE.exists() else set())
    preamble_text = PREAMBLE.read_text()
    pinned = (TEST262_DIR / "pinned-sha.txt").read_text().strip()

    base_env = dict(os.environ)
    base_env.update(FORCE_COLOR="0", NO_COLOR="1", NODE_DISABLE_COLORS="1")

    buckets = {k: Bucket() for k in
               ("pass", "diff", "runtime-fail", "compile-fail", "skip")}
    per_dir: dict[str, dict[str, int]] = {}
    neg_pass = 0  # negative cases where both runtimes correctly rejected
    self_judged = 0  # cases scored Perry-only (oracle lacks the feature)
    self_pass = 0    # of those, how many Perry passed
    judged_n = 0
    all_failures: list[dict] = []  # every non-pass case, uncapped

    stage = Path(tempfile.mkdtemp(prefix="test262-"))

    # Materialize the case list so we can shard / cap deterministically. The
    # shard is strided over the sorted list so each shard spans the whole
    # alphabet (avoids one shard getting all of the slow `built-ins/...`).
    cases = list(discover(root, args.dir, applicable, args.all_features,
                          self_validate))
    if shard_n:
        cases = cases[shard_i::shard_n]
    if args.max:
        cases = cases[:args.max]

    def judge_one(case):
        """Compile+run one case under its own temp dir (so workers don't clash)
        and return (rel, cat, bucket_key, reason, is_negative, is_self_validate)."""
        rel, src, meta = case
        cat = top_dir(rel)
        try:
            program = assemble(src, meta, harness, preamble_text)
        except OSError as e:
            return (rel, cat, "skip", f"assemble: {e}", False, meta.self_validate)
        workdir = Path(tempfile.mkdtemp(dir=stage))
        staged = workdir / "case.js"
        try:
            staged.write_text(program)
            if meta.self_validate:
                # Oracle (Node) can't run this feature — judge Perry alone on
                # the case's own `assert.*` self-checks (#4792).
                out_bin = workdir / "case.out"
                c_env = dict(base_env, PERRY_ALLOW_UNIMPLEMENTED="1",
                             PERRY_NO_AUTO_OPTIMIZE="1")
                c_exit, c_out = run(
                    [str(args.perry_bin), "compile", str(staged), "-o",
                     str(out_bin)], c_env, args.timeout, cwd=str(workdir))
                if c_exit != 0:
                    return (rel, cat, "compile-fail", error_line(c_out),
                            False, True)
                p_exit, p_out = run([str(out_bin)], base_env, args.timeout)
                ran_clean = p_exit == 0
                # Positive: pass iff it ran clean. Negative: pass iff it threw
                # the (unverified) expected error, i.e. exited non-zero.
                if ran_clean != meta.negative:
                    return (rel, cat, "pass", "", False, True)
                reason = (first_line(p_out) if not ran_clean
                          else "ran clean; expected a thrown error (negative)")
                return (rel, cat, "runtime-fail", reason, False, True)
            # 1) Node is the oracle (negative cases legitimately exit != 0).
            n_exit, n_out = run(["node", str(HOST_RUNNER), str(staged)],
                                base_env, args.timeout)
            node_clean = n_exit == 0
            # 2) Perry compile (permissive — unimplemented surfaces as a gap).
            out_bin = workdir / "case.out"
            c_env = dict(base_env, PERRY_ALLOW_UNIMPLEMENTED="1",
                         PERRY_NO_AUTO_OPTIMIZE="1", PERRY_ALLOW_EVAL="1")
            c_exit, c_out = run(
                [str(args.perry_bin), "compile", str(staged), "-o",
                 str(out_bin)], c_env, args.timeout, cwd=str(workdir))
            if c_exit != 0:
                if node_clean:
                    return (rel, cat, "compile-fail", error_line(c_out),
                            False, False)
                return (rel, cat, "pass", "", True, False)  # both reject (neg)
            # 3) Run the Perry binary.
            p_exit, p_out = run([str(out_bin)], base_env, args.timeout)
            perry_clean = p_exit == 0
            if node_clean and perry_clean:
                if normalize(p_out) == normalize(n_out):
                    return (rel, cat, "pass", "", False, False)
                return (rel, cat, "diff", first_line(p_out), False, False)
            if node_clean and not perry_clean:
                return (rel, cat, "runtime-fail", first_line(p_out),
                        False, False)
            if not node_clean and not perry_clean:
                return (rel, cat, "pass", "", True, False)  # both reject (neg)
            return (rel, cat, "runtime-fail",
                    "Perry ran clean; Node rejected (missed negative)",
                    False, False)
        finally:
            shutil.rmtree(workdir, ignore_errors=True)

    try:
        with ThreadPoolExecutor(max_workers=max(1, args.jobs)) as ex:
            futures = [ex.submit(judge_one, c) for c in cases]
            for fut in as_completed(futures):
                rel, cat, key, reason, is_neg, is_self = fut.result()
                counts = per_dir.setdefault(cat, {k: 0 for k in buckets})
                buckets[key].add(rel, reason, args.sample_cap)
                counts[key] += 1
                if is_neg:
                    neg_pass += 1
                if is_self and key != "skip":
                    self_judged += 1
                    if key == "pass":
                        self_pass += 1
                if key in ("diff", "runtime-fail", "compile-fail"):
                    all_failures.append({"test": rel, "bucket": key,
                                         "reason": reason})
    finally:
        shutil.rmtree(stage, ignore_errors=True)

    # Every failing test, uncapped (the capped `samples` above is just for the
    # console). Sorted for stable diffs between runs.
    all_failures.sort(key=lambda f: (f["bucket"], f["test"]))

    if not args.quiet:
        for cat in sorted(per_dir):
            c = per_dir[cat]
            judged = c["pass"] + c["diff"] + c["runtime-fail"] + c["compile-fail"]
            rate = f"{100 * c['pass'] / judged:.0f}%" if judged else "—"
            print(f"  {cat:<28} pass={c['pass']:<5} diff={c['diff']:<4} "
                  f"rt-fail={c['runtime-fail']:<4} "
                  f"compile-fail={c['compile-fail']:<5} "
                  f"skip={c['skip']:<4} parity={rate}")

    totals = {k: buckets[k].count for k in buckets}
    judged = (totals["pass"] + totals["diff"] + totals["runtime-fail"]
              + totals["compile-fail"])
    parity_pct = round(100 * totals["pass"] / judged, 1) if judged else 0.0

    report = {
        "generated_at": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
        "test262_pinned": pinned,
        "node_runtime": run(["node", "--version"], base_env, 10)[1].strip(),
        "dirs": args.dir,
        "all_features": args.all_features,
        "totals": totals,
        "judged": judged,
        "negative_agreements": neg_pass,
        "self_validated": {
            "features": sorted(self_validate),
            "judged": self_judged,
            "pass": self_pass,
            "pass_pct": (round(100 * self_pass / self_judged, 1)
                         if self_judged else 0.0),
        },
        "parity_pct": parity_pct,
        "per_dir": per_dir,
        "samples": {
            k: [s.__dict__ for s in buckets[k].samples]
            for k in ("diff", "runtime-fail", "compile-fail", "skip")
        },
        "failures": all_failures,
    }
    args.report.write_text(json.dumps(report, indent=2) + "\n")

    # Plain-text sidecar: every failing test path + bucket + reason, one per
    # line — so you never have to guess which tests are red.
    fail_txt = args.report.with_suffix(".failures.txt")
    fail_txt.write_text(
        "".join(f"{f['bucket']:<13} {f['test']}\t{f['reason']}\n"
                for f in all_failures))

    print()
    print("=" * 60)
    print(f"  Test262 subset radar (#799) — test262 {pinned}")
    print("=" * 60)
    for k in ("pass", "diff", "runtime-fail", "compile-fail", "skip"):
        print(f"  {k:<14} {totals[k]}")
    print(f"  {'judged':<14} {judged}   (excludes skip)")
    print(f"  {'of which neg':<14} {neg_pass}   (both runtimes correctly rejected)")
    if self_judged:
        sv_pct = round(100 * self_pass / self_judged, 1)
        feats = ", ".join(sorted(self_validate))
        print(f"  {'self-validated':<14} {self_pass}/{self_judged} = {sv_pct}%"
              f"   (Perry-only; oracle lacks: {feats})")
    print(f"  parity:        {parity_pct}%")
    print(f"  report:        {args.report}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
