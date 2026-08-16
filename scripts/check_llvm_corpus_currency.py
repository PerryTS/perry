#!/usr/bin/env python3
"""Keep the in-process LLVM dialect corpora from going quietly stale (#7982).

`crates/perry-codegen/src/dialect/tests.rs` builds three tracked `.ll` files
through the reader and asserts they construct and verify.  The CI job asserts
those tests RAN -- "a checkout missing the tracked .ll corpora would otherwise
green vacuously".  That assert is real, and it is not enough: it proves the
tests ran, not that they test today's IR.

Measured 2026-08-11: all three corpora were last refreshed 2026-08-03, 151
`crates/perry-codegen/src` commits earlier, and contained ZERO `addrspace(1)`
-- while RS4GC had meanwhile made `ptr addrspace(1)` the shape of every GC
root (497 sites in the spike module alone).  The unit gate was green through
three consecutive `main` failures of the end-to-end arm.  That is CLAUDE.md's
fourth way a gate cannot fail, occurring INSIDE the liveness assert written to
prevent it.  It had already happened once before, nine days earlier (#7310
refreshed the same files because they still carried pre-#7305 setjmp calls).

A fixture refreshed by hand goes stale again, so:

  * `scripts/refresh_llvm_inprocess_corpora.sh` regenerates all three, and
  * this script is the alarm.

WHAT THIS CATCHES, precisely -- and what it does not.  It asserts every IR form
the reader carries a dedicated branch for is PRESENT in the corpora.  So a form
that disappears (codegen stopped emitting it; the corpus was captured from a
build that could not) fails here, which by the knob kill-policy means either
refreshing the corpus or deleting the now-untested reader branch.

It cannot see a form codegen learns to emit that nothing has taught this table
about -- exactly #7982's shape.  Nothing static can: the honest closure for
that direction is the end-to-end `PERRY_LLVM_INPROCESS=native` arm, which
compiles today's IR through the reader by construction.  This table is what
makes the UNIT corpora non-decorative, and it is deliberately keyed on the
reader's own special-cases so that adding a branch without corpus coverage is
the thing that fails.
"""

from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
CORPUS_DIR = REPO / "experiments" / "llvm-inprocess-spike"

# form label -> (regex, corpora that must carry it, why the reader cares)
#
# "any" means at least one corpus.  EH forms are named on the EH corpus
# specifically, because that is the file whose whole purpose is to carry them.
REQUIRED_FORMS: dict[str, tuple[str, str, str]] = {
    "addrspace alloca": (
        r"^\s*%\S+ = alloca ptr addrspace\(\d+\)",
        "any",
        "RS4GC root slot; `basic_type`'s addrspace arm",
    ),
    "addrspace load": (
        r"^\s*%\S+ = load ptr addrspace\(\d+\),",
        "any",
        "root reload; `basic_type` in load type position",
    ),
    "addrspace store": (
        r"^\s*store ptr addrspace\(\d+\) ",
        "any",
        "root spill; `ty_and_val`'s addrspace extension (OPERAND position)",
    ),
    "addrspace inttoptr": (
        r"^\s*%\S+ = inttoptr .* to ptr addrspace\(\d+\)",
        "any",
        "NaN-box payload -> managed pointer; `basic_type` in cast dest",
    ),
    "addrspace ptrtoint": (
        r"^\s*%\S+ = ptrtoint ptr addrspace\(\d+\) ",
        "any",
        "managed pointer -> NaN-box; `ty_and_val` in cast source",
    ),
    "addrspace null operand": (
        r"^\s*store ptr addrspace\(\d+\) null,",
        "any",
        "`constant` must null in the OPERAND's address space, not addrspace(0)",
    ),
    "gc strategy": (
        r'^define .*\bgc "statepoint-example"',
        "any",
        "what makes RS4GC run at all; a module missing it has NO precise roots",
    ),
    "define string attribute": (
        r'^define .*"frame-pointer"="non-leaf"',
        "any",
        "string-attribute arm of the define-line attribute loop",
    ),
    "callsite string attribute": (
        r'^\s*(?:call|tail call) .*\)\s*"gc-leaf-function"',
        "any",
        "string-attribute arm of the callsite attribute match",
    ),
    "token cleanup landingpad": (
        r"^\s*%\S+ = landingpad token cleanup",
        "eh_text.ll",
        "the RS4GC-era pad shape; built through llvm-sys, not inkwell",
    ),
    "itanium catch landingpad": (
        r"^\s*%\S+ = landingpad \{ ptr, i32 \} catch ptr null",
        "any",
        "the pre-RS4GC pad shape the reader still has a branch for",
    ),
    "invoke edge": (
        r"^\s*(?:%\S+ = )?invoke ",
        "eh_text.ll",
        "#7302's invoke/landingpad support",
    ),
    "personality clause": (
        r"^define .*personality ptr @perry_eh_personality",
        "eh_text.ll",
        "personality is lifted out of the attribute loop by `parse_header`",
    ),
}

CORPORA = ("spike_text.ll", "batch_kernel.ll", "eh_text.ll")


def scan(text: str, pattern: str) -> int:
    return len(re.findall(pattern, text, flags=re.MULTILINE))


def check(root: Path) -> list[str]:
    problems: list[str] = []
    corpora: dict[str, str] = {}
    for name in CORPORA:
        path = root / "experiments" / "llvm-inprocess-spike" / name
        if not path.is_file():
            problems.append(f"{name}: tracked corpus is missing")
            continue
        corpora[name] = path.read_text(encoding="utf-8")
    if problems:
        return problems

    for label, (pattern, where, why) in REQUIRED_FORMS.items():
        if where == "any":
            total = sum(scan(text, pattern) for text in corpora.values())
            target = "any corpus"
        else:
            total = scan(corpora[where], pattern)
            target = where
        if total == 0:
            problems.append(
                f"{label}: absent from {target} -- the reader has a branch for it "
                f"({why}) that nothing exercises. Either refresh the corpora "
                f"(scripts/refresh_llvm_inprocess_corpora.sh) or delete the "
                f"reader branch; an untested mode is a decision nobody made."
            )
    return problems


def self_test() -> int:
    """The detector must be able to say no.

    Sabotage each direction with the exact shapes at issue: the stale corpus
    that shipped (no `addrspace`, `{ptr, i32}` pads only) must be rejected, and
    a current one accepted.
    """
    failures = []

    stale = (
        "define double @f(double %a) {\n"
        "  %r1 = alloca double\n"
        "  %r2 = landingpad { ptr, i32 } catch ptr null\n"
        "}\n"
    )
    current = (
        'define double @f(double %a) "frame-pointer"="non-leaf" '
        'gc "statepoint-example" personality ptr @perry_eh_personality {\n'
        "  %r1 = alloca ptr addrspace(1)\n"
        "  store ptr addrspace(1) null, ptr %r1\n"
        "  %r2 = load ptr addrspace(1), ptr %r1\n"
        "  %r3 = ptrtoint ptr addrspace(1) %r2 to i64\n"
        "  %r4 = inttoptr i64 %r3 to ptr addrspace(1)\n"
        "  store ptr addrspace(1) %r4, ptr %r1\n"
        '  call void @g(i64 %r3) "gc-leaf-function"\n'
        "  %r5 = invoke double @h() to label %c unwind label %p\n"
        "  %r6 = landingpad token cleanup\n"
        "  %r7 = landingpad { ptr, i32 } catch ptr null\n"
        "}\n"
    )

    for label, (pattern, _where, _why) in REQUIRED_FORMS.items():
        if scan(current, pattern) == 0:
            failures.append(f"the current-IR sample does not match `{label}`")

    stale_missed = [
        label
        for label, (pattern, _w, _y) in REQUIRED_FORMS.items()
        if scan(stale, pattern) > 0 and "addrspace" in label
    ]
    if stale_missed:
        failures.append(f"the pre-RS4GC sample matched addrspace forms: {stale_missed}")

    # The itanium pad must still be recognised in BOTH samples: it is a form
    # the reader keeps, not one this change retires.
    if scan(stale, REQUIRED_FORMS["itanium catch landingpad"][0]) == 0:
        failures.append("the itanium pad pattern stopped matching its own shape")

    for name in CORPORA:
        if not (CORPUS_DIR / name).is_file():
            failures.append(f"{name} is not where this script expects it")

    for failure in failures:
        print(f"llvm-corpus-currency self-test FAILED: {failure}", file=sys.stderr)
    if failures:
        return 1
    print(
        "llvm-corpus-currency self-test: OK "
        "(today's IR shapes are all matched; the 2026-08-03 corpus shape is rejected)"
    )
    return 0


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args()
    if args.self_test:
        return self_test()

    problems = check(REPO)
    if problems:
        print("llvm-inprocess corpus currency FAILED:", file=sys.stderr)
        for problem in problems:
            print(f"  - {problem}", file=sys.stderr)
        return 1
    print(
        f"llvm-inprocess corpus currency OK: {len(REQUIRED_FORMS)} reader-supported "
        f"IR forms present across {len(CORPORA)} corpora"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
