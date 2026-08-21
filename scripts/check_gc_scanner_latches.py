#!/usr/bin/env python3
"""Reject process-global latches around thread-local GC scanner registration.

`MUTABLE_ROOT_SCANNERS` and the FFI scanner registries are thread-local. A
process-global `Once`, `OnceLock`, `AtomicBool`, or `Mutex<bool>` therefore
cannot guard a `gc_register_*root_scanner*` call: the first thread consumes the
latch and all later heaps run without that scanner (#8530).

This build-free gate follows the latch into its `call_once` / `get_or_init`
closure (and the equivalent atomic `if` body). It deliberately permits a
process-global latch for unrelated setup in the same function, which is needed
when registration and genuinely process-wide callback wiring are split.
"""

from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
CRATES = ("crates/perry-runtime/src", "crates/perry-stdlib/src")
MIN_REGISTRATIONS = 30

THREAD_LOCAL_RE = re.compile(
    r"(?m)^[ \t]*(?:(?:std|crate)::)?(?:thread_local|perry_thread_local)!\s*\{"
)
STATIC_RE = re.compile(
    r"\bstatic\s+([A-Za-z_][A-Za-z_0-9]*)\s*:\s*([^=;]+?)\s*="
)
PROCESS_LATCH_TYPE_RE = re.compile(
    r"(?:\bOnce\b|\bOnceLock\s*<|\bAtomicBool\b|\bMutex\s*<\s*bool\s*>)"
)
REGISTER_RE = re.compile(
    r"\b(?:gc|perry_ffi_gc)_register_[A-Za-z_0-9]*root_scanner[A-Za-z_0-9]*\s*\("
)
ONCE_METHODS = ("call_once", "get_or_init", "get_or_try_init")
ATOMIC_GUARD_METHODS = ("load", "swap", "compare_exchange", "compare_exchange_weak")
RAW_STRING_RE = re.compile(r"(?:br|r)(?P<hashes>#{0,16})\"")


def read_source(path: Path) -> str:
    return path.read_text(encoding="utf-8")


def mask_non_code(src: str) -> str:
    """Blank comments and literals while preserving offsets and newlines."""
    out = list(src)
    i = 0
    while i < len(src):
        if src.startswith("//", i):
            end = src.find("\n", i + 2)
            end = len(src) if end < 0 else end
            for j in range(i, end):
                out[j] = " "
            i = end
            continue
        if src.startswith("/*", i):
            depth = 1
            j = i + 2
            while j < len(src) and depth:
                if src.startswith("/*", j):
                    depth += 1
                    j += 2
                elif src.startswith("*/", j):
                    depth -= 1
                    j += 2
                else:
                    j += 1
            for k in range(i, j):
                if out[k] != "\n":
                    out[k] = " "
            i = j
            continue

        raw = RAW_STRING_RE.match(src, i)
        if raw:
            marker = '"' + raw.group("hashes")
            start = i
            end = src.find(marker, raw.end())
            end = len(src) if end < 0 else end + len(marker)
            for j in range(start, end):
                if out[j] != "\n":
                    out[j] = " "
            i = end
            continue

        if src[i] == '"':
            j = i + 1
            while j < len(src):
                if src[j] == "\\":
                    j += 2
                    continue
                j += 1
                if src[j - 1] == '"':
                    break
            for k in range(i, min(j, len(src))):
                if out[k] != "\n":
                    out[k] = " "
            i = j
            continue

        # Mask character literals without mistaking Rust lifetimes (`'a`) for
        # literals. A real char closes on this line and within a few bytes.
        if src[i] == "'":
            j = i + 1
            if j < len(src) and src[j] == "\\":
                j += 2
            else:
                j += 1
            if j < len(src) and src[j] == "'":
                j += 1
                for k in range(i, j):
                    out[k] = " "
                i = j
                continue
        i += 1
    return "".join(out)


def matching_delimiter(src: str, opening: int, left: str, right: str) -> int | None:
    depth = 0
    for i in range(opening, len(src)):
        if src[i] == left:
            depth += 1
        elif src[i] == right:
            depth -= 1
            if depth == 0:
                return i
    return None


def thread_local_spans(masked: str) -> list[tuple[int, int]]:
    spans: list[tuple[int, int]] = []
    for match in THREAD_LOCAL_RE.finditer(masked):
        opening = masked.find("{", match.start(), match.end())
        closing = matching_delimiter(masked, opening, "{", "}")
        if closing is not None:
            spans.append((opening, closing))
    return spans


def process_latches(masked: str) -> set[str]:
    tls_spans = thread_local_spans(masked)
    result: set[str] = set()
    for match in STATIC_RE.finditer(masked):
        if not PROCESS_LATCH_TYPE_RE.search(match.group(2)):
            continue
        if any(start < match.start() < end for start, end in tls_spans):
            continue
        result.add(match.group(1))
    return result


def line_number(src: str, offset: int) -> int:
    return src.count("\n", 0, offset) + 1


def scan_source(src: str, rel: str) -> tuple[list[str], int]:
    masked = mask_non_code(src)
    registrations = len(REGISTER_RE.findall(masked))
    problems: list[str] = []

    for latch in sorted(process_latches(masked)):
        for method in ONCE_METHODS:
            use_re = re.compile(rf"\b{re.escape(latch)}\s*\.\s*{method}\s*\(")
            for match in use_re.finditer(masked):
                opening = masked.find("(", match.start(), match.end())
                closing = matching_delimiter(masked, opening, "(", ")")
                if closing is None or not REGISTER_RE.search(masked, opening, closing):
                    continue
                problems.append(
                    f"{rel}:{line_number(src, match.start())}: process-global `{latch}` "
                    f"uses `{method}` around GC scanner registration; the scanner registry "
                    "is thread-local, so use a thread-local `Cell<bool>` latch"
                )

        for method in ATOMIC_GUARD_METHODS:
            use_re = re.compile(rf"\b{re.escape(latch)}\s*\.\s*{method}\s*\(")
            for match in use_re.finditer(masked):
                opening = masked.find("(", match.start(), match.end())
                closing = matching_delimiter(masked, opening, "(", ")")
                if closing is None:
                    continue
                block_open = masked.find("{", closing + 1, min(len(masked), closing + 300))
                if block_open < 0 or ";" in masked[closing + 1 : block_open]:
                    continue
                condition_start = masked.rfind("if", max(0, match.start() - 300), match.start())
                if condition_start < 0:
                    continue
                block_close = matching_delimiter(masked, block_open, "{", "}")
                if block_close is None or not REGISTER_RE.search(masked, block_open, block_close):
                    continue
                problems.append(
                    f"{rel}:{line_number(src, match.start())}: process-global `{latch}` "
                    f"uses atomic `{method}` to guard GC scanner registration; the scanner "
                    "registry is thread-local, so use a thread-local `Cell<bool>` latch"
                )

        lock_re = re.compile(
            rf"\blet\s+(?:mut\s+)?(?P<guard>[A-Za-z_][A-Za-z_0-9]*)\s*=\s*"
            rf"{re.escape(latch)}\s*\.\s*(?:lock|try_lock)\s*\("
        )
        for match in lock_re.finditer(masked):
            initializer_end = masked.find(";", match.end(), min(len(masked), match.end() + 1000))
            if initializer_end < 0:
                continue
            guard = re.escape(match.group("guard"))
            guarded_if = re.compile(rf"\bif\s*!\s*\*?\s*{guard}\b[^{{;]*\{{")
            if_match = guarded_if.search(
                masked, initializer_end + 1, min(len(masked), initializer_end + 1000)
            )
            if if_match is None:
                continue
            block_open = masked.find("{", if_match.start(), if_match.end())
            block_close = matching_delimiter(masked, block_open, "{", "}")
            if block_close is None or not REGISTER_RE.search(masked, block_open, block_close):
                continue
            problems.append(
                f"{rel}:{line_number(src, match.start())}: process-global `{latch}` uses a "
                "mutex boolean to guard GC scanner registration; the scanner registry is "
                "thread-local, so use a thread-local `Cell<bool>` latch"
            )

    return problems, registrations


def scan_tree(root: Path, enforce_floor: bool = True) -> tuple[list[str], int]:
    problems: list[str] = []
    registrations = 0
    for crate in CRATES:
        for path in sorted((root / crate).rglob("*.rs")):
            rel = path.relative_to(root).as_posix()
            found, count = scan_source(read_source(path), rel)
            problems.extend(found)
            registrations += count
    if enforce_floor and registrations < MIN_REGISTRATIONS:
        problems.append(
            f"scanner-registration scan found only {registrations} calls; expected at least "
            f"{MIN_REGISTRATIONS}, so the gate's scope or parser has silently shrunk"
        )
    return sorted(problems), registrations


def self_test() -> int:
    bad_shapes = {
        "Once::call_once": """
static REGISTERED: Once = Once::new();
fn ensure() { REGISTERED.call_once(|| gc_register_mutable_root_scanner(scan)); }
""",
        "OnceLock::get_or_init": """
static REGISTERED: OnceLock<()> = OnceLock::new();
fn ensure() { REGISTERED.get_or_init(|| { gc_register_mutable_root_scanner_named("x", scan); }); }
""",
        "AtomicBool::swap": """
static REGISTERED: AtomicBool = AtomicBool::new(false);
fn ensure() {
    if !REGISTERED.swap(true, Ordering::AcqRel) {
        gc_register_mutable_root_scanner(scan);
    }
}
""",
        "Mutex<bool>": """
static REGISTERED: Mutex<bool> = Mutex::new(false);
fn ensure() {
    let mut registered = REGISTERED.lock().unwrap();
    if !*registered {
        gc_register_mutable_root_scanner(scan);
        *registered = true;
    }
}
""",
    }
    good_shapes = {
        "thread-local Once": """
thread_local! { static REGISTERED: Once = Once::new(); }
fn ensure() { REGISTERED.with(|r| r.call_once(|| gc_register_mutable_root_scanner(scan))); }
""",
        "thread-local Cell": """
thread_local! { static REGISTERED: Cell<bool> = const { Cell::new(false) }; }
fn ensure() {
    REGISTERED.with(|r| {
        if !r.get() { gc_register_mutable_root_scanner(scan); r.set(true); }
    });
}
""",
        "split process setup": """
static CALLBACKS: Once = Once::new();
thread_local! { static REGISTERED: Cell<bool> = const { Cell::new(false) }; }
fn ensure() {
    REGISTERED.with(|r| { if !r.get() { gc_register_mutable_root_scanner(scan); r.set(true); } });
    CALLBACKS.call_once(register_process_callbacks);
}
""",
        "comments and strings": r'''
static REGISTERED: Once = Once::new();
// REGISTERED.call_once(|| gc_register_mutable_root_scanner(scan));
const NOTE: &str = "REGISTERED.call_once(|| gc_register_mutable_root_scanner(scan))";
''',
    }

    failures: list[str] = []
    for label, src in bad_shapes.items():
        problems, _ = scan_source(src, "probe.rs")
        if not problems:
            failures.append(f"{label} was not rejected")
    for label, src in good_shapes.items():
        problems, _ = scan_source(src, "probe.rs")
        if problems:
            failures.append(f"{label} was rejected: {'; '.join(problems)}")

    if failures:
        for failure in failures:
            print(f"self-test FAILED: {failure}", file=sys.stderr)
        return 1
    print(
        "GC scanner latch self-test: OK "
        f"({len(bad_shapes)} bad shapes rejected, {len(good_shapes)} good shapes accepted)"
    )
    return 0


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--self-test", action="store_true", help="prove the checker can fail")
    args = parser.parse_args()
    if args.self_test:
        return self_test()

    problems, registrations = scan_tree(REPO)
    if problems:
        print("process-global GC scanner registration latches found:\n", file=sys.stderr)
        for problem in problems:
            print(f"  {problem}", file=sys.stderr)
        return 1
    print(
        f"GC scanner latch policy OK: {registrations} registration calls checked "
        "across perry-runtime and perry-stdlib"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
