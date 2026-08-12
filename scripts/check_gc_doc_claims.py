#!/usr/bin/env python3
"""Hold the current GC documentation to claims a machine can re-derive.

The collector moved ~200 commits in four days and the reference pages drifted
behind it in three shapes, each of which this checker owns one rule for:

1. **Paths that stopped existing.**  `memory-model.md`'s entire source map
   still pointed at the deleted monolithic `crates/perry-runtime/src/gc.rs`,
   with line numbers, long after the split into `gc/`.  A path claim is
   verifiable, so it is verified; a `path:LINE` citation is not maintainable at
   all and is rejected outright in favour of naming the symbol.

2. **Numbers that stopped matching.**  `PROMOTION_AGE = 2` outlived the
   adaptive 1-4 tenuring threshold; `~55 registered scanners` outlived a
   population of 123.  A documented constant now carries a `gc-fact` marker
   naming the `const` it came from, and the marker is compared against the
   source.

3. **Issue numbers standing in for facts.**  "that phase is atomic and unsliced
   (#7874)" was false the moment #7874 closed, and nothing in the tree noticed.
   The current operations page therefore names no issue at all: it says what
   the code does today, which the two rules above can check, while trackers
   live in the tracker.

WHAT THIS DOES NOT CATCH (say it plainly, per CLAUDE.md's gate rules)
--------------------------------------------------------------------
Prose that is simply wrong about behaviour.  No regex knows that "walks the
whole arena" stopped being true.  The defence against that class is to bind the
load-bearing numbers to `gc-fact` markers, which is why rule 2 exists and why
adding a number to these pages should mean adding a marker.

Usage:
    python3 scripts/check_gc_doc_claims.py             # check the repo
    python3 scripts/check_gc_doc_claims.py --self-test # check the checker
    python3 scripts/check_gc_doc_claims.py --list      # describe the rules
"""

from __future__ import annotations

import argparse
import ast
import re
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent

# Pages that must describe the collector that ships.  Keep this list path-exact:
# a new page does not inherit membership, and a historical journal must never
# acquire it (their opening decisions are dated, not current).
CURRENT_DOCS = (
    "docs/src/internals/garbage-collector.md",
    "docs/src/internals/memory-model.md",
    "docs/src/internals/gc-rooting-invariant.md",
    "benchmarks/gc_ratchet/README.md",
    "CLAUDE.md",
)

# The operations page is the one a reader is told to trust without reconciling
# anything.  It carries the strict form of rule 3.
OPERATIONS_PAGE = "docs/src/internals/garbage-collector.md"

# Rule 1.  A claim is a path only if it starts at a real top-level directory --
# `arena/promote.rs` in running prose is a module reference, not a claim to
# check, and treating it as one would produce noise instead of a gate.
PATH_ROOTS = (
    "crates/",
    "scripts/",
    "benchmarks/",
    "docs/",
    "test-parity/",
    "test-files/",
    ".github/",
)

# `changelog.d/` fragments are deleted at tag time by design, so their absence
# is not drift.  Globs are patterns, not paths.
PATH_EXEMPT_PREFIXES = ("changelog.d/",)

INLINE_CODE_RE = re.compile(r"`([^`\n]+)`")
LINE_CITATION_RE = re.compile(r"^(?P<path>[^\s:]+):(?P<line>\d+)$")

# Rule 2.  `<!-- gc-fact: NAME = VALUE in PATH -->` binds a documented number to
# the constant it came from; `<!-- gc-symbol: NAME in PATH -->` binds a
# documented "look for this here" pointer to a definition that still exists.
FACT_RE = re.compile(
    r"<!--\s*gc-fact:\s*(?P<name>[A-Za-z_][A-Za-z0-9_]*)\s*=\s*(?P<value>.+?)"
    r"\s+in\s+(?P<path>\S+?)\s*-->"
)
SYMBOL_RE = re.compile(
    r"<!--\s*gc-symbol:\s*(?P<name>[A-Za-z_][A-Za-z0-9_]*)\s+in\s+(?P<path>\S+?)\s*-->"
)
SYMBOL_DEF_RE = r"(?:fn|struct|enum|trait|type|const|static|macro_rules!)\s+{name}\b"
RUST_ITEM_RE = r"(?:pub(?:\([^)]*\))?\s+)?(?:const|static)\s+{name}\s*:\s*[^=]+=\s*(?P<rhs>[^;]+);"

# Rule 3.  Tracker attribution outside the operations page: an issue number
# standing in as the authority for a present-tense claim.
ISSUE_RE = re.compile(r"#\d{3,5}\b")
TRACKER_ATTRIBUTION_RES = (
    re.compile(r"#\d{3,5}\s+(?:tracks|is\s+tracking|remains\s+open|is\s+still\s+open)\b"),
    re.compile(r"(?:tracked\s+(?:by|in)|blocked\s+on|awaiting)\s+#\d{3,5}\b"),
)

# Ratchet/report tables legitimately name issue numbers as the provenance of a
# pinned measurement.  Rule 3's strict arm is the operations page only, so this
# stays a one-line note rather than an exemption list.


def eval_int(expr: str) -> int | None:
    """Evaluate a Rust integer literal expression, or return None."""
    cleaned = re.sub(r"_", "", expr.strip())
    cleaned = re.sub(r"\b(\d+)(?:u8|u16|u32|u64|usize|i8|i16|i32|i64|isize)\b", r"\1", cleaned)
    try:
        tree = ast.parse(cleaned, mode="eval")
    except SyntaxError:
        return None
    allowed = (
        ast.Expression,
        ast.BinOp,
        ast.UnaryOp,
        ast.Constant,
        ast.Add,
        ast.Sub,
        ast.Mult,
        ast.LShift,
        ast.USub,
    )
    for node in ast.walk(tree):
        if not isinstance(node, allowed):
            return None
        if isinstance(node, ast.Constant) and not isinstance(node.value, int):
            return None
    try:
        return int(eval(compile(tree, "<gc-fact>", "eval")))  # noqa: S307 - AST whitelisted above
    except (ValueError, TypeError, ZeroDivisionError):
        return None


def values_agree(documented: str, source: str) -> bool:
    doc_int, src_int = eval_int(documented), eval_int(source)
    if doc_int is not None and src_int is not None:
        return doc_int == src_int
    return documented.strip() == source.strip()


def path_problems(rel: str, text: str, root: Path) -> list[str]:
    problems = []
    seen = set()
    for span in INLINE_CODE_RE.findall(text):
        # A code span may be a whole command (`scripts/x.sh <binary>`); the path
        # claim is its first token.
        candidate = span.strip().split()[0].rstrip(".,;") if span.strip() else ""
        if candidate in seen:
            continue
        seen.add(candidate)
        citation = LINE_CITATION_RE.match(candidate)
        target = citation.group("path") if citation else candidate
        if not target.startswith(PATH_ROOTS) or "*" in target:
            continue
        if target.startswith(PATH_EXEMPT_PREFIXES):
            continue
        if citation:
            problems.append(
                f"{rel}: `{candidate}` cites a line number. Line numbers drift silently; "
                "name the file and the symbol instead."
            )
            continue
        if not (root / target).exists():
            problems.append(f"{rel}: `{target}` does not exist")
    return problems


def fact_problems(rel: str, text: str, root: Path) -> list[str]:
    problems = []
    for match in FACT_RE.finditer(text):
        name, documented, path = match.group("name"), match.group("value"), match.group("path")
        source_path = root / path
        if not source_path.is_file():
            problems.append(f"{rel}: gc-fact {name} names {path}, which is not a file")
            continue
        source = source_path.read_text(encoding="utf-8")
        item = re.search(RUST_ITEM_RE.format(name=re.escape(name)), source)
        if item is None:
            problems.append(
                f"{rel}: gc-fact {name} is not defined in {path} "
                "(renamed or deleted -- update the page, not the marker)"
            )
            continue
        if not values_agree(documented, item.group("rhs")):
            problems.append(
                f"{rel}: gc-fact {name} documents {documented.strip()!r} "
                f"but {path} defines {item.group('rhs').strip()!r}"
            )
    for match in SYMBOL_RE.finditer(text):
        name, path = match.group("name"), match.group("path")
        source_path = root / path
        if not source_path.is_file():
            problems.append(f"{rel}: gc-symbol {name} names {path}, which is not a file")
            continue
        source = source_path.read_text(encoding="utf-8")
        if not re.search(SYMBOL_DEF_RE.format(name=re.escape(name)), source):
            problems.append(
                f"{rel}: gc-symbol {name} is not defined in {path} "
                "(moved or renamed -- the source map is pointing at nothing)"
            )
    return problems


def tracker_problems(rel: str, text: str) -> list[str]:
    problems = []
    if rel == OPERATIONS_PAGE:
        for hit in sorted(set(ISSUE_RE.findall(text))):
            problems.append(
                f"{rel}: names {hit}. The operations page states what the collector does "
                "today (checkable) -- an issue reference becomes false when the issue "
                "closes, and nothing in the tree notices."
            )
        return problems
    seen = set()
    for pattern in TRACKER_ATTRIBUTION_RES:
        for match in pattern.finditer(text):
            phrase = " ".join(match.group(0).split())
            if phrase in seen:
                continue
            seen.add(phrase)
            problems.append(
                f"{rel}: attributes a current claim to a tracker ({phrase}). "
                "State what the code does; the tracker's state is not in this tree."
            )
    return problems


def check_document(rel: str, text: str, root: Path) -> list[str]:
    return (
        path_problems(rel, text, root)
        + fact_problems(rel, text, root)
        + tracker_problems(rel, text)
    )


def check_repo(root: Path) -> list[str]:
    problems = []
    facts = 0
    for rel in CURRENT_DOCS:
        path = root / rel
        if not path.is_file():
            problems.append(f"{rel}: checked document is missing")
            continue
        text = path.read_text(encoding="utf-8")
        facts += len(FACT_RE.findall(text))
        problems.extend(check_document(rel, text, root))
    # A rule that inspects nothing passes vacuously.  The fact rule is the one
    # with a population that can silently go to zero (delete every marker and it
    # is green), so it asserts its own subject was live.
    if facts < MIN_FACTS:
        problems.append(
            f"only {facts} gc-fact markers found, expected at least {MIN_FACTS}. "
            "Rule 2 is the only defence against a documented number drifting; "
            "removing markers instead of fixing them disarms it."
        )
    return problems


# Floor for the fact rule.  Raise it when a page gains markers; never lower it
# to make a red build green.
MIN_FACTS = 8


def self_test() -> int:
    failures = []
    root = REPO

    if eval_int("16 * 1024") != 16384 or eval_int("1_048_576") != 1048576:
        failures.append("integer normalisation is broken")
    if eval_int("__import__('os')") is not None:
        failures.append("the expression evaluator admitted a non-literal")
    if not values_agree("16 * 1024", "16384") or values_agree("2", "4"):
        failures.append("value comparison is broken")

    # Rule 1: a dead path and a line citation must both fail; a live one must not.
    dead = path_problems("doc.md", "see `crates/perry-runtime/src/gc.rs`", root)
    if not dead:
        failures.append("rule 1 passed a path that does not exist")
    cited = path_problems("doc.md", "see `scripts/check_gc_doc_claims.py:1`", root)
    if not any("line number" in problem for problem in cited):
        failures.append("rule 1 passed a line-number citation")
    live = path_problems("doc.md", "see `scripts/check_gc_doc_claims.py`", root)
    if live:
        failures.append(f"rule 1 failed a path that exists: {live}")
    exempt = path_problems("doc.md", "see `changelog.d/0000-not-a-real-fragment.md`", root)
    if exempt:
        failures.append("rule 1 failed an intentionally ephemeral changelog fragment")

    # Rule 2: the marker must fail on a drifted value, a renamed const, and a
    # dead file -- and pass when it agrees with the source.
    real = "docs/src/internals/garbage-collector.md"
    const_file = "crates/perry-runtime/src/gc/types.rs"
    ok = fact_problems(
        real, f"<!-- gc-fact: LARGE_OBJECT_THRESHOLD_BYTES = 16 * 1024 in {const_file} -->", root
    )
    if ok:
        failures.append(f"rule 2 failed a marker that matches the source: {ok}")
    drifted = fact_problems(
        real, f"<!-- gc-fact: LARGE_OBJECT_THRESHOLD_BYTES = 4 * 1024 in {const_file} -->", root
    )
    if not drifted:
        failures.append("rule 2 passed a drifted value")
    renamed = fact_problems(
        real, f"<!-- gc-fact: DELETED_CONSTANT_NAME = 1 in {const_file} -->", root
    )
    if not renamed:
        failures.append("rule 2 passed a constant that is not defined")
    missing = fact_problems(real, "<!-- gc-fact: X = 1 in crates/gone/src/gone.rs -->", root)
    if not missing:
        failures.append("rule 2 passed a marker naming a file that does not exist")
    sym_ok = fact_problems(
        real, "<!-- gc-symbol: gc_malloc in crates/perry-runtime/src/gc/malloc.rs -->", root
    )
    if sym_ok:
        failures.append(f"rule 2 failed a symbol that is defined: {sym_ok}")
    sym_bad = fact_problems(
        real, "<!-- gc-symbol: gc_malloc in crates/perry-runtime/src/gc/tenuring.rs -->", root
    )
    if not sym_bad:
        failures.append("rule 2 passed a symbol that is not defined in the named file")

    # Rule 3: strict on the operations page, attribution-shaped elsewhere.
    strict = tracker_problems(OPERATIONS_PAGE, "the phase is unsliced (#7874).")
    if not strict:
        failures.append("rule 3 passed an issue reference on the operations page")
    loose_bad = tracker_problems("docs/src/internals/memory-model.md", "#7875 tracks that residue.")
    if not loose_bad:
        failures.append("rule 3 passed a tracker attribution off the operations page")
    loose_ok = tracker_problems(
        "docs/src/internals/memory-model.md",
        "#6882 set it from `js_gc_init` and was inert until #7450 moved it pre-`main`.",
    )
    if loose_ok:
        failures.append(f"rule 3 failed a historical, non-attributive issue reference: {loose_ok}")

    # The vacuity floor must be able to fail.
    if MIN_FACTS < 1:
        failures.append("MIN_FACTS floor cannot fail")

    for failure in failures:
        print(f"check_gc_doc_claims self-test: {failure}", file=sys.stderr)
    if failures:
        return 1
    print("check_gc_doc_claims self-test: OK")
    return 0


def describe() -> int:
    print(__doc__.strip())
    print("\nChecked documents:")
    for rel in CURRENT_DOCS:
        strict = " (strict: no issue references)" if rel == OPERATIONS_PAGE else ""
        print(f"  {rel}{strict}")
    return 0


def main() -> int:
    parser = argparse.ArgumentParser(description="Check current GC documentation claims")
    parser.add_argument("--self-test", action="store_true")
    parser.add_argument("--list", action="store_true")
    args = parser.parse_args()
    if args.self_test:
        return self_test()
    if args.list:
        return describe()

    problems = check_repo(REPO)
    if problems:
        for problem in problems:
            print(f"check_gc_doc_claims: {problem}", file=sys.stderr)
        print(f"check_gc_doc_claims: FAILED ({len(problems)} problems)", file=sys.stderr)
        return 1
    facts = sum(
        len(FACT_RE.findall((REPO / rel).read_text(encoding="utf-8"))) for rel in CURRENT_DOCS
    )
    print(f"check_gc_doc_claims: OK — {len(CURRENT_DOCS)} documents, {facts} facts re-derived")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
