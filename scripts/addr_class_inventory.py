#!/usr/bin/env python3
"""Audit handle-vs-heap-pointer address classification sites.

Perry NaN-boxes JS values; POINTER_TAG payloads are USUALLY heap pointers but
several subsystems smuggle small integer registry handles under the same tag
(see crates/perry-runtime/src/value/addr_class.rs for the band map).  Runtime
code must classify a payload by magnitude through the predicates in
`value::addr_class` BEFORE dereferencing it — hand-re-typed band literals and
unvalidated `as *const GcHeader` casts are the root of a recurring
Linux-only segfault class (#1843, #4004, #4665, #4800).

Two rule classes:

1. BAND LITERAL — a handle-band boundary literal (0x100000, 0xF0000, 0x40000,
   0xE0000, 0x200000, underscore-separated variants) appearing in code in
   perry-runtime/perry-stdlib outside `value/addr_class.rs`.  New sites must
   call the named `addr_class` predicates/constants instead.

2. GCHEADER CAST — `as *const/mut GcHeader` outside `gc/` (collector
   internals) and `value/addr_class.rs` (the checked `try_read_gc_header`
   owner).  Pre-existing probe sites are grandfathered through the allowlist
   with a justification; new sites should route through
   `addr_class::try_read_gc_header` or carry an allowlist entry explaining
   what validates the address before the dereference.

Allowlist: scripts/addr_class_allowlist.txt, same
`path-prefix | line-substring-or-* | justification` format as
scripts/gc_store_site_allowlist.txt.  Malformed lines fail the run (exit 2).
"""

from __future__ import annotations

import argparse
import re
import sys
from dataclasses import dataclass, field
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[1]
DEFAULT_ALLOWLIST = REPO_ROOT / "scripts" / "addr_class_allowlist.txt"

SCAN_ROOTS = (
    "crates/perry-runtime/src",
    "crates/perry-stdlib/src",
)

# The module that owns the band constants/predicates, and the collector
# internals that legitimately manipulate GcHeader layout directly.
EXCLUDED_PREFIXES = (
    "crates/perry-runtime/src/value/addr_class.rs",
    "crates/perry-runtime/src/gc/",
)

# Word-bounded band-boundary literals (plus Rust underscore-separator
# variants).  0x100000001b3 (FNV prime), 0x400000 (O_DSYNC), 0x100000000
# (.text floor) etc. do NOT match because the literal continues with more
# word characters.
BAND_LITERAL_RE = re.compile(
    r"0x(?:10_?0000|F_?0000|4_?0000|E_?0000|20_?0000)\b",
    re.IGNORECASE,
)

GC_HEADER_CAST_RE = re.compile(r"as\s+\*(?:const|mut)\s+(?:crate::gc::)?GcHeader\b")

# HANDLE FLOOR (#6279) — a hand-rolled address floor that is BELOW
# `HANDLE_BAND_MAX` (0x100000).  `0x1000` / `0x10000` are an order of magnitude
# too low, so the fetch (0x40000..0xE0000), zlib (0xE0000..0xF0000) and proxy
# (0xF0000..) handle bands sail straight through into a dereference.  This is
# the *wrong-literal* case that BAND_LITERAL_RE structurally cannot see: that
# rule only knows the CORRECT boundaries, so an invented floor is invisible to
# it.  Two real crashes had exactly this shape (js_object_freeze's `> 0x10000`
# and js_object_delete_field's `< 0x10000`, both fixed in #6280).
#
# Only flag the literal when it is compared against something address-shaped —
# `0x10000` is also a perfectly ordinary 64 KB buffer size, and those are not
# our problem.
HANDLE_FLOOR_RE = re.compile(
    r"(?:\bptr\b|\baddr\b|\bbits\b|_ptr\b|_addr\b|_bits\b|as\s+usize|as\s+u64)"
    r"[^;]{0,60}?(?:<|>|<=|>=)\s*(?:crate::gc::GC_HEADER_SIZE\s*\+\s*)?0x1_?0?000\b"
    r"|0x1_?0?000\b\s*(?:<|<=)\s*[A-Za-z_]*(?:ptr|addr|bits)\b",
    re.IGNORECASE,
)

# HANDLE FLOOR, RANGE FORM (#6320) — the same too-low floor written as a Rust
# range test instead of a comparison:
#
#     if !(0x1000..0x0001_0000_0000_0000).contains(&addr) { return false; }
#
# HANDLE_FLOOR_RE structurally cannot see this: the literal is followed by `..`,
# never by a comparison operator, and the address operand appears AFTER it
# inside `.contains(&…)`.  That blind spot is exactly why the closure-validation
# probes in `symbol/iterator.rs`, `symbol/properties.rs` and `jsx.rs` survived
# the #6279 sweep and kept SIGSEGV-ing on a Proxy id (#5976 / #6320).
#
# The established remediation (#6321) keeps the range and adds a band predicate
# next to it, so a band predicate in the surrounding lines clears the finding —
# same pairing contract as `lone-valid-obj-ptr` below.  Reported under the
# `handle-floor` rule so it shares that rule's per-file ratchet.
HANDLE_FLOOR_RANGE_RE = re.compile(r"\(\s*0x1_?0?000\s*\.\.")

# LONE is_valid_obj_ptr (#6279) — `is_valid_obj_ptr` used as the ONLY guard
# before a dereference.  Its own doc says it is not sufficient:
#
#   the platform HEAP_MIN floor on Linux/Android/iOS/Windows (0x1000) is BELOW
#   the handle band, so this predicate alone does NOT reject small handles
#   there — pair it with is_handle_band
#
# That is exactly the shape of #6271 (`gz.on("data")` deref'ing a zlib stream
# handle): no band literal and no GcHeader cast, so neither of the original two
# rules could see it.  A band predicate anywhere in the surrounding guard clears
# the finding.
VALID_OBJ_PTR_RE = re.compile(r"\bis_valid_obj_ptr\s*\(")
BAND_PREDICATE_RE = re.compile(
    r"is_above_handle_band|is_handle_band|is_small_handle|is_proxy_id_band"
    r"|try_read_gc_header"
)
# How many lines above the call may satisfy the pairing requirement.
BAND_PREDICATE_LOOKBACK = 5
# ...and how many CODE lines below (blank lines and comments don't count: the
# #6321 shape puts the band guard right after the coarse range pre-filter, but
# behind a long justification comment).
BAND_PREDICATE_LOOKAHEAD_CODE = 6

DEFAULT_RATCHET_BASELINE = REPO_ROOT / "scripts" / "addr_class_ratchet_baseline.txt"

# Rules governed by the count ratchet rather than the line-substring allowlist.
RATCHETED_RULES = ("handle-floor", "lone-valid-obj-ptr")

LINE_COMMENT_RE = re.compile(r"//.*$")


@dataclass
class Finding:
    rel_path: str
    line_no: int
    rule: str
    line: str

    def render(self) -> str:
        return f"{self.rel_path}:{self.line_no}: [{self.rule}] {self.line.strip()}"


@dataclass
class AllowlistEntry:
    path_prefix: str
    substring: str
    justification: str
    line_no: int
    hits: int = field(default=0)

    def matches(self, finding: Finding) -> bool:
        if not finding.rel_path.startswith(self.path_prefix):
            return False
        return self.substring == "*" or self.substring in finding.line


def strip_comment(line: str) -> str:
    # Good enough for this audit: drop everything after `//`.  Band literals
    # inside string literals are not a thing in these crates, and doc-comment
    # mentions of historical values are fine.
    return LINE_COMMENT_RE.sub("", line)


def band_predicate_near(lines: list[str], idx: int) -> bool:
    """True when a band predicate guards the statement at `lines[idx]`.

    Looks back a few raw lines and forward over the next few CODE lines
    (comments/blanks skipped) — the guard is a statement, not a neighbour.
    """

    start = max(0, idx - BAND_PREDICATE_LOOKBACK)
    context = [strip_comment(line) for line in lines[start : idx + 1]]
    taken = 0
    cursor = idx + 1
    while cursor < len(lines) and taken < BAND_PREDICATE_LOOKAHEAD_CODE:
        code = strip_comment(lines[cursor])
        if code.strip():
            context.append(code)
            taken += 1
        cursor += 1
    return bool(BAND_PREDICATE_RE.search("\n".join(context)))


def scan_text(rel_path: str, text: str) -> list[Finding]:
    findings: list[Finding] = []
    if any(rel_path.startswith(prefix) for prefix in EXCLUDED_PREFIXES):
        return findings
    lines = text.splitlines()
    for idx, raw in enumerate(lines):
        line_no = idx + 1
        code = strip_comment(raw)
        if BAND_LITERAL_RE.search(code):
            findings.append(Finding(rel_path, line_no, "band-literal", raw))
        if GC_HEADER_CAST_RE.search(code):
            findings.append(Finding(rel_path, line_no, "gcheader-cast", raw))
        if HANDLE_FLOOR_RE.search(code):
            findings.append(Finding(rel_path, line_no, "handle-floor", raw))
        elif HANDLE_FLOOR_RANGE_RE.search(code) and not band_predicate_near(lines, idx):
            # A band predicate in the enclosing guard clears it — the range test
            # is then just a coarse pre-filter, not the real gate (#6321).
            findings.append(Finding(rel_path, line_no, "handle-floor", raw))
        if VALID_OBJ_PTR_RE.search(code) and "fn is_valid_obj_ptr" not in code:
            # A band predicate anywhere in the enclosing guard clears it.
            start = max(0, idx - BAND_PREDICATE_LOOKBACK)
            context = "\n".join(
                strip_comment(l) for l in lines[start : idx + 2]
            )
            if not BAND_PREDICATE_RE.search(context):
                findings.append(
                    Finding(rel_path, line_no, "lone-valid-obj-ptr", raw)
                )
    return findings


def collect_inventory() -> tuple[list[Finding], int]:
    findings: list[Finding] = []
    files_scanned = 0
    for root in SCAN_ROOTS:
        for path in sorted((REPO_ROOT / root).rglob("*.rs")):
            rel_path = path.relative_to(REPO_ROOT).as_posix()
            # Skip parked/hidden trees (e.g. `.value.parked/`) — not compiled.
            if any(part.startswith(".") for part in rel_path.split("/")):
                continue
            files_scanned += 1
            findings.extend(scan_text(rel_path, path.read_text(encoding="utf-8")))
    return findings, files_scanned


def load_allowlist(path: Path) -> list[AllowlistEntry]:
    """Parse `path-prefix | line-substring-or-* | justification` lines.

    Every entry MUST carry a non-empty justification; a malformed line is a
    hard error so the allowlist can't silently rot.
    """

    if not path.is_file():
        return []
    entries: list[AllowlistEntry] = []
    errors: list[str] = []
    for line_no, raw in enumerate(path.read_text(encoding="utf-8").splitlines(), 1):
        line = raw.strip()
        if not line or line.startswith("#"):
            continue
        parts = [part.strip() for part in line.split("|", 2)]
        if len(parts) != 3 or not parts[0] or not parts[1] or not parts[2]:
            errors.append(
                f"{path.name}:{line_no}: expected "
                "'path-prefix | line-substring-or-* | justification', got: " + raw
            )
            continue
        entries.append(AllowlistEntry(parts[0], parts[1], parts[2], line_no))
    if errors:
        for error in errors:
            print(error, file=sys.stderr)
        raise SystemExit(2)
    return entries


def apply_allowlist(
    findings: list[Finding], entries: list[AllowlistEntry]
) -> tuple[list[Finding], int]:
    kept: list[Finding] = []
    suppressed = 0
    for finding in findings:
        entry = next((e for e in entries if e.matches(finding)), None)
        if entry is None:
            kept.append(finding)
        else:
            entry.hits += 1
            suppressed += 1
    return kept, suppressed


def run_self_tests() -> int:
    failures: list[str] = []

    def expect(cond: bool, message: str) -> None:
        if not cond:
            failures.append(message)

    runtime = "crates/perry-runtime/src/foo.rs"

    # --- handle-floor rule (#6279) ------------------------------------------
    # An address compared against a floor BELOW HANDLE_BAND_MAX is the bug.
    for src in (
        "if (obj as usize) < 0x10000 {\n",
        "if ptr.is_null() || (ptr as usize) < 0x1000 {\n",
        "if raw_addr >= crate::gc::GC_HEADER_SIZE + 0x1000 {\n",
        "} else if top16 == 0 && bits >= 0x1000 {\n",
    ):
        expect(
            any(f.rule == "handle-floor" for f in scan_text(runtime, src)),
            f"handle-floor should flag: {src.strip()}",
        )

    # 0x10000 as a plain SIZE (a 64 KB buffer, a chunk cap) is not an address
    # guard and must not be flagged — otherwise the rule is unusable noise.
    for src in (
        "const CHUNK: usize = 0x10000;\n",
        "let mut buf = vec![0u8; 0x10000];\n",
        "if len > 0x10000 {\n",
    ):
        expect(
            not any(f.rule == "handle-floor" for f in scan_text(runtime, src)),
            f"handle-floor must NOT flag a size literal: {src.strip()}",
        )

    # The CORRECT boundary is not a handle-floor finding (band-literal owns it).
    expect(
        not any(
            f.rule == "handle-floor"
            for f in scan_text(runtime, "if (ptr as usize) < 0x100000 {\n")
        ),
        "handle-floor must not fire on the correct HANDLE_BAND_MAX boundary",
    )

    # Comment-only mentions are ignored, same as the other rules.
    expect(
        not any(
            f.rule == "handle-floor"
            for f in scan_text(runtime, "// the old floor was ptr < 0x10000\n")
        ),
        "handle-floor must ignore comments",
    )

    # --- handle-floor, RANGE form (#6320) -----------------------------------
    # The same too-low floor written as a range test. The comparison-operator
    # regex structurally cannot see it, which is how the closure-validation
    # probes survived the #6279 sweep and kept faulting on a Proxy id.
    for src in (
        "    if !(0x1000..0x0001_0000_0000_0000).contains(&addr) {\n        return false;\n    }\n",
        "    } else if (0x10000..=RAW_PTR_MAX).contains(&bits) {\n        deref(bits)\n",
    ):
        expect(
            any(f.rule == "handle-floor" for f in scan_text(runtime, src)),
            f"handle-floor should flag the range form: {src.splitlines()[0].strip()}",
        )
    # Paired with a band predicate on the next statement -> cleared. This is the
    # #6321 fix shape (coarse range pre-filter, real gate right below, behind a
    # long justification comment), so the rule must accept it.
    paired_range = (
        "    if !(0x1000..0x0001_0000_0000_0000).contains(&addr) {\n"
        "        return std::ptr::null();\n"
        "    }\n"
        "    // #5976: reject the small-handle band BEFORE the magic probe.\n"
        "    // Revocable-proxy ids and stdlib registry ids are NaN-boxed\n"
        "    // POINTER_TAG values, not heap pointers.\n"
        "\n"
        "    if crate::value::addr_class::is_handle_band(addr as usize) {\n"
        "        return std::ptr::null();\n"
        "    }\n"
    )
    expect(
        not any(f.rule == "handle-floor" for f in scan_text(runtime, paired_range)),
        "handle-floor must accept a range pre-filter paired with a band predicate",
    )
    # A range whose floor is already the CORRECT boundary is not a finding.
    expect(
        not any(
            f.rule == "handle-floor"
            for f in scan_text(runtime, "if (0x100000..MAX).contains(&addr) {\n")
        ),
        "handle-floor must not fire on a range starting at HANDLE_BAND_MAX",
    )

    # --- lone-valid-obj-ptr rule (#6279) ------------------------------------
    lone = "    if is_valid_obj_ptr(ptr as *const u8) {\n        (*ptr).class_id\n"
    expect(
        any(f.rule == "lone-valid-obj-ptr" for f in scan_text(runtime, lone)),
        "lone-valid-obj-ptr should flag an unpaired is_valid_obj_ptr guard",
    )
    # Paired with a band predicate -> cleared. This is the fix shape, so the rule
    # must accept it or it would just block people from fixing the bug.
    paired = (
        "    if crate::value::addr_class::is_above_handle_band(ptr as usize)\n"
        "        && is_valid_obj_ptr(ptr as *const u8)\n    {\n"
    )
    expect(
        not any(f.rule == "lone-valid-obj-ptr" for f in scan_text(runtime, paired)),
        "lone-valid-obj-ptr must accept a guard paired with a band predicate",
    )
    # try_read_gc_header does both checks itself.
    trg = "    if let Some(h) = try_read_gc_header(ptr) {\n        let _ = is_valid_obj_ptr(ptr);\n"
    expect(
        not any(f.rule == "lone-valid-obj-ptr" for f in scan_text(runtime, trg)),
        "lone-valid-obj-ptr must accept try_read_gc_header",
    )
    # The definition itself is not a call site.
    expect(
        not any(
            f.rule == "lone-valid-obj-ptr"
            for f in scan_text(runtime, "pub fn is_valid_obj_ptr(ptr: *const u8) -> bool {\n")
        ),
        "lone-valid-obj-ptr must not flag the definition",
    )

    # Band literals in code are caught; comment-only mentions are not.
    hits = scan_text(runtime, "if addr < 0x100000 {\n")
    expect(
        len(hits) == 1 and hits[0].rule == "band-literal",
        "band literal in code should be flagged",
    )
    expect(
        not scan_text(runtime, "// historic floor was 0x100000\n"),
        "band literal in a comment should be ignored",
    )
    expect(
        bool(scan_text(runtime, "if (0xF0000..0x100000).contains(&a) {}\n")),
        "proxy band range should be flagged",
    )
    expect(
        bool(scan_text(runtime, "const X: usize = 0x4_0000;\n")),
        "underscore variant should be flagged",
    )

    # Neighbouring literals that merely contain a band prefix must not match.
    for benign in (
        "h = h.wrapping_mul(0x100000001b3);\n",
        '"O_DSYNC" => Some(0x400000),\n',
        "if !(0x100000000..=0x400000000).contains(&f) {}\n",
        "let mask = 0x0000_FFFF_FFFF_FFFF;\n",
    ):
        expect(not scan_text(runtime, benign), f"benign literal flagged: {benign!r}")

    # GcHeader casts are caught in both path forms.
    expect(
        scan_text(runtime, "let h = (a - 8) as *const crate::gc::GcHeader;\n")[0].rule
        == "gcheader-cast",
        "qualified GcHeader cast should be flagged",
    )
    expect(
        bool(scan_text(runtime, "let h = p.sub(8) as *mut GcHeader;\n")),
        "bare GcHeader cast should be flagged",
    )

    # Owner module and collector internals are exempt.
    expect(
        not scan_text(
            "crates/perry-runtime/src/value/addr_class.rs",
            "pub const HANDLE_BAND_MAX: usize = 0x100000;\n",
        ),
        "addr_class.rs must be exempt",
    )
    expect(
        not scan_text(
            "crates/perry-runtime/src/gc/mod.rs",
            "let h = a as *const GcHeader;\n",
        ),
        "gc/ must be exempt",
    )

    # Allowlist matching: prefix + substring, prefix + wildcard.
    finding = Finding(runtime, 1, "gcheader-cast", "x as *const GcHeader")
    expect(
        AllowlistEntry("crates/perry-runtime/src/foo.rs", "*", "j", 1).matches(finding),
        "wildcard entry should match",
    )
    expect(
        AllowlistEntry("crates/perry-runtime/src/foo.rs", "GcHeader", "j", 1).matches(
            finding
        ),
        "substring entry should match",
    )
    expect(
        not AllowlistEntry("crates/perry-runtime/src/bar.rs", "*", "j", 1).matches(
            finding
        ),
        "other-path entry must not match",
    )

    if failures:
        for failure in failures:
            print(f"self-test failure: {failure}", file=sys.stderr)
        return 1
    print("addr-class inventory self-tests passed.")
    return 0


def load_ratchet_baseline(path: Path) -> dict[tuple[str, str], int]:
    """Parse `rule | path | count` lines: KNOWN pre-existing sites per (rule, file).

    A COUNT baseline, not a line-substring allowlist: these sites move on every
    refactor, so pinning them to line text would rot immediately and give false
    comfort. The contract is a ratchet — a file may never gain a site, and every
    fix lowers its number.
    """

    baseline: dict[tuple[str, str], int] = {}
    if not path.is_file():
        return baseline
    errors: list[str] = []
    for line_no, raw in enumerate(path.read_text(encoding="utf-8").splitlines(), 1):
        line = raw.strip()
        if not line or line.startswith("#"):
            continue
        parts = [part.strip() for part in line.split("|", 2)]
        if len(parts) != 3 or parts[0] not in RATCHETED_RULES or not parts[2].isdigit():
            errors.append(
                f"{path.name}:{line_no}: expected 'rule | path | count' with rule in "
                f"{RATCHETED_RULES}, got: {raw}"
            )
            continue
        baseline[(parts[0], parts[1])] = int(parts[2])
    if errors:
        for error in errors:
            print(error, file=sys.stderr)
        raise SystemExit(2)
    return baseline


def check_ratchet(
    findings: list[Finding], baseline: dict[tuple[str, str], int]
) -> tuple[list[Finding], list[str]]:
    """Split ratcheted findings into regressions (a file gained sites) and debt."""

    per_key: dict[tuple[str, str], list[Finding]] = {}
    for f in findings:
        if f.rule in RATCHETED_RULES:
            per_key.setdefault((f.rule, f.rel_path), []).append(f)

    regressions: list[Finding] = []
    for key, hits in sorted(per_key.items()):
        if len(hits) > baseline.get(key, 0):
            # A count ratchet cannot know WHICH line is new, so surface them all
            # rather than fingering an arbitrary one.
            regressions.extend(hits)

    stale: list[str] = []
    for (rule, rel_path), allowed in sorted(baseline.items()):
        actual = len(per_key.get((rule, rel_path), []))
        if actual < allowed:
            stale.append(
                f"  {rule} | {rel_path}: baseline says {allowed}, found {actual} "
                f"— lower it to {actual}"
            )
    return regressions, stale


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--self-test", action="store_true")
    parser.add_argument("--allowlist", type=Path, default=DEFAULT_ALLOWLIST)
    parser.add_argument("--baseline", type=Path, default=DEFAULT_RATCHET_BASELINE)
    parser.add_argument(
        "--write-baseline",
        action="store_true",
        help="regenerate the ratchet baseline from the current tree",
    )
    parser.add_argument(
        "--list-unused-allowlist",
        action="store_true",
        help="also report allowlist entries that matched nothing",
    )
    args = parser.parse_args(argv)
    if args.self_test:
        return run_self_tests()

    findings, files_scanned = collect_inventory()
    entries = load_allowlist(args.allowlist)

    # The line-substring allowlist governs band-literal / gcheader-cast only. It
    # must NOT be able to suppress a ratcheted finding: several of its entries are
    # broad (`path | * | justification`), so letting them apply here would let a
    # brand-new violation slip into an already-allowlisted file — the very blind
    # spot #6279 is about.
    ratcheted = [f for f in findings if f.rule in RATCHETED_RULES]
    other = [f for f in findings if f.rule not in RATCHETED_RULES]
    other, suppressed = apply_allowlist(other, entries)

    if args.write_baseline:
        counts: dict[tuple[str, str], int] = {}
        for f in ratcheted:
            key = (f.rule, f.rel_path)
            counts[key] = counts.get(key, 0) + 1
        header = [
            "# addr-class ratchet baseline (#6279).",
            "#",
            "# Pre-existing sites for the two rules that CANNOT be fixed in one pass:",
            "#",
            "#   handle-floor        a hand-rolled address floor BELOW HANDLE_BAND_MAX",
            "#                       (0x1000 / 0x10000). Does not reject the fetch, zlib",
            "#                       or proxy handle bands, so it can deref a handle and",
            "#                       segfault on Linux. macOS hides it behind a 2 TB floor.",
            "#",
            "#   lone-valid-obj-ptr  is_valid_obj_ptr used as the ONLY guard before a",
            "#                       deref. Its own doc says that is not sufficient on",
            "#                       Linux/Windows/Android/iOS — pair it with a band",
            "#                       predicate. This is the exact shape of #6271.",
            "#",
            "# Counts, not line matches: these sites move constantly and a line-pinned",
            "# allowlist would rot on the first refactor. The contract is a RATCHET —",
            "# a file may never gain a site (that fails the gate), and fixing one means",
            "# lowering its number here. The gate tells you when a count is stale.",
            "#",
            "# Converting a site to an addr_class predicate is always safety-monotonic:",
            "# it can only reject MORE addresses, never dereference more.",
            "#",
            "# Regenerate: python3 scripts/addr_class_inventory.py --write-baseline",
            "",
        ]
        body = [
            f"{rule} | {path} | {count}"
            for (rule, path), count in sorted(counts.items())
        ]
        args.baseline.write_text("\n".join(header + body) + "\n", encoding="utf-8")
        totals: dict[str, int] = {}
        for (rule, _), count in counts.items():
            totals[rule] = totals.get(rule, 0) + count
        summary = ", ".join(f"{v} {k}" for k, v in sorted(totals.items()))
        print(f"Wrote {args.baseline} ({summary}).")
        return 0

    baseline = load_ratchet_baseline(args.baseline)
    regressions, stale = check_ratchet(ratcheted, baseline)

    if args.list_unused_allowlist:
        for entry in entries:
            if entry.hits == 0:
                print(
                    f"unused allowlist entry ({args.allowlist.name}:{entry.line_no}): "
                    f"{entry.path_prefix} | {entry.substring}"
                )

    failed = False

    if regressions:
        failed = True
        by_key: dict[tuple[str, str], list[Finding]] = {}
        for f in regressions:
            by_key.setdefault((f.rule, f.rel_path), []).append(f)
        print(
            "addr-class RATCHET FAILED — a file gained a site in a rule that is\n"
            "frozen at its current count. Use the predicates in\n"
            "crates/perry-runtime/src/value/addr_class.rs (is_handle_band /\n"
            "is_above_handle_band / try_read_gc_header) instead of a hand-rolled\n"
            "address floor or a bare is_valid_obj_ptr guard: neither rejects the\n"
            "fetch/zlib/proxy handle bands on Linux, and dereferencing a handle\n"
            "segfaults there while macOS silently hides it (#1843, #4004, #4665,\n"
            "#4800, #6271).\n"
        )
        for (rule, rel_path), hits in sorted(by_key.items()):
            allowed = baseline.get((rule, rel_path), 0)
            print(f"  [{rule}] {rel_path}: {len(hits)} site(s), baseline allows {allowed}")
            for f in hits:
                print(f"      line {f.line_no}: {f.line.strip()}")
        print()

    if other:
        failed = True
        print(
            "Address-classification audit failed; use the predicates/constants in\n"
            "crates/perry-runtime/src/value/addr_class.rs (is_handle_band /\n"
            "is_small_handle / is_proxy_id_band / try_read_gc_header / ...) instead\n"
            "of re-typing band literals or casting to GcHeader, or add a justified\n"
            "entry to scripts/addr_class_allowlist.txt:"
        )
        for finding in other:
            print(f"  {finding.render()}")

    if failed:
        return 1

    if stale:
        print("Ratchet baseline is stale (sites were fixed but not recorded):")
        for msg in stale:
            print(msg)
        print("Run: python3 scripts/addr_class_inventory.py --write-baseline\n")

    held = sum(baseline.values())
    print(
        f"Address-classification audit passed "
        f"({files_scanned} files scanned, {suppressed} allowlisted, "
        f"{held} known sites held by the ratchet)."
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
