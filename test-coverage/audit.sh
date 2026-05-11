#!/usr/bin/env bash
# Perry Test Coverage Audit
#
# Scans all pub extern "C" functions in perry-runtime and perry-stdlib,
# cross-references them against TypeScript fixtures and Rust tests, and
# generates a coverage report.
#
# Usage:
#   ./test-coverage/audit.sh              # Print report to stdout
#   ./test-coverage/audit.sh --markdown   # Generate COVERAGE.md

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
MARKDOWN_MODE=0
if [[ "${1:-}" == "--markdown" ]]; then
  MARKDOWN_MODE=1
fi

python3 - "$ROOT" "$SCRIPT_DIR/COVERAGE.md" "$MARKDOWN_MODE" <<'PYEOF'
from __future__ import annotations

import collections
import re
import sys
from datetime import datetime, timezone
from pathlib import Path

root = Path(sys.argv[1])
output_file = Path(sys.argv[2])
markdown_mode = sys.argv[3] == "1"

ffi_re = re.compile(
    r'pub\s+(?:unsafe\s+)?extern\s+"C"\s+fn\s+([A-Za-z_][A-Za-z0-9_]*)\s*\('
)

ffi_functions: list[tuple[str, str, str]] = []
for crate, rel in (
    ("runtime", "crates/perry-runtime/src"),
    ("stdlib", "crates/perry-stdlib/src"),
):
    for path in sorted((root / rel).rglob("*.rs")):
        text = path.read_text(errors="ignore")
        source = str(path.relative_to(root))
        for match in ffi_re.finditer(text):
            ffi_functions.append((crate, match.group(1), source))

print("Scanning FFI functions...", file=sys.stderr)
print(f"Found {len(ffi_functions)} FFI functions", file=sys.stderr)

ts_fixture_text = []
for pattern in ("*.ts", "*.tsx"):
    for path in sorted((root / "test-files").glob(pattern)):
        ts_fixture_text.append(path.read_text(errors="ignore"))
ts_text = "\n".join(ts_fixture_text)

rust_test_texts = []
for path in sorted((root / "crates").rglob("*.rs")):
    text = path.read_text(errors="ignore")
    if "#[test]" in text or "#[tokio::test]" in text:
        rust_test_texts.append(text)

rows = []
for crate, fn_name, source_file in ffi_functions:
    ts_covered = fn_name in ts_text
    rust_covered = any(fn_name in text for text in rust_test_texts)
    rows.append(
        {
            "crate": crate,
            "fn": fn_name,
            "source": source_file,
            "ts": ts_covered,
            "rust": rust_covered,
            "combined": ts_covered or rust_covered,
        }
    )

total = len(rows)
ts_covered = sum(1 for row in rows if row["ts"])
rust_covered = sum(1 for row in rows if row["rust"])
combined_covered = sum(1 for row in rows if row["combined"])
combined_uncovered = total - combined_covered

def pct(count: int) -> str:
    return f"{count * 100 / total:.1f}" if total else "0.0"

file_total = collections.Counter(row["source"] for row in rows)
file_ts = collections.Counter(row["source"] for row in rows if row["ts"])
file_rust = collections.Counter(row["source"] for row in rows if row["rust"])
file_combined = collections.Counter(row["source"] for row in rows if row["combined"])

combined_uncovered_rows = [row for row in rows if not row["combined"]]
ts_uncovered_rows = [row for row in rows if not row["ts"]]

if markdown_mode:
    with output_file.open("w") as out:
        out.write("# Perry FFI Test Coverage\n\n")
        out.write(f"Generated: {datetime.now(timezone.utc).strftime('%Y-%m-%dT%H:%M:%SZ')}\n\n")
        out.write("## Summary\n\n")
        out.write(f"- **Total FFI functions:** {total}\n")
        out.write(f"- **Covered by TypeScript fixtures:** {ts_covered} ({pct(ts_covered)}%)\n")
        out.write(f"- **Covered by Rust tests:** {rust_covered} ({pct(rust_covered)}%)\n")
        out.write(f"- **Covered by either TS or Rust:** {combined_covered} ({pct(combined_covered)}%)\n")
        out.write(f"- **Uncovered by either TS or Rust:** {combined_uncovered}\n\n")
        out.write("## Coverage by File\n\n")
        out.write("| File | Total | TS Covered | Rust Covered | Combined | TS Coverage | Combined Coverage |\n")
        out.write("|------|-------|------------|--------------|----------|-------------|-------------------|\n")
        for source in sorted(file_total):
            total_for_file = file_total[source]
            ts_for_file = file_ts[source]
            rust_for_file = file_rust[source]
            combined_for_file = file_combined[source]
            out.write(
                f"| `{source}` | {total_for_file} | {ts_for_file} | {rust_for_file} | "
                f"{combined_for_file} | {ts_for_file * 100 / total_for_file:.0f}% | "
                f"{combined_for_file * 100 / total_for_file:.0f}% |\n"
            )

        if combined_uncovered_rows:
            out.write("\n## Combined Uncovered Functions\n\n")
            for row in sorted(combined_uncovered_rows, key=lambda item: (item["source"], item["fn"])):
                out.write(f"- `{row['fn']}` ({row['source']})\n")

        if ts_uncovered_rows:
            out.write("\n## TypeScript Uncovered Functions\n\n")
            for row in sorted(ts_uncovered_rows, key=lambda item: (item["source"], item["fn"])):
                out.write(f"- `{row['fn']}` ({row['source']})\n")

    print(f"TS coverage: {ts_covered}/{total} ({pct(ts_covered)}%)", file=sys.stderr)
    print(
        f"Combined coverage: {combined_covered}/{total} ({pct(combined_covered)}%)",
        file=sys.stderr,
    )
    print(f"Report written to: {output_file}", file=sys.stderr)
else:
    print("\n=== Perry FFI Test Coverage ===")
    print(f"Total:              {total}")
    print(f"TS covered:         {ts_covered} ({pct(ts_covered)}%)")
    print(f"Rust covered:       {rust_covered} ({pct(rust_covered)}%)")
    print(f"Combined covered:   {combined_covered} ({pct(combined_covered)}%)")
    print(f"Combined uncovered: {combined_uncovered}\n")
    print("--- Coverage by File ---")
    for source in sorted(file_total):
        total_for_file = file_total[source]
        ts_for_file = file_ts[source]
        combined_for_file = file_combined[source]
        print(
            f"  {source:<58s} "
            f"TS {ts_for_file:3d}/{total_for_file:<3d} "
            f"Combined {combined_for_file:3d}/{total_for_file:<3d}"
        )
    print(f"\n--- Combined Uncovered Functions ({len(combined_uncovered_rows)} total) ---")
    for row in sorted(combined_uncovered_rows, key=lambda item: (item["source"], item["fn"])):
        print(f"  {row['fn']:<45s} {row['source']}")
PYEOF
