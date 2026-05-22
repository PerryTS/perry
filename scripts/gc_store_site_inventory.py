#!/usr/bin/env python3
"""Audit raw GC-relevant store sites.

The generational collector relies on every raw heap/slot write being either
barriered, rooted, initialization-only, pointer-free, or stack-local. This
script scans the first-party paths where raw GC-relevant stores are expected
and requires a nearby `GC_STORE_AUDIT(...)` marker with a reason.
"""

from __future__ import annotations

import re
import sys
from dataclasses import dataclass
from pathlib import Path
from typing import Iterable


REPO_ROOT = Path(__file__).resolve().parents[1]

AUDIT_CLASSES = {
    "BARRIERED",
    "EXTERNAL_BARRIERED",
    "ROOT",
    "INIT",
    "POINTER_FREE",
    "STACK",
}

MARKER_RE = re.compile(
    r"GC_STORE_AUDIT\((" + "|".join(sorted(AUDIT_CLASSES)) + r")\):\s*\S"
)

CODEGEN_DEST_RE = re.compile(
    r"\.store\([^,]+,\s*[^,]+,\s*&?(?P<dest>[A-Za-z_][A-Za-z0-9_]*)\)"
)
CODEGEN_EMIT_RAW_STORE_RE = re.compile(r'emit_raw\(format!\("store\b')

RUST_FIELD_STORE_RE = re.compile(
    r"\(\*[^)\n]+\)\.(?P<field>keys_array|entries|elements)\s*="
)
RUST_POINTER_FIELD_STORE_RE = re.compile(
    r"\b(?P<owner>[A-Za-z_][A-Za-z0-9_]*)\.(?P<field>string_ptr)\s*="
)
RUST_GLOBAL_INDEX_STORE_RE = re.compile(
    r"\b(?P<target>[A-Z][A-Z0-9_]*)\s*\[[^\]]+\]\s*="
)
RUST_TLS_INDEX_STORE_RE = re.compile(
    r"\(\*[A-Za-z_][A-Za-z0-9_]*\.get\(\)\)\[[^\]]+\]\s*="
)

RUST_PTR_STORE_RE = re.compile(r"\b(?:std::)?ptr::write(?:_unaligned)?\s*\(")
RUST_COPY_RE = re.compile(r"\b(?:std::)?ptr::copy(?:_nonoverlapping)?\s*\(")
RUST_DEREF_ASSIGN_RE = re.compile(
    r"\*(?P<target>[A-Za-z_][A-Za-z0-9_]*)(?:\.add\([^)]*\))?\s*=(?!=)"
)
RUST_ATOMIC_STORE_RE = re.compile(
    r"\b(?P<target>[A-Za-z_][A-Za-z0-9_]*)\.store\s*\(\s*(?P<value>[^,]+)"
)
RUST_ATOMIC_COMPARE_EXCHANGE_RE = re.compile(
    r"\b(?P<target>[A-Za-z_][A-Za-z0-9_]*)\.compare_exchange\s*\(\s*[^,]+,\s*(?P<value>[^,]+)"
)


SCAN_PATHS = [
    Path("crates/perry-codegen/src/expr"),
    Path("crates/perry-runtime/src/array.rs"),
    Path("crates/perry-runtime/src/object"),
    Path("crates/perry-runtime/src/closure.rs"),
    Path("crates/perry-runtime/src/json.rs"),
    Path("crates/perry-runtime/src/regex.rs"),
    Path("crates/perry-runtime/src/plugin.rs"),
    Path("crates/perry-runtime/src/thread.rs"),
    Path("crates/perry-runtime/src/promise.rs"),
    Path("crates/perry-runtime/src/map.rs"),
    Path("crates/perry-runtime/src/set.rs"),
    Path("crates/perry-runtime/src/string.rs"),
    Path("crates/perry-runtime/src/typedarray.rs"),
    Path("crates/perry-runtime/src/buffer.rs"),
    Path("crates/perry-stdlib/src"),
]


CODEGEN_HEAP_DEST_HINTS = (
    "arr_header_addr",
    "arr_ptr",
    "byte_ptr",
    "elem_ptr",
    "element_addr",
    "element_ptr",
    "field_addr",
    "field_ptr",
    "g_ref",
    "offset_field_ptr",
    "raw",
    "storage",
)

RUST_COPY_RISK_HINTS = (
    "arr_elements",
    "dst,",
    "dst)",
    "dst.add",
    "dst_data",
    "dst_elements",
    "elements.add",
    "elements_ptr",
    "fields_ptr",
    "new_ptr",
    "pair_elems",
    "result_elems",
    "rewritten_captures",
    "src_elements",
)

RUST_POINTER_FREE_COPY_HINTS = (
    "body",
    "buf_data",
    "buffer_data",
    "bytes",
    "data_ptr",
    "hash",
    "key_bytes",
    "last_char",
    "part.as_ptr",
    "property_name",
    "source_data",
    "str_bytes",
)

STACK_COPY_HINTS = (
    "heap_buf.as_mut_ptr",
    "regular_args",
    "spread_data",
    "stack_buf.as_mut_ptr",
)

RUST_DEREF_RISK_TARGETS = (
    "arr_data",
    "captures_ptr",
    "dst",
    "dst_captures",
    "dst_data",
    "dst_elements",
    "dst_fields",
    "elements",
    "elements_ptr",
    "fields",
    "new_keys_elements",
    "pair_elems",
    "result_elements",
)

RUST_ATOMIC_ROOT_TARGET_HINTS = (
    "CACHE",
    "CACHED",
    "GLOBAL",
    "ROOT",
    "SINGLETON",
    "PTR",
)

RUST_ATOMIC_ROOT_VALUE_HINTS = (
    "addr",
    "bits",
    "new_ptr",
    "ptr",
    "to_bits",
    "value",
)

RUST_GLOBAL_INDEX_RISK_TARGET_HINTS = (
    "CACHE",
    "GLOBAL",
    "ROOT",
    "TABLE",
)

RUST_GLOBAL_INDEX_RISK_EXACT_TARGETS = {
    "INTERN_TABLE",
    "SMALL_INT_CACHE",
    "TRANSITION_CACHE_GLOBAL",
}

RUST_GLOBAL_INDEX_POINTER_HINTS = (
    "key_ptr",
    "keys_array",
    "next_keys",
    "old_entry",
    "ptr",
    "string_ptr",
)


@dataclass(frozen=True)
class Finding:
    path: Path
    line_no: int
    text: str
    reason: str

    def render(self) -> str:
        rel = self.path.relative_to(REPO_ROOT)
        return f"{rel}:{self.line_no}: {self.reason}: {self.text.strip()}"


def iter_scan_roots() -> Iterable[Path]:
    for rel in SCAN_PATHS:
        root = REPO_ROOT / rel
        if root.is_file():
            yield root
        elif root.is_dir():
            yield from sorted(root.rglob("*.rs"))

    for ext_dir in sorted((REPO_ROOT / "crates").glob("perry-ext-*")):
        src = ext_dir / "src"
        if src.is_dir():
            yield from sorted(src.rglob("*.rs"))


def is_comment_or_blank(line: str) -> bool:
    stripped = line.strip()
    return not stripped or stripped.startswith("//") or stripped.startswith("///")


def call_window(lines: list[str], index: int) -> str:
    """Return a small multiline window for classifying split calls."""

    start = index
    end = min(len(lines), index + 6)
    return " ".join(line.strip() for line in lines[start:end])


def has_nearby_marker(lines: list[str], index: int) -> bool:
    start = max(0, index - 6)
    end = min(len(lines), index + 7)
    return any(MARKER_RE.search(lines[i]) for i in range(start, end))


def is_risky_codegen_store(line: str) -> bool:
    if CODEGEN_EMIT_RAW_STORE_RE.search(line):
        return True
    match = CODEGEN_DEST_RE.search(line)
    if not match:
        return False
    dest = match.group("dest")
    return dest in CODEGEN_HEAP_DEST_HINTS


def classify_rust_store(path: Path, lines: list[str], index: int) -> str | None:
    line = lines[index]
    window = call_window(lines, index)
    atomic_store = RUST_ATOMIC_STORE_RE.search(window)
    if atomic_store and is_risky_atomic_root_store(
        atomic_store.group("target"), atomic_store.group("value")
    ):
        return "raw atomic cache/global pointer store"

    atomic_cas = RUST_ATOMIC_COMPARE_EXCHANGE_RE.search(window)
    if atomic_cas and is_risky_atomic_root_store(
        atomic_cas.group("target"), atomic_cas.group("value")
    ):
        return "raw atomic cache/global pointer CAS"

    global_index = RUST_GLOBAL_INDEX_STORE_RE.search(line)
    if global_index and is_risky_global_index_store(global_index.group("target"), window):
        return "raw cache/global pointer table store"

    if RUST_TLS_INDEX_STORE_RE.search(line) and is_risky_tls_index_store(window):
        return "raw TLS cache pointer table store"

    pointer_field = RUST_POINTER_FIELD_STORE_RE.search(line)
    if pointer_field:
        return "raw cache/global pointer field store"

    deref = RUST_DEREF_ASSIGN_RE.search(line)
    if deref and any(hint in deref.group("target") for hint in RUST_DEREF_RISK_TARGETS):
        if path.name in {"buffer.rs", "typedarray.rs"}:
            return None
        return "raw direct slot assignment"

    if RUST_FIELD_STORE_RE.search(line):
        return "raw heap pointer field store"

    if RUST_PTR_STORE_RE.search(line):
        if any(hint in window for hint in STACK_COPY_HINTS):
            return "raw stack/temporary argument store"
        return "raw slot write"

    if RUST_COPY_RE.search(line):
        if any(hint in window for hint in STACK_COPY_HINTS):
            return "raw stack/temporary argument copy"
        if path.name in {"string.rs", "buffer.rs", "typedarray.rs"}:
            return None
        if any(hint in window for hint in RUST_POINTER_FREE_COPY_HINTS):
            return None
        if any(hint in window for hint in RUST_COPY_RISK_HINTS):
            return "raw slot copy"
        if path.name == "array.rs":
            return "raw array slot copy"
    return None


def is_risky_atomic_root_store(target: str, value: str) -> bool:
    target_upper = target.upper()
    value_lower = value.lower()
    if not any(hint in target_upper for hint in RUST_ATOMIC_ROOT_TARGET_HINTS):
        return False
    return any(hint in value_lower for hint in RUST_ATOMIC_ROOT_VALUE_HINTS)


def is_risky_global_index_store(target: str, window: str) -> bool:
    target_upper = target.upper()
    if target_upper not in RUST_GLOBAL_INDEX_RISK_EXACT_TARGETS and not any(
        hint in target_upper for hint in RUST_GLOBAL_INDEX_RISK_TARGET_HINTS
    ):
        return False
    window_lower = window.lower()
    return target_upper in RUST_GLOBAL_INDEX_RISK_EXACT_TARGETS or any(
        hint in window_lower for hint in RUST_GLOBAL_INDEX_POINTER_HINTS
    )


def is_risky_tls_index_store(window: str) -> bool:
    window_lower = window.lower()
    return "cache" in window_lower and any(
        hint in window_lower for hint in RUST_GLOBAL_INDEX_POINTER_HINTS
    )


def scan_file(path: Path) -> list[Finding]:
    try:
        lines = path.read_text(encoding="utf-8").splitlines()
    except UnicodeDecodeError:
        lines = path.read_text(encoding="utf-8", errors="replace").splitlines()

    findings: list[Finding] = []
    for index, line in enumerate(lines):
        if is_comment_or_blank(line):
            continue

        reason: str | None = None
        if "crates/perry-codegen/src/expr" in path.as_posix():
            if is_risky_codegen_store(line):
                reason = "raw generated heap/global store"
        else:
            reason = classify_rust_store(path, lines, index)

        if reason and not has_nearby_marker(lines, index):
            findings.append(Finding(path, index + 1, line, reason))

    return findings


def run_self_tests() -> int:
    failures: list[str] = []

    def check(rel_path: str, lines: list[str], expected: str | None) -> None:
        reason = classify_rust_store(REPO_ROOT / rel_path, lines, 0)
        if expected is None:
            if reason is not None:
                failures.append(f"{rel_path}: expected clean, got {reason!r}")
        elif reason is None or expected not in reason:
            failures.append(f"{rel_path}: expected {expected!r}, got {reason!r}")

    check(
        "crates/perry-runtime/src/array.rs",
        ["*dst.add(i) = *src.add(i);"],
        "raw direct slot assignment",
    )
    check(
        "crates/perry-runtime/src/object/field_get_set.rs",
        ["*dst_data.add(i) = *src_data.add(i);"],
        "raw direct slot assignment",
    )
    check(
        "crates/perry-runtime/src/array.rs",
        ["std::ptr::copy_nonoverlapping(src, dst, len as usize);"],
        "raw slot copy",
    )
    check(
        "crates/perry-runtime/src/array.rs",
        [
            "std::ptr::copy(",
            "    elements.add(s as usize),",
            "    elements.add(t as usize),",
            "    count as usize,",
            ");",
        ],
        "raw slot copy",
    )
    check(
        "crates/perry-runtime/src/buffer.rs",
        ["ptr::copy_nonoverlapping(src_data, dst_data, buf_len);"],
        None,
    )
    check(
        "crates/perry-stdlib/src/crypto.rs",
        ["std::ptr::copy_nonoverlapping(bytes.as_ptr(), dst, bytes.len());"],
        None,
    )
    check(
        "crates/perry-runtime/src/object/mod.rs",
        ["CACHED.store(value.to_bits(), Ordering::Relaxed);"],
        "raw atomic cache/global pointer store",
    )
    check(
        "crates/perry-runtime/src/object/mod.rs",
        ["match GLOBAL_THIS_PTR.compare_exchange(0, new_ptr, Ordering::AcqRel, Ordering::Acquire) {"],
        "raw atomic cache/global pointer CAS",
    )
    check(
        "crates/perry-runtime/src/string.rs",
        ["SMALL_INT_CACHE[idx] = ptr;"],
        "raw cache/global pointer table store",
    )
    check(
        "crates/perry-runtime/src/string.rs",
        ["entry.string_ptr = key as usize;"],
        "raw cache/global pointer field store",
    )
    check(
        "crates/perry-runtime/src/string.rs",
        [
            "INTERN_TABLE[0] = InternEntry {",
            "    hash: 0xC0DEC0DE,",
            "    string_ptr,",
            "};",
        ],
        "raw cache/global pointer table store",
    )
    check(
        "crates/perry-runtime/src/object/mod.rs",
        [
            "TRANSITION_CACHE_GLOBAL[slot] = TransitionEntry {",
            "    prev_keys,",
            "    key_ptr: kp,",
            "    next_keys,",
            "};",
        ],
        "raw cache/global pointer table store",
    )
    check(
        "crates/perry-runtime/src/object/mod.rs",
        [
            "(*cache.get())[slot] = ShapeCacheEntry {",
            "    shape_id,",
            "    keys_array,",
            "};",
        ],
        "raw TLS cache pointer table store",
    )
    check(
        "crates/perry-runtime/src/object/mod.rs",
        ["NEXT_ID.store(1, Ordering::Relaxed);"],
        None,
    )
    check(
        "crates/perry-runtime/src/object/mod.rs",
        ["READY.store(true, Ordering::Release);"],
        None,
    )
    check(
        "crates/perry-runtime/src/json.rs",
        ["std::ptr::write(slot, JSValue::from_bits(value_bits));"],
        "raw slot write",
    )
    check(
        "crates/perry-runtime/src/regex.rs",
        ["std::ptr::write(elements_ptr.add(i), nanboxed);"],
        "raw slot write",
    )
    check(
        "crates/perry-runtime/src/plugin.rs",
        ["*fields.add(1) = make_nanboxed_string(&name);"],
        "raw direct slot assignment",
    )
    check(
        "crates/perry-runtime/src/plugin.rs",
        ["(*obj).keys_array = keys_arr;"],
        "raw heap pointer field store",
    )
    check(
        "crates/perry-runtime/src/thread.rs",
        ["*arr_elements.add(i) = f64::from_bits(bits);"],
        "raw direct slot assignment",
    )
    check(
        "crates/perry-runtime/src/thread.rs",
        ["*fields_ptr.add(i) = f64::from_bits(bits);"],
        "raw direct slot assignment",
    )
    check(
        "crates/perry-runtime/src/thread.rs",
        ["*keys_elements.add(i) = f64::from_bits(key_val.bits());"],
        "raw direct slot assignment",
    )
    check(
        "crates/perry-runtime/src/promise.rs",
        ["*fields.add(0) = promise_box_handle.get_nanbox_f64();"],
        "raw direct slot assignment",
    )

    if failures:
        print("GC store-site inventory self-test failed:")
        for failure in failures:
            print(f"  {failure}")
        return 1

    print("GC store-site inventory self-test passed.")
    return 0


def main(argv: list[str] | None = None) -> int:
    argv = sys.argv[1:] if argv is None else argv
    if argv == ["--self-test"]:
        return run_self_tests()
    if argv:
        print("usage: gc_store_site_inventory.py [--self-test]", file=sys.stderr)
        return 2

    findings: list[Finding] = []
    seen: set[Path] = set()
    for path in iter_scan_roots():
        if path in seen:
            continue
        seen.add(path)
        findings.extend(scan_file(path))

    if findings:
        print("GC store-site inventory failed; add nearby GC_STORE_AUDIT markers:")
        for finding in findings:
            print(f"  {finding.render()}")
        print(
            "\nAccepted marker form: "
            "// GC_STORE_AUDIT(BARRIERED): reason, with class one of "
            + ", ".join(sorted(AUDIT_CLASSES))
        )
        return 1

    print(f"GC store-site inventory passed ({len(seen)} files scanned).")
    return 0


if __name__ == "__main__":
    sys.exit(main())
