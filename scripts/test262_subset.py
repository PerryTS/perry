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
from dataclasses import dataclass, field
from pathlib import Path

SCRIPT_DIR = Path(__file__).resolve().parent
REPO_ROOT = SCRIPT_DIR.parent
TEST262_DIR = REPO_ROOT / "test-compat" / "test262"
PREAMBLE = TEST262_DIR / "preamble.js"

# Default subtrees to walk (relative to <root>/test). Language + builtins are
# the cleanest TS-subset denominator; intl402/staging are out of scope.
DEFAULT_DIRS = ("language", "built-ins")

# Subtrees skipped wholesale — out of scope for Perry's TS subset regardless of
# feature tags (some cases in these dirs carry no `features:`).
_PATH_SKIP = re.compile(
    r"(?:^|/)(?:"
    r"intl402|staging|"
    r"eval|"  # dynamic eval — Perry is AOT
    r"Atomics|SharedArrayBuffer|"  # no shared heap
    r"Temporal|"  # not implemented
    r"RegExp/(?:lookbehind|property-escapes)"  # Rust regex crate gaps
    r")(?:/|$)"
)

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
             all_features: bool):
    """Yield (relpath, src, meta) for every applicable, runnable case."""
    test_root = root / "test"
    for d in dirs:
        base = test_root / d
        if not base.is_dir():
            continue
        for path in sorted(base.rglob("*.js")):
            rel = path.relative_to(test_root).as_posix()
            if path.name.endswith("_FIXTURE.js") or "_FIXTURE" in path.name:
                continue
            if _PATH_SKIP.search(rel):
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
            if not all_features and meta.features:
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
    args = ap.parse_args()

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
    preamble_text = PREAMBLE.read_text()
    pinned = (TEST262_DIR / "pinned-sha.txt").read_text().strip()

    base_env = dict(os.environ)
    base_env.update(FORCE_COLOR="0", NO_COLOR="1", NODE_DISABLE_COLORS="1")

    buckets = {k: Bucket() for k in
               ("pass", "diff", "runtime-fail", "compile-fail", "skip")}
    per_dir: dict[str, dict[str, int]] = {}
    neg_pass = 0  # negative cases where both runtimes correctly rejected
    judged_n = 0

    stage = Path(tempfile.mkdtemp(prefix="test262-"))
    src_dir = stage / "src"
    bin_dir = stage / "bin"
    src_dir.mkdir()
    bin_dir.mkdir()
    try:
        for rel, src, meta in discover(root, args.dir, applicable,
                                       args.all_features):
            if args.max and judged_n >= args.max:
                break
            cat = top_dir(rel)
            counts = per_dir.setdefault(
                cat, {k: 0 for k in buckets})

            # Assemble (skip the case if an include is missing).
            try:
                program = assemble(src, meta, harness, preamble_text)
            except OSError as e:
                buckets["skip"].add(rel, f"assemble: {e}", args.sample_cap)
                counts["skip"] += 1
                continue

            staged = src_dir / "case.js"
            staged.write_text(program)

            # 1) Node is the oracle (negative cases legitimately exit != 0).
            n_exit, n_out = run(["node", str(staged)], base_env, args.timeout)
            node_clean = n_exit == 0

            # 2) Perry: compile (permissive — unimplemented surfaces as gap).
            out_bin = bin_dir / "case.out"
            c_env = dict(base_env, PERRY_ALLOW_UNIMPLEMENTED="1",
                         PERRY_NO_AUTO_OPTIMIZE="1")
            c_exit, c_out = run(
                [str(args.perry_bin), "compile", str(staged), "-o", str(out_bin)],
                c_env, args.timeout, cwd=str(bin_dir))
            judged_n += 1

            if c_exit != 0:
                # Perry rejected at compile time.
                if node_clean:
                    buckets["compile-fail"].add(rel, error_line(c_out),
                                                args.sample_cap)
                    counts["compile-fail"] += 1
                else:
                    # Negative case, parse/early phase — both reject. Correct.
                    buckets["pass"].add(rel, "", args.sample_cap)
                    counts["pass"] += 1
                    neg_pass += 1
                continue

            # 3) Run the Perry binary.
            p_exit, p_out = run([str(out_bin)], base_env, args.timeout)
            try:
                out_bin.unlink()
            except OSError:
                pass
            perry_clean = p_exit == 0

            if node_clean and perry_clean:
                if normalize(p_out) == normalize(n_out):
                    buckets["pass"].add(rel, "", args.sample_cap)
                    counts["pass"] += 1
                else:
                    buckets["diff"].add(rel, first_line(p_out), args.sample_cap)
                    counts["diff"] += 1
            elif node_clean and not perry_clean:
                buckets["runtime-fail"].add(rel, first_line(p_out),
                                            args.sample_cap)
                counts["runtime-fail"] += 1
            elif not node_clean and not perry_clean:
                # Negative case, runtime phase — both reject. Correct.
                buckets["pass"].add(rel, "", args.sample_cap)
                counts["pass"] += 1
                neg_pass += 1
            else:  # Node rejected, Perry ran clean — a missed negative.
                buckets["runtime-fail"].add(
                    rel, "Perry ran clean; Node rejected (missed negative)",
                    args.sample_cap)
                counts["runtime-fail"] += 1
    finally:
        shutil.rmtree(stage, ignore_errors=True)

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
        "parity_pct": parity_pct,
        "per_dir": per_dir,
        "samples": {
            k: [s.__dict__ for s in buckets[k].samples]
            for k in ("diff", "runtime-fail", "compile-fail", "skip")
        },
    }
    args.report.write_text(json.dumps(report, indent=2) + "\n")

    print()
    print("=" * 60)
    print(f"  Test262 subset radar (#799) — test262 {pinned}")
    print("=" * 60)
    for k in ("pass", "diff", "runtime-fail", "compile-fail", "skip"):
        print(f"  {k:<14} {totals[k]}")
    print(f"  {'judged':<14} {judged}   (excludes skip)")
    print(f"  {'of which neg':<14} {neg_pass}   (both runtimes correctly rejected)")
    print(f"  parity:        {parity_pct}%")
    print(f"  report:        {args.report}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
