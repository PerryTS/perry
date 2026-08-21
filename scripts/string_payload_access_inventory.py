#!/usr/bin/env python3
"""Ratchet open-coded access to GC-managed StringHeader payloads.

Two source shapes are counted per crate:

* ``inline-offset``: pointer arithmetic that adds
  ``size_of::<...StringHeader>()`` instead of using the runtime API.
* ``reader-helper``: a Rust function containing both ``StringHeader`` and
  ``from_utf8_unchecked``, the characteristic copy-pasted reader shape.

The committed baseline is debt, not an allowance for new code. A category may
never increase in a crate, and a decrease must lower the baseline in the same
change. New crates implicitly start at zero.

Usage:
    python3 scripts/string_payload_access_inventory.py
    python3 scripts/string_payload_access_inventory.py --self-test
    python3 scripts/string_payload_access_inventory.py --write-baseline
    python3 scripts/string_payload_access_inventory.py --list
"""

from __future__ import annotations

import argparse
import re
import sys
import tempfile
from collections import Counter
from dataclasses import dataclass
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[1]
DEFAULT_BASELINE = REPO_ROOT / "scripts" / "string_payload_access_baseline.txt"
RULES = ("inline-offset", "reader-helper")

INLINE_OFFSET_RE = re.compile(
    r"(?:\.(?:add|wrapping_add|byte_add|wrapping_byte_add"
    r"|offset|wrapping_offset|byte_offset|wrapping_byte_offset)"
    r"\s*\(\s*|\+\s*)"
    r"(?:(?:std|core)::)?(?:mem::)?size_of\s*::\s*<\s*"
    r"(?:[A-Za-z_][A-Za-z0-9_]*::)*StringHeader\s*>\s*\(\s*\)"
    r"(?:\s+as\s+(?:usize|isize))?",
    re.MULTILINE,
)
FUNCTION_RE = re.compile(r"\bfn\s+([A-Za-z_][A-Za-z0-9_]*)[^;{]*\{", re.MULTILINE)
CHAR_LITERAL_RE = re.compile(
    r"(?:b)?'(?:\\(?:.|u\{[0-9A-Fa-f_]+\}|x[0-9A-Fa-f]{2})|[^'\\\n])'"
)


@dataclass(frozen=True)
class Finding:
    crate: str
    rel_path: str
    line_no: int
    rule: str
    detail: str

    def render(self) -> str:
        return f"{self.rel_path}:{self.line_no}: [{self.rule}] {self.detail}"


def mask_non_code(text: str) -> str:
    """Blank Rust comments and strings while preserving offsets/newlines."""

    chars = list(text)
    out = list(text)
    i = 0
    block_depth = 0
    state = "code"
    raw_hashes = 0
    while i < len(chars):
        if state == "line":
            if chars[i] == "\n":
                state = "code"
            else:
                out[i] = " "
            i += 1
            continue
        if state == "block":
            if text.startswith("/*", i):
                out[i : i + 2] = "  "
                block_depth += 1
                i += 2
            elif text.startswith("*/", i):
                out[i : i + 2] = "  "
                block_depth -= 1
                i += 2
                if block_depth == 0:
                    state = "code"
            else:
                if chars[i] != "\n":
                    out[i] = " "
                i += 1
            continue
        if state == "string":
            if chars[i] == "\\":
                out[i] = " "
                if i + 1 < len(chars):
                    if chars[i + 1] != "\n":
                        out[i + 1] = " "
                    i += 2
                else:
                    i += 1
            elif chars[i] == '"':
                out[i] = " "
                state = "code"
                i += 1
            else:
                if chars[i] != "\n":
                    out[i] = " "
                i += 1
            continue
        if state == "raw":
            terminator = '"' + ("#" * raw_hashes)
            if text.startswith(terminator, i):
                out[i : i + len(terminator)] = " " * len(terminator)
                i += len(terminator)
                state = "code"
            else:
                if chars[i] != "\n":
                    out[i] = " "
                i += 1
            continue

        if text.startswith("//", i):
            out[i : i + 2] = "  "
            i += 2
            state = "line"
        elif text.startswith("/*", i):
            out[i : i + 2] = "  "
            i += 2
            block_depth = 1
            state = "block"
        elif chars[i] in ("'", "b") and (char := CHAR_LITERAL_RE.match(text, i)):
            width = char.end() - i
            out[i : i + width] = " " * width
            i += width
        elif chars[i] == '"':
            out[i] = " "
            i += 1
            state = "string"
        elif chars[i] in ("r", "b"):
            raw = re.match(r"(?:br|r)(#{0,255})\"", text[i:])
            if raw:
                width = raw.end()
                raw_hashes = len(raw.group(1))
                out[i : i + width] = " " * width
                i += width
                state = "raw"
            else:
                i += 1
        else:
            i += 1
    return "".join(out)


def matching_brace(text: str, opening: int) -> int | None:
    depth = 0
    for idx in range(opening, len(text)):
        if text[idx] == "{":
            depth += 1
        elif text[idx] == "}":
            depth -= 1
            if depth == 0:
                return idx
    return None


def scan_text(crate: str, rel_path: str, text: str) -> list[Finding]:
    code = mask_non_code(text)
    findings: list[Finding] = []
    for match in INLINE_OFFSET_RE.finditer(code):
        findings.append(
            Finding(
                crate,
                rel_path,
                code.count("\n", 0, match.start()) + 1,
                "inline-offset",
                "open-coded StringHeader payload offset",
            )
        )

    cursor = 0
    while match := FUNCTION_RE.search(code, cursor):
        opening = match.end() - 1
        closing = matching_brace(code, opening)
        if closing is None:
            break
        body = code[match.start() : closing + 1]
        if "StringHeader" in body and "from_utf8_unchecked" in body:
            findings.append(
                Finding(
                    crate,
                    rel_path,
                    code.count("\n", 0, match.start()) + 1,
                    "reader-helper",
                    f"fn {match.group(1)}",
                )
            )
        cursor = closing + 1
    return findings


def crate_dirs(root: Path = REPO_ROOT) -> list[Path]:
    crates = root / "crates"
    return sorted(path for path in crates.iterdir() if (path / "Cargo.toml").is_file())


def collect_inventory(root: Path = REPO_ROOT) -> tuple[list[Finding], int]:
    findings: list[Finding] = []
    files_scanned = 0
    for crate_dir in crate_dirs(root):
        crate = crate_dir.name
        for path in sorted(crate_dir.rglob("*.rs")):
            rel_path = path.relative_to(root).as_posix()
            if any(part.startswith(".") or part == "target" for part in path.parts):
                continue
            files_scanned += 1
            text = path.read_text(encoding="utf-8")
            # Most workspace Rust never mentions the ABI. Avoid running the
            # character-level comment/string masker over tens of megabytes of
            # unrelated compiler and test sources on every lint invocation.
            if "StringHeader" not in text or not (
                "size_of" in text or "from_utf8_unchecked" in text
            ):
                continue
            findings.extend(scan_text(crate, rel_path, text))
    return findings, files_scanned


def counts_for(findings: list[Finding]) -> Counter[tuple[str, str]]:
    return Counter((finding.rule, finding.crate) for finding in findings)


def load_baseline(path: Path) -> dict[tuple[str, str], int]:
    baseline: dict[tuple[str, str], int] = {}
    if not path.is_file():
        return baseline
    errors: list[str] = []
    for line_no, raw in enumerate(path.read_text(encoding="utf-8").splitlines(), 1):
        line = raw.strip()
        if not line or line.startswith("#"):
            continue
        parts = [part.strip() for part in line.split("|", 2)]
        if len(parts) != 3 or parts[0] not in RULES or not parts[2].isdigit():
            errors.append(
                f"{path.name}:{line_no}: expected 'rule | crate | count', got: {raw}"
            )
            continue
        key = (parts[0], parts[1])
        if key in baseline:
            errors.append(f"{path.name}:{line_no}: duplicate entry for {key}")
            continue
        baseline[key] = int(parts[2])
    if errors:
        print("\n".join(errors), file=sys.stderr)
        raise SystemExit(2)
    return baseline


def compare_counts(
    actual: Counter[tuple[str, str]], baseline: dict[tuple[str, str], int]
) -> tuple[list[tuple[str, str, int, int]], list[tuple[str, str, int, int]]]:
    regressions = []
    stale = []
    for rule, crate in sorted(set(actual) | set(baseline)):
        found = actual[(rule, crate)]
        allowed = baseline.get((rule, crate), 0)
        if found > allowed:
            regressions.append((rule, crate, allowed, found))
        elif found < allowed:
            stale.append((rule, crate, allowed, found))
    return regressions, stale


def write_baseline(path: Path, actual: Counter[tuple[str, str]]) -> None:
    lines = [
        "# Open-coded StringHeader payload-access baseline (#8429).",
        "# Format: rule | crate | count",
        "# Regenerate: python3 scripts/string_payload_access_inventory.py --write-baseline",
        "# Counts may only decrease; a new crate starts at zero.",
        "",
    ]
    for rule, crate in sorted(actual):
        if actual[(rule, crate)]:
            lines.append(f"{rule} | {crate} | {actual[(rule, crate)]}")
    path.write_text("\n".join(lines) + "\n", encoding="utf-8")


def run_self_tests() -> int:
    failures: list[str] = []

    def expect(condition: bool, message: str) -> None:
        if not condition:
            failures.append(message)

    planted = r'''
unsafe fn copied_reader(ptr: *const perry_runtime::StringHeader) -> &'static str {
    let data = (ptr as *const u8)
        .add(std::mem::size_of::<perry_runtime::StringHeader>());
    std::str::from_utf8_unchecked(std::slice::from_raw_parts(
        data,
        (*ptr).byte_len as usize,
    ))
}
'''
    findings = scan_text("synthetic-crate", "crates/synthetic-crate/src/lib.rs", planted)
    expect(
        sum(f.rule == "inline-offset" for f in findings) == 1,
        "synthetic inline payload offset was not detected exactly once",
    )
    expect(
        sum(f.rule == "reader-helper" for f in findings) == 1,
        "synthetic copy-pasted reader helper was not detected exactly once",
    )

    alternate_offsets = r'''
unsafe fn alternate_offsets(ptr: *const u8) {
    let _ = ptr.byte_add(core::mem::size_of::<StringHeader>());
    let _ = ptr.wrapping_byte_add(size_of::<crate::StringHeader>());
    let _ = ptr.offset(std::mem::size_of::<perry_runtime::StringHeader>() as isize);
    let _ = ptr.wrapping_offset(size_of::<StringHeader>() as isize);
    let _ = ptr.byte_offset(size_of::<StringHeader>() as isize);
    let _ = ptr.wrapping_byte_offset(size_of::<StringHeader>() as isize);
    let _ = ptr.add(mem::size_of::<StringHeader>());
}
'''
    alternate_findings = scan_text(
        "synthetic-crate", "crates/synthetic-crate/src/alternate.rs", alternate_offsets
    )
    expect(
        sum(f.rule == "inline-offset" for f in alternate_findings) == 7,
        "alternate raw-pointer payload offsets were not all detected",
    )

    clean = r'''
fn sanctioned(ptr: *const perry_runtime::StringHeader) -> Vec<u8> {
    unsafe { perry_runtime::string::OwnedStringBytes::copy_from_header(ptr) }
        .as_bytes()
        .to_vec()
}
// .add(std::mem::size_of::<StringHeader>()) is documentation, not code.
const EXAMPLE: &str = ".add(std::mem::size_of::<StringHeader>())";
'''
    expect(
        not scan_text("synthetic-crate", "crates/synthetic-crate/src/lib.rs", clean),
        "sanctioned access, comments, or strings produced a finding",
    )

    # Exercise crate discovery and the ratchet end to end. This prevents the
    # scanner's regex unit tests from staying green if workspace traversal or
    # per-crate attribution is accidentally broken.
    with tempfile.TemporaryDirectory() as temp_dir:
        temp_root = Path(temp_dir)
        crate_dir = temp_root / "crates" / "synthetic-crate"
        source = crate_dir / "src" / "lib.rs"
        source.parent.mkdir(parents=True)
        (crate_dir / "Cargo.toml").write_text(
            '[package]\nname = "synthetic-crate"\nversion = "0.0.0"\n',
            encoding="utf-8",
        )
        source.write_text(planted, encoding="utf-8")
        discovered, files_scanned = collect_inventory(temp_root)
        expect(files_scanned == 1, "synthetic crate source was not scanned exactly once")
        expect(
            counts_for(discovered) == counts_for(findings),
            "filesystem inventory disagreed with direct source scanning",
        )

        planted_baseline = dict(counts_for(discovered))
        source.write_text(clean, encoding="utf-8")
        removed, _ = collect_inventory(temp_root)
        regressions, stale = compare_counts(counts_for(removed), planted_baseline)
        expect(
            not regressions and bool(stale),
            "removing a planted offender did not make its baseline fail stale",
        )

    actual = counts_for(findings)
    regressions, stale = compare_counts(actual, {})
    expect(bool(regressions) and not stale, "zero baseline did not reject offenders")
    regressions, stale = compare_counts(actual, dict(actual))
    expect(not regressions and not stale, "matching baseline was not accepted")
    lowered = dict(actual)
    lowered[("inline-offset", "synthetic-crate")] += 1
    regressions, stale = compare_counts(actual, lowered)
    expect(not regressions and bool(stale), "a removed offender did not require a repin")

    if failures:
        for failure in failures:
            print(f"self-test failure: {failure}", file=sys.stderr)
        return 1
    print("string-payload access inventory self-tests passed")
    return 0


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--self-test", action="store_true")
    parser.add_argument("--list", action="store_true", help="print every finding")
    parser.add_argument("--write-baseline", action="store_true")
    parser.add_argument("--baseline", type=Path, default=DEFAULT_BASELINE)
    args = parser.parse_args(argv)

    if args.self_test:
        return run_self_tests()

    findings, files_scanned = collect_inventory()
    actual = counts_for(findings)
    if args.write_baseline:
        write_baseline(args.baseline, actual)
        print(f"wrote {args.baseline.relative_to(REPO_ROOT)}")
        return 0

    if args.list:
        for finding in findings:
            print(finding.render())

    baseline = load_baseline(args.baseline)
    regressions, stale = compare_counts(actual, baseline)
    if regressions:
        print("String payload-access ratchet increased:", file=sys.stderr)
        for rule, crate, allowed, found in regressions:
            print(
                f"  {rule} | {crate}: baseline {allowed}, found {found}",
                file=sys.stderr,
            )
            for finding in findings:
                if finding.rule == rule and finding.crate == crate:
                    print(f"    {finding.render()}", file=sys.stderr)
    if stale:
        print("String payload-access baseline is stale; record the progress:", file=sys.stderr)
        for rule, crate, allowed, found in stale:
            print(
                f"  {rule} | {crate}: baseline {allowed}, found {found}",
                file=sys.stderr,
            )
    if regressions or stale:
        print(
            "Run: python3 scripts/string_payload_access_inventory.py --write-baseline",
            file=sys.stderr,
        )
        return 1

    totals = Counter()
    for (rule, _crate), count in actual.items():
        totals[rule] += count
    print(
        f"string-payload access inventory: {files_scanned} files; "
        f"{totals['inline-offset']} inline offsets and "
        f"{totals['reader-helper']} reader helpers held by the ratchet"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
