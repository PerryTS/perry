#!/usr/bin/env python3
"""GC_FLAG_PINNED custody gate (#7645).

The copying minor skips its eligibility preflight — an O(live young graph)
traversal whose whole job is to prove nothing reachable is pinned — whenever
the young-pin latch in `crates/perry-runtime/src/gc/pin.rs` is clear. That
substitution is sound only if EVERY creation of a pin goes through
`gc::pin_object`, which is what arms the latch. A pin created any other way
leaves the collector free to relocate a pinned object, and the holders of those
pins keep raw addresses no scanner rewrites — i.e. a use-after-move.

So the completeness of the pin-site list is load-bearing, and a list in a
comment is not a gate. This script is the gate.

Two rules
---------

**A. named** — the token `GC_FLAG_PINNED` may only appear in a *masking*
position (`flags & (… | GC_FLAG_PINNED)`, `gc_flags &= !GC_FLAG_PINNED`), i.e.
reading or clearing the bit. Anything that ORs or assigns it into a flags byte
is a creation and must live in `gc/pin.rs` or be allowlisted.

**B. raw byte** — any write (`=`/`|=`) into an identifier whose name contains
`gc_flags` whose right-hand side carries an integer literal with bit 2 set.

Rule B is not redundant. When this gate was written, two of the six production
pin sites — `perry-ui-macos`'s textfield and table string reads — wrote
`*gc_flags_ptr |= 0x04;` and were invisible to `grep GC_FLAG_PINNED`. A gate
that knew only rule A would have certified a pin-site list missing a third of
its entries.

How it fails
------------

* a creation site that is not `pin_object` and is not allowlisted -> exit 1
* an allowlist entry that matches nothing any more -> exit 1 (a stale exemption
  is how these gates rot; `deferred_registration_flush_sites` in
  `crates/perry-runtime/src/arena/tests.rs` fails the same way)
* fewer than MIN_TOKENS `GC_FLAG_PINNED` tokens seen -> exit 2, because a regex
  that stopped matching would otherwise report a clean, empty, green run

`--self-test` plants every offender shape in a temp tree and requires the
scanner to reject each one, and requires it NOT to flag the legitimate
read/clear/preserve shapes. Run it before trusting a green scan.

What this gate CANNOT see
-------------------------

A flags byte reconstructed through a variable (`let preserved = flags &
(… | GC_FLAG_PINNED); (*new).gc_flags = … | preserved;` — `move_young`'s
copy) carries an existing pin forward but cannot originate one: the mask can
only pass through a bit the source object already had, and that object's pin
went through `pin_object`. Preservation is safe; creation is what is gated.

The two other channels that write a header's flag byte were checked by hand
and cannot originate a pin either, so they are not scanned:

* the allocators seed `GC_FLAG_ARENA | gc_birth_extra_flags()`, and
  `GC_BIRTH_EXTRA_FLAGS` is only ever `0` or `GC_FLAG_MARKED`
  (`gc/barrier.rs`, `gc/cycle.rs` — the only two writers);
* codegen's inline bump allocators (`perry-codegen`'s `array_literal.rs`,
  `lower_call/new.rs`) emit the same `GC_FLAG_ARENA = 0x02` plus that same
  birth byte.

Both are worth re-checking if either ever grows a third flag source.

One shape is genuinely out of reach of a textual scan: a numeric bit built up
through an intermediate local (`let mut f = (*h).gc_flags; f |= 4;
(*h).gc_flags = f;`). Rule A sees no token and rule B sees no literal on a
`gc_flags` target. That shape is covered by the SECOND layer instead — the
`move_young` check that aborts when a preflight-skipped cycle is about to
relocate a pinned object — which is why the change carries a runtime guard and
not only this script.
"""

from __future__ import annotations

import argparse
import os
import re
import sys
import tempfile
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent

# `GC_FLAG_PINNED` is bit 2 (0x04) of GcHeader::gc_flags.
PINNED_BIT = 0x04

TOKEN = "GC_FLAG_PINNED"

# Rule B: an assignment into any identifier containing `gc_flags`. Covers the
# struct-field form (`(*header).gc_flags |= X`) and the raw-byte-pointer form
# (`*gc_flags_ptr |= X`) alike.
FLAG_WRITE = re.compile(r"\bgc_flags\w*\s*(?P<op>\|=|=)(?!=)(?P<rhs>[^;]*)")

INT_LITERAL = re.compile(r"0[xX](?P<hex>[0-9A-Fa-f_]+)|\b(?P<dec>[0-9][0-9_]*)\b")

STRING_LITERAL = re.compile(r'"(?:[^"\\]|\\.)*"', re.S)

# `let gc_flags = ...` is a local read, not a header write.
LET_BINDING = re.compile(r"\blet\s+(?:mut\s+)?gc_flags\w*\s*[:=]")

# Sites that are allowed to originate a pin outside `pin_object`.
#   (relative path, line substring, why)
# An entry that matches nothing is a failure — delete it when the site goes.
ALLOWLIST: list[tuple[str, str, str]] = [
    (
        "crates/perry-runtime/src/gc/malloc.rs",
        "pinned = state.push_test_object(64, GC_FLAG_PINNED)",
        "MallocState::push_test_object seeds a synthetic malloc-space header "
        "for the drop_tests. Malloc space is never Eden/FromSurvivor "
        "(CopyingPointerSet::classify_arena), so a pin there can never "
        "constrain the copying minor and need not arm the latch.",
    ),
    (
        "crates/perry-runtime/src/gc/tests/copying/latch.rs",
        "(*sabotage_plant_7645).gc_flags |= GC_FLAG_PINNED",
        "the latch sabotage test deliberately plants a young pin WITHOUT "
        "arming the latch, to prove the dynamic move_young guard catches an "
        "incomplete latch. It must stay a raw write or it tests nothing.",
    ),
    (
        "crates/perry-runtime/src/gc/tests/forwarding_target_validation.rs",
        "(*fake_header).gc_flags = 0x86;",
        "#8174 plants the RECYCLED BYTES #8040 observed at a dead side-table "
        "key, verbatim, and reads them as a GcHeader. 0x86 happens to carry "
        "bit 2, but nothing here is pinning anything: the address is payload "
        "interior of a live allocation and there is no object at it. Writing "
        "the bytes as named flags would be a lie about what was measured.",
    ),
]

# Floor: the tree currently carries ~80 `GC_FLAG_PINNED` tokens. A regex that
# silently stopped matching would report zero offenders and pass.
MIN_TOKENS = 40


def strip_comments(text: str) -> str:
    out = []
    for line in text.splitlines():
        out.append(line.split("//", 1)[0])
    return "\n".join(out)


def strip_strings(text: str) -> str:
    """Blank out string literals, keeping the line count.

    A `GC_FLAG_PINNED` inside a diagnostic message (the `move_young` abort
    reporter says the name out loud) is prose, not a pin.
    """
    return STRING_LITERAL.sub(lambda m: '"' + ("\n" * m.group(0).count("\n")) + '"', text)


def statements(text: str):
    """Yield (statement_text, line_number_of_statement_start)."""
    line = 1
    start = 0
    for index, char in enumerate(text):
        if char == "\n":
            line += 1
        if char == ";" or char == "{" or char == "}":
            chunk = text[start:index]
            if chunk.strip():
                yield chunk, line - chunk.count("\n")
            start = index + 1
    tail = text[start:]
    if tail.strip():
        yield tail, line - tail.count("\n")


def named_offenders(rel: str, text: str) -> tuple[list[tuple[str, int, str]], int]:
    """Rule A. Returns (offenders, tokens seen)."""
    offenders: list[tuple[str, int, str]] = []
    tokens = 0
    code = strip_strings(strip_comments(text))
    for chunk, lineno in statements(code):
        if TOKEN not in chunk:
            continue
        flat = " ".join(chunk.split())
        if flat.startswith("use ") or re.search(r"\bconst\s+GC_FLAG_PINNED\b", flat):
            tokens += flat.count(TOKEN)
            continue
        # `&=` clears; normalise it to a plain mask so `x &= !PINNED` reads as
        # a mask rather than as an assignment.
        probe = flat.replace("&=", "& ")
        for match in re.finditer(re.escape(TOKEN), probe):
            tokens += 1
            before = probe[: match.start()]
            last_and = before.rfind("&")
            last_assign = max(before.rfind("|="), before.rfind("="))
            if last_and > last_assign:
                continue  # masking position: a read or a clear
            offenders.append((rel, lineno, flat[:200]))
    return offenders, tokens


def raw_byte_offenders(rel: str, text: str) -> list[tuple[str, int, str]]:
    """Rule B."""
    offenders: list[tuple[str, int, str]] = []
    for lineno, line in enumerate(strip_comments(text).splitlines(), start=1):
        if "gc_flags" not in line or LET_BINDING.search(line):
            continue
        for match in FLAG_WRITE.finditer(line):
            rhs = STRING_LITERAL.sub("", match.group("rhs"))
            if TOKEN in rhs:
                continue  # rule A owns the named form
            for literal in INT_LITERAL.finditer(rhs):
                raw = literal.group("hex")
                value = (
                    int(raw.replace("_", ""), 16)
                    if raw
                    else int(literal.group("dec").replace("_", ""))
                )
                if value & PINNED_BIT:
                    offenders.append((rel, lineno, line.strip()[:200]))
                    break
    return offenders


def scan(root: Path) -> tuple[list[tuple[str, int, str]], int]:
    found: list[tuple[str, int, str]] = []
    tokens = 0
    crates = root / "crates"
    for path in sorted(crates.rglob("*.rs")):
        if "/target/" in str(path):
            continue
        rel = path.relative_to(root).as_posix()
        try:
            text = path.read_text(encoding="utf-8", errors="replace")
        except OSError:
            continue
        # `pin.rs` IS the sanctioned setter; rule A does not apply inside it.
        if not rel.endswith("gc/pin.rs"):
            named, seen = named_offenders(rel, text)
            found.extend(named)
            tokens += seen
        else:
            tokens += text.count(TOKEN)
        found.extend(raw_byte_offenders(rel, text))
    return found, tokens


def apply_allowlist(
    sites: list[tuple[str, int, str]],
) -> tuple[list[tuple[str, int, str]], list[tuple[str, str, str]]]:
    used: set[int] = set()
    offenders: list[tuple[str, int, str]] = []
    for rel, lineno, line in sites:
        hit = None
        for index, (allow_path, needle, _why) in enumerate(ALLOWLIST):
            if rel == allow_path and needle in line:
                hit = index
                break
        if hit is None:
            offenders.append((rel, lineno, line))
        else:
            used.add(hit)
    stale = [entry for index, entry in enumerate(ALLOWLIST) if index not in used]
    return offenders, stale


def report(root: Path, quiet: bool = False) -> int:
    sites, tokens = scan(root)
    if tokens < MIN_TOKENS:
        print(
            f"gc_pin_sites: found only {tokens} {TOKEN} tokens, expected at least "
            f"{MIN_TOKENS}. The scan is broken — a green run here would be vacuous.",
            file=sys.stderr,
        )
        return 2
    offenders, stale = apply_allowlist(sites)
    status = 0
    if offenders:
        status = 1
        print(
            "GC_FLAG_PINNED custody violation: these sites originate a pin without\n"
            "going through gc::pin_object, so they pin an object WITHOUT arming the\n"
            "young-pin latch. The copying minor skips its pin preflight on that latch\n"
            "(#7645) and will relocate the object out from under whoever holds it.\n"
            "Route the site through `gc::pin_object(header)` — or, across an FFI\n"
            "boundary, `js_gc_pin_user_ptr(user_ptr)`.\n",
            file=sys.stderr,
        )
        for rel, lineno, line in offenders:
            print(f"  {rel}:{lineno}: {line}", file=sys.stderr)
    if stale:
        status = 1
        print(
            "\ngc_pin_sites: these ALLOWLIST entries no longer match any pin site.\n"
            "Delete them — a stale exemption is how this gate stops being one.\n",
            file=sys.stderr,
        )
        for allow_path, needle, why in stale:
            print(f"  {allow_path} | {needle} | {why}", file=sys.stderr)
    if status == 0 and not quiet:
        print(
            f"gc_pin_sites: OK — every pin originates in gc::pin_object "
            f"({len(sites)} allowlisted exception(s), {tokens} {TOKEN} tokens scanned)."
        )
    return status


OFFENDER_PLANTS = {
    "named or-into-flags": "    (*h).gc_flags |= crate::gc::GC_FLAG_PINNED;\n",
    "named assign-into-flags": "    (*h).gc_flags = GC_FLAG_MARKED | GC_FLAG_PINNED;\n",
    "named passed as a seed argument": "    let x = alloc_with(64, GC_FLAG_PINNED);\n",
    "raw-byte hex": "    *gc_flags_ptr |= 0x04;\n",
    "raw-byte combined hex": "    (*h).gc_flags = 0x06;\n",
    "raw-byte decimal": "    (*h).gc_flags |= 4;\n",
}

BENIGN_PLANT = """unsafe fn f(h: *mut u8) {
    use crate::gc::GC_FLAG_PINNED;
    if (*h).gc_flags & (GC_FLAG_MARKED | GC_FLAG_PINNED) != 0 { return; }
    let preserved = flags & (GC_FLAG_SHAPE_SHARED | GC_FLAG_INTERNED | GC_FLAG_PINNED);
    (*h).gc_flags &= !GC_FLAG_PINNED;
    (*h).gc_flags = flags & !GC_FLAG_PINNED;
    (*h).gc_flags |= GC_FLAG_MARKED;
    (*h).gc_flags |= 0x01;
    let gc_flags_addr = blk.sub(I64, &handle, "7");
    let gc_flags = blk.load(I8, &gc_flags_ptr);
}
"""


def _scan_source(body: str) -> list[tuple[str, int, str]]:
    with tempfile.TemporaryDirectory() as tmp:
        fake = Path(tmp) / "crates" / "fake" / "src"
        fake.mkdir(parents=True)
        (fake / "lib.rs").write_text(body)
        sites, _tokens = scan(Path(tmp))
        offenders, _stale = apply_allowlist(sites)
        return offenders


def self_test() -> int:
    failures: list[str] = []
    for name, plant in OFFENDER_PLANTS.items():
        if not _scan_source("unsafe fn f(h: *mut u8) {\n" + plant + "}\n"):
            failures.append(f"scanner MISSED the {name} offender: {plant.strip()}")
    benign = _scan_source(BENIGN_PLANT)
    if benign:
        failures.append(f"scanner FALSE-POSITIVED on read/clear/preserve shapes: {benign}")
    missing = [
        entry
        for entry in ALLOWLIST
        if not (REPO_ROOT / entry[0]).exists()
        or entry[1] not in (REPO_ROOT / entry[0]).read_text(encoding="utf-8", errors="replace")
    ]
    if missing:
        failures.append(f"ALLOWLIST entries whose file/needle does not exist: {missing}")
    if failures:
        for line in failures:
            print(f"gc_pin_sites --self-test FAILED: {line}", file=sys.stderr)
        return 1
    print(
        f"gc_pin_sites --self-test: OK ({len(OFFENDER_PLANTS)} offender shapes caught, "
        "no false positives on read/clear/preserve)"
    )
    return 0


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--self-test",
        action="store_true",
        help="prove the scanner can fail before trusting a green run",
    )
    parser.add_argument("--quiet", action="store_true")
    parser.add_argument("--root", default=str(REPO_ROOT))
    args = parser.parse_args()
    if args.self_test:
        return self_test()
    return report(Path(args.root), quiet=args.quiet)


if __name__ == "__main__":
    os.chdir(REPO_ROOT)
    sys.exit(main())
