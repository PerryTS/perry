#!/usr/bin/env python3
"""Check the typed ledger for the native-table rows formerly named NR_PTR.

The Rust table is the executable declaration.  The TSV is the provider-side
inventory: it pins each runtime symbol to the result class and to the source
file whose implementation was read.  Keeping both sides checked prevents a
new row from silently inheriting the old, storage-free `NR_PTR` contract.
"""

from __future__ import annotations

import argparse
import re
import tempfile
from dataclasses import dataclass
from pathlib import Path


REPO = Path(__file__).resolve().parents[1]
TABLE_DIR = Path("crates/perry-codegen/src/lower_call/native_table")
LEDGER = Path("scripts/native_result_ledger.tsv")
# The campaign's textual census reported 372 `ret: NR_PTR` hits. Two were
# prose comments in fastify.rs, while one real row uses the positional `cr(...)`
# helper, leaving 371 executable declarations. The scanner parses declarations,
# not comments, and includes that helper row.
EXPECTED_ROWS = 371
EXPECTED_PROVIDERS = 322
KINDS = {
    "NR_GCPTR",
    "NR_NULLABLE_GCPTR",
    "NR_HANDLE_ID",
    "NR_FOREIGN_PTR",
    "NR_JS_VALUE",
}
RET_RE = re.compile(r"\bret:\s*(NR_[A-Z0-9_]+)\b")
RUNTIME_RE = re.compile(r'\bruntime:\s*"([^"]+)"')


class LedgerError(RuntimeError):
    pass


@dataclass(frozen=True)
class Row:
    source: Path
    line: int
    runtime: str
    kind: str


@dataclass(frozen=True)
class Provider:
    kind: str
    source: Path
    rust_return: str


def scan_rows(root: Path, table_dir: Path) -> list[Row]:
    rows: list[Row] = []
    for source in sorted((root / table_dir).rglob("*.rs")):
        lines = source.read_text().splitlines()
        for index, line in enumerate(lines):
            if line.lstrip().startswith("//"):
                continue
            ret = RET_RE.search(line)
            positional = re.fullmatch(r"\s*(NR_[A-Z0-9_]+),\s*", line)
            if not ret and not positional:
                continue
            kind = (ret or positional).group(1)
            if kind == "NR_PTR":
                raise LedgerError(
                    f"{source.relative_to(root)}:{index + 1}: legacy NR_PTR row"
                )
            if kind not in KINDS:
                continue
            runtime = None
            if ret:
                for previous in reversed(lines[:index]):
                    match = RUNTIME_RE.search(previous)
                    if match:
                        runtime = match.group(1)
                        break
                    if "NativeModSig {" in previous:
                        break
            else:
                strings: list[str] = []
                for previous in reversed(lines[:index]):
                    strings.extend(re.findall(r'"([^"]+)"', previous))
                    if re.search(r"\bcr\s*\(", previous):
                        break
                if len(strings) >= 2:
                    runtime = strings[-2]
            if runtime is None:
                raise LedgerError(
                    f"{source.relative_to(root)}:{index + 1}: typed row has no runtime symbol"
                )
            rows.append(Row(source.relative_to(root), index + 1, runtime, kind))
    return rows


def read_providers(root: Path, ledger_path: Path) -> dict[str, Provider]:
    path = root / ledger_path
    providers: dict[str, Provider] = {}
    for line_number, line in enumerate(path.read_text().splitlines(), 1):
        if not line or line.startswith("#"):
            continue
        fields = line.split("\t")
        if len(fields) != 4:
            raise LedgerError(f"{ledger_path}:{line_number}: expected four TSV fields")
        symbol, kind, source_text, rust_return = fields
        if symbol in providers:
            raise LedgerError(f"{ledger_path}:{line_number}: duplicate provider {symbol}")
        if kind not in KINDS:
            raise LedgerError(
                f"{ledger_path}:{line_number}: provider {symbol} lacks a result class"
            )
        source = Path(source_text)
        provider_path = root / source
        if not provider_path.is_file():
            raise LedgerError(
                f"{ledger_path}:{line_number}: provider source does not exist: {source}"
            )
        definition = re.compile(rf"\bfn\s+{re.escape(symbol)}\s*\(")
        if not definition.search(provider_path.read_text()):
            raise LedgerError(
                f"{ledger_path}:{line_number}: {source} does not declare {symbol}"
            )
        providers[symbol] = Provider(kind, source, rust_return)
    return providers


def check(
    root: Path,
    table_dir: Path = TABLE_DIR,
    ledger_path: Path = LEDGER,
    expected_rows: int = EXPECTED_ROWS,
    expected_providers: int = EXPECTED_PROVIDERS,
) -> dict[str, int]:
    rows = scan_rows(root, table_dir)
    providers = read_providers(root, ledger_path)
    if len(rows) != expected_rows:
        raise LedgerError(f"expected {expected_rows} classified rows, found {len(rows)}")
    if len(providers) != expected_providers:
        raise LedgerError(
            f"expected {expected_providers} classified providers, found {len(providers)}"
        )

    used: set[str] = set()
    counts = {kind: 0 for kind in sorted(KINDS)}
    for row in rows:
        provider = providers.get(row.runtime)
        if provider is None:
            raise LedgerError(
                f"{row.source}:{row.line}: provider {row.runtime} lacks a class"
            )
        if provider.kind != row.kind:
            raise LedgerError(
                f"{row.source}:{row.line}: {row.runtime} is {row.kind}, "
                f"provider ledger says {provider.kind}"
            )
        used.add(row.runtime)
        counts[row.kind] += 1

    stale = sorted(set(providers) - used)
    if stale:
        raise LedgerError(f"stale provider ledger entries: {', '.join(stale)}")
    return {kind: count for kind, count in counts.items() if count}


def self_test() -> None:
    with tempfile.TemporaryDirectory(prefix="native-result-ledger-") as tmp:
        root = Path(tmp)
        tables = root / "tables"
        providers = root / "providers"
        tables.mkdir()
        providers.mkdir()
        provider = providers / "fixture.rs"
        provider.write_text('pub extern "C" fn fixture_provider() -> *mut u8 { panic!() }\n')
        ledger = root / "ledger.tsv"
        ledger.write_text(
            "# runtime_symbol\tresult_kind\tprovider_source\tprovider_return\n"
            "fixture_provider\tNR_GCPTR\tproviders/fixture.rs\t*mut u8\n"
        )
        good = (
            'NativeModSig {\n    runtime: "fixture_provider",\n'
            "    ret: NR_GCPTR,\n}\n"
        )
        table = tables / "fixture.rs"
        table.write_text(good)
        check(root, Path("tables"), Path("ledger.tsv"), 1, 1)

        # Sabotage: restoring the erased result kind must turn the gate red.
        table.write_text(good.replace("NR_GCPTR", "NR_PTR"))
        try:
            check(root, Path("tables"), Path("ledger.tsv"), 1, 1)
        except LedgerError as error:
            if "legacy NR_PTR" not in str(error):
                raise
        else:
            raise LedgerError("self-test accepted a planted NR_PTR row")

        # Sabotage: a table/provider disagreement must also turn it red.
        table.write_text(good.replace("NR_GCPTR", "NR_HANDLE_ID"))
        try:
            check(root, Path("tables"), Path("ledger.tsv"), 1, 1)
        except LedgerError as error:
            if "provider ledger says" not in str(error):
                raise
        else:
            raise LedgerError("self-test accepted an unclassified provider declaration")
    print("native_result_ledger self-test passed (NR_PTR and provider-class sabotages rejected)")


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args()
    try:
        if args.self_test:
            self_test()
        else:
            counts = check(REPO)
            rendered = " ".join(f"{kind}={count}" for kind, count in sorted(counts.items()))
            print(
                "native_result_ledger passed: "
                f"{EXPECTED_ROWS} rows, {EXPECTED_PROVIDERS} providers; {rendered}"
            )
    except (LedgerError, OSError) as error:
        print(f"native_result_ledger FAILED: {error}")
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
