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


def scan_text(rel_path: str, text: str) -> list[Finding]:
    findings: list[Finding] = []
    if any(rel_path.startswith(prefix) for prefix in EXCLUDED_PREFIXES):
        return findings
    for line_no, raw in enumerate(text.splitlines(), 1):
        code = strip_comment(raw)
        if BAND_LITERAL_RE.search(code):
            findings.append(Finding(rel_path, line_no, "band-literal", raw))
        if GC_HEADER_CAST_RE.search(code):
            findings.append(Finding(rel_path, line_no, "gcheader-cast", raw))
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


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--self-test", action="store_true")
    parser.add_argument("--allowlist", type=Path, default=DEFAULT_ALLOWLIST)
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
    findings, suppressed = apply_allowlist(findings, entries)

    if args.list_unused_allowlist:
        for entry in entries:
            if entry.hits == 0:
                print(
                    f"unused allowlist entry ({args.allowlist.name}:{entry.line_no}): "
                    f"{entry.path_prefix} | {entry.substring}"
                )

    if findings:
        print(
            "Address-classification audit failed; use the predicates/constants in\n"
            "crates/perry-runtime/src/value/addr_class.rs (is_handle_band /\n"
            "is_small_handle / is_proxy_id_band / try_read_gc_header / ...) instead\n"
            "of re-typing band literals or casting to GcHeader, or add a justified\n"
            "entry to scripts/addr_class_allowlist.txt:"
        )
        for finding in findings:
            print(f"  {finding.render()}")
        return 1

    print(
        f"Address-classification audit passed "
        f"({files_scanned} files scanned, {suppressed} allowlisted)."
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
