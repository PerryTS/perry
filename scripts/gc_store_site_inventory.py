#!/usr/bin/env python3
"""Audit raw GC-relevant store sites.

The generational collector relies on every raw heap/slot write being either
barriered, rooted, initialization-only, pointer-free, or stack-local. This
script scans the first-party paths where raw GC-relevant stores are expected
and requires a nearby `GC_STORE_AUDIT(...)` marker with a reason.
"""

from __future__ import annotations

import argparse
import json
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
    r"\.store(?:_aligned|_volatile)?\([^,]+,\s*[^,]+,\s*&?(?P<dest>[A-Za-z_][A-Za-z0-9_]*)\s*[,)]"
)
CODEGEN_EMIT_RAW_STORE_RE = re.compile(r'emit_raw\(format!\("store\b')

RUST_FIELD_STORE_RE = re.compile(
    r"\(\*[^)\n]+\)\.(?P<field>keys_array|entries|elements)\s*="
)
RUST_PROMISE_FIELD_STORE_RE = re.compile(
    r"\(\*[^)\n]+\)\.(?P<field>on_fulfilled|on_rejected|next)\s*="
)
RUST_POINTER_FIELD_STORE_RE = re.compile(
    r"\b(?P<owner>[A-Za-z_][A-Za-z0-9_]*)\.(?P<field>string_ptr)\s*=(?!=)"
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


# The whole runtime is mutator code that can store JSValues into GC-managed
# memory; scan all of it so new modules can't land unaudited. The collector
# itself (crates/perry-runtime/src/gc/) legitimately performs raw stores —
# it is excluded via the allowlist file with a justification.
SCAN_PATHS = [
    Path("crates/perry-codegen/src"),
    Path("crates/perry-runtime/src"),
    Path("crates/perry-stdlib/src"),
]

DEFAULT_ALLOWLIST = REPO_ROOT / "scripts" / "gc_store_site_allowlist.txt"


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
    # #8185: the shared barrier emitters store through `slot_ptr` /
    # `root_slot`; without these hints, deleting their GC_STORE_AUDIT marker
    # was invisible to this scanner (the marker-deletion sabotage passed).
    "root_slot",
    "slot_ptr",
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


@dataclass
class AllowlistEntry:
    path_prefix: str
    line_substring: str  # "*" matches any line
    justification: str
    source_line: int
    hits: int = 0

    def matches(self, finding: Finding) -> bool:
        rel = repo_rel(finding.path)
        if not rel.startswith(self.path_prefix):
            return False
        return self.line_substring == "*" or self.line_substring in finding.text


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


def repo_rel(path: Path) -> str:
    try:
        return path.relative_to(REPO_ROOT).as_posix()
    except ValueError:
        return path.as_posix()


def is_codegen_path(path: Path) -> bool:
    return repo_rel(path).startswith("crates/perry-codegen/src/")


def is_runtime_module(path: Path, module: str) -> bool:
    rel = repo_rel(path)
    flat = f"crates/perry-runtime/src/{module}.rs"
    directory = f"crates/perry-runtime/src/{module}/"
    return rel == flat or rel.startswith(directory)


def is_pointer_free_module(path: Path) -> bool:
    """Modules whose element storage is raw bytes/numerics, never JSValues."""

    return (
        is_runtime_module(path, "buffer")
        or is_runtime_module(path, "typedarray")
        or is_runtime_module(path, "typedarray_view")
    )


def is_comment_or_blank(line: str) -> bool:
    stripped = line.strip()
    return not stripped or stripped.startswith("//") or stripped.startswith("///")


def call_window(lines: list[str], index: int) -> str:
    """Return a small multiline window for classifying split calls."""

    start = index
    end = min(len(lines), index + 6)
    return " ".join(line.strip() for line in lines[start:end])


def window_head_len(lines: list[str], index: int) -> int:
    """Length of the window's first line as `call_window` renders it."""

    return len(lines[index].strip())


def anchored_search(
    pattern: re.Pattern[str], window: str, head_len: int
) -> re.Match[str] | None:
    """Match `pattern` in `window` only when the match STARTS on the head line.

    `call_window` exists so a call whose arguments are split across lines is
    still classified from its opening line (`CACHE.store(` … `);`). Searching
    the whole window unanchored, however, re-reports that one store from each
    of the ~6 lines ABOVE it: the `array/indexing.rs` finding was six lines
    wide and listed `continue;`, a bare `}` and two `if` lines as "raw atomic
    cache/global pointer store" (#7258). Requiring the match to begin inside
    the head line keeps the split-call coverage and reports each site once, at
    the line that actually contains the store.
    """

    match = pattern.search(window)
    if match is None or match.start() >= head_len:
        return None
    return match


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
    head_len = window_head_len(lines, index)
    atomic_store = anchored_search(RUST_ATOMIC_STORE_RE, window, head_len)
    if atomic_store and is_risky_atomic_root_store(
        atomic_store.group("target"), atomic_store.group("value")
    ):
        return "raw atomic cache/global pointer store"

    atomic_cas = anchored_search(RUST_ATOMIC_COMPARE_EXCHANGE_RE, window, head_len)
    if atomic_cas and is_risky_atomic_root_store(
        atomic_cas.group("target"), atomic_cas.group("value")
    ):
        return "raw atomic cache/global pointer CAS"

    global_index = RUST_GLOBAL_INDEX_STORE_RE.search(line)
    if global_index and is_risky_global_index_store(global_index.group("target"), window):
        return "raw cache/global pointer table store"

    if RUST_TLS_INDEX_STORE_RE.search(line) and is_risky_tls_index_store(window):
        return "raw TLS cache pointer table store"

    if is_runtime_module(path, "promise"):
        if RUST_PROMISE_FIELD_STORE_RE.search(line):
            return "raw Promise heap pointer field store"

    pointer_field = RUST_POINTER_FIELD_STORE_RE.search(line)
    if pointer_field:
        return "raw cache/global pointer field store"

    deref = RUST_DEREF_ASSIGN_RE.search(line)
    if deref and any(hint in deref.group("target") for hint in RUST_DEREF_RISK_TARGETS):
        if is_pointer_free_module(path):
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
        if is_runtime_module(path, "string") or is_pointer_free_module(path):
            return None
        if any(hint in window for hint in RUST_POINTER_FREE_COPY_HINTS):
            return None
        if any(hint in window for hint in RUST_COPY_RISK_HINTS):
            return "raw slot copy"
        if is_runtime_module(path, "array"):
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
        if is_codegen_path(path):
            if is_risky_codegen_store(line):
                reason = "raw generated heap/global store"
        else:
            reason = classify_rust_store(path, lines, index)

        if reason and not has_nearby_marker(lines, index):
            findings.append(Finding(path, index + 1, line, reason))

    return findings


# ═════════════════════════════════════════════════════════════════════════════
# #8185 — verify the CLAIMS, not the comments.
#
# A `GC_STORE_AUDIT` marker is a human claim, and until now this gate audited
# only that the claim EXISTS. #8185's finding is that a deleted write barrier
# passes every runtime probe — the remembered set is merely incomplete, so
# nothing fails until an unrelated minor collection drops a live object — and
# the only detector is a static assertion against the emitted LLVM IR. The
# passes below bind each claim class to evidence:
#
# * BARRIERED in perry-codegen → the barrier is emitted by a stem-labelled
#   emitter, and every stem is IR-verified by a per-PR lib test
#   (`crates/perry-codegen/src/expr/barrier_stem_census_tests.rs`): a
#   `cond_br` INTO `<stem>.barrier`, the `js_write_barrier_slot` call inside
#   that block, and the branch predicate walked by def-chain (so `br i1 true`
#   with the dead predicate left in place fails). This script proves the
#   BINDING: every emitter call site's stem literal appears in the Rust
#   registry and vice versa, the test module is registered so it actually
#   compiles, and every codegen BARRIERED marker sits in a file bound to a
#   census stem. The IR itself is asserted by `cargo test -p perry-codegen`
#   (lib tests run per-PR); lint has no compiler and cargo-test has no lint,
#   and both are required contexts, so the split loses nothing.
# * BARRIERED / EXTERNAL_BARRIERED in perry-runtime / perry-stdlib /
#   perry-ext-* → rustc compiles these, so there is no perry-emitted IR to
#   assert on. The claim is verified against SOURCE structure instead: from
#   the marker to the end of its enclosing function there must be a call to a
#   barrier primitive (defined under `crates/perry-runtime/src/gc/`) or to a
#   registered discharge helper, and every registered helper is itself
#   verified to reach a primitive through the call graph. Granularity is the
#   enclosing function — a function with two barriered stores and one barrier
#   call still passes — and that limit is printed, not hidden.
# * ROOT / INIT / POINTER_FREE / STACK → human-audited ONLY. Counted and
#   declared unverified in every run's summary, never silently trusted.
#
# Rot discipline (model: scripts/gc_rekeyed_key_tables.py): if the registry
# cannot be parsed, a floor is not met, or a scan matches nothing, the gate
# exits 2 — it never reads as a clean empty pass.

CODEGEN_TESTS_SUFFIX = "_tests.rs"

# fn name -> 0-based index of the stem/block-prefix argument. Position-keyed on
# purpose: the stem is the SITE'S IDENTITY (#8189 added it precisely so one
# site's guard cannot satisfy another site's assertion), so an emitter call
# whose stem this table cannot resolve to a string literal is a hard failure,
# not a skip. If a signature changes, this table must change with it — the
# resolver fails loudly on a non-literal either way.
STEM_EMITTER_ARG_INDEX = {
    "emit_write_barrier_slot_generation_tested": 5,
    "emit_write_barrier_slot_value_and_generation_tested": 5,
    "emit_jsvalue_slot_store_pointer_tested": 11,
    "emit_guarded_inbounds_array_store": 4,
}

# Emitter wrappers that forward a caller-supplied stem: their INTERNAL emitter
# call passes an identifier, and the stem literal lives at THEIR call sites
# (which the census scans through the same table above).
STEM_FORWARDERS = {"emit_guarded_inbounds_array_store"}

STEM_REGISTRY_PATH = "crates/perry-codegen/src/expr/barrier_stem_census_tests.rs"
STEM_REGISTRY_HEADER = "VERIFIED_BARRIER_STEMS"
STEM_REGISTRY_ENTRY_RE = re.compile(
    r'\(\s*"(?P<stem>[a-z0-9_.]+)"\s*,\s*StemKind::(?P<kind>[A-Za-z]+)\s*\)'
)
STEM_TEST_MOD_DECL = "mod barrier_stem_census_tests;"
STEM_TEST_MOD_HOST = "crates/perry-codegen/src/expr/mod.rs"

# Census floor at landing time: 8 literal-stem call sites over 5 stems. A scan
# that suddenly matches fewer than this has rotted, not improved.
MIN_STEM_CALL_SITES = 6
MIN_STEMS = 4

# file -> (stem that covers its BARRIERED markers, exact marker count).
# A BARRIERED marker in any OTHER codegen file has no IR witness and fails;
# a count drift in a bound file forces this table (and the witness) to be
# looked at, not walked past. "*" covers the shared emitter file, whose
# barrier arm every census stem exercises.
CODEGEN_BARRIERED_BINDINGS = {
    # Three markers since #9237: the two shared slot-store emitters, and
    # `emit_scalar_aware_store_gated_on_pointerness`, whose unconditional slot
    # write is discharged by its caller's stem-labelled barrier — for its only
    # caller (`lower_index_set_fast`) that is `idxset.inbounds`, a stem already
    # registered in `VERIFIED_BARRIER_STEMS` with a live IR witness, so the new
    # claim brings no new obligation of its own.
    "crates/perry-codegen/src/expr/write_barrier.rs": ("*", 3),
    # Two markers: the original generation-tested push store and, since #8872,
    # the unconditional element store inside `emit_dynamic_pointer_push_store`,
    # which the same `apush`-stem caller barriers after its layout bookkeeping.
    "crates/perry-codegen/src/expr/array_push.rs": ("apush", 2),
}

RUNTIME_MARKER_RE = re.compile(r"GC_STORE_AUDIT\((BARRIERED|EXTERNAL_BARRIERED)\)")

# Barrier primitives: functions DEFINED under crates/perry-runtime/src/gc/
# whose name matches this. They are the collector's own store/remember API —
# the ground truth every runtime-side BARRIERED claim must reach.
GROUND_BARRIER_NAME_RE = re.compile(
    r"^(?:runtime_write_barrier\w*|runtime_store_\w+|js_write_barrier\w*"
    r"|replay_old_parent_slot_range_barriers)$"
)
MIN_GROUND_FNS = 8

# Discharge helpers OUTSIDE gc/: a runtime BARRIERED marker may point at one
# of these instead of a primitive. Every entry is chain-verified each run —
# the helper must exist and its call graph must reach a primitive within
# DISCHARGE_MAX_DEPTH hops — so deleting the barrier INSIDE a helper turns
# every marker that leans on it red. An entry that stops existing fails too.
RUNTIME_DISCHARGE_HELPERS = {
    "note_array_slot": "array/header.rs: store + layout note + slot barrier",
    "note_array_slot_layout_only": (
        "array/header.rs: layout note + born-old barrier (fresh/suppressed sites)"
    ),
    "store_array_slot": "array/header.rs: canonicalize + runtime_store_jsvalue_slot",
    "store_array_slot_resolved": (
        "array/header_gc_slots.rs: resolved-head store + layout note + runtime_write_barrier_slot"
    ),
    "rebuild_array_layout": "array/header.rs: post-hoc bulk funnel; replays slot barriers",
    "rebuild_array_layout_exact": "array/header.rs: exact rebuild after bulk copy",
    "rebuild_array_layout_from_slots": "object/gc_slots.rs: rebuild from slot table",
    "replay_array_growth_write_barriers": (
        "array/header.rs: replays the copied prefix's barriers after js_array_grow"
    ),
    "store_object_field_slot": "object/mod.rs: object field store via runtime_store",
    "store_object_field_slot_layout_deferred": (
        "object/mod.rs: JSON-parser field store; layout settled at finalize (#7630)"
    ),
    "rebuild_object_field_layout": (
        "object/mod.rs: fresh-descriptor funnel; replays field barriers"
    ),
    "note_closure_capture_slot": "closure/alloc.rs: capture-slot note + barrier",
    "rebuild_closure_layout_and_barriers": (
        "closure/dynamic_props.rs: clone/rebind funnel; replays capture barriers"
    ),
    "store_thread_array_slot": "thread.rs: deserialize funnel over store_array_slot",
    "store_thread_object_field": (
        "thread.rs: deserialize funnel over store_object_field_slot"
    ),
}
DISCHARGE_MAX_DEPTH = 4
MIN_RUNTIME_BARRIER_MARKERS = 60

FN_DEF_RE = re.compile(
    r"^\s*(?:pub(?:\([^)]*\))?\s+)?(?:default\s+|const\s+|async\s+|unsafe\s+|extern\s+\"[^\"]*\"\s+)*fn\s+([A-Za-z0-9_]+)"
)
IDENT_RE = re.compile(r"^[A-Za-z_][A-Za-z0-9_]*$")


class VerificationRot(Exception):
    """The verifier itself cannot see its subject — exit 2, never a clean pass."""


def strip_rust_comments(text: str, keep_strings: bool) -> str:
    """Line-preserving comment stripper with a real string/char state machine.

    `keep_strings=True` (stem extraction) keeps string literals verbatim;
    `False` (call detection, brace counting) blanks them, because this tree is
    full of `"{"` and `"foo("` inside emitted-IR format strings.
    """
    out: list[str] = []
    in_block = False
    for line in text.splitlines():
        result: list[str] = []
        i = 0
        n = len(line)
        while i < n:
            ch = line[i]
            nxt = line[i + 1] if i + 1 < n else ""
            if in_block:
                if ch == "*" and nxt == "/":
                    in_block = False
                    i += 2
                    continue
                i += 1
                continue
            if ch == "/" and nxt == "/":
                break
            if ch == "/" and nxt == "*":
                in_block = True
                i += 2
                continue
            if ch == '"':
                j = i + 1
                while j < n:
                    if line[j] == "\\":
                        j += 2
                        continue
                    if line[j] == '"':
                        break
                    j += 1
                literal = line[i : min(j + 1, n)]
                result.append(literal if keep_strings else '""')
                i = min(j + 1, n)
                continue
            if ch == "'":
                # char literal or lifetime; consume conservatively
                if nxt == "\\" and i + 3 < n and line[i + 3] == "'":
                    result.append("''" if not keep_strings else line[i : i + 4])
                    i += 4
                    continue
                if i + 2 < n and line[i + 2] == "'":
                    result.append("''" if not keep_strings else line[i : i + 3])
                    i += 3
                    continue
                result.append(ch)
                i += 1
                continue
            result.append(ch)
            i += 1
        out.append("".join(result))
    return "\n".join(out)


def rust_function_spans(stripped: str) -> list[tuple[str, int, int]]:
    """(name, first_line_idx, last_line_idx) per fn, 0-based, brace-counted.

    Same approach as scripts/gc_runtime_root_holders.py::function_bodies; a
    body that over-runs by a brace only widens the search window, which errs
    toward accepting — the direction the registry floors compensate for.
    """
    lines = stripped.splitlines()
    spans: list[tuple[str, int, int]] = []
    index = 0
    while index < len(lines):
        match = FN_DEF_RE.match(lines[index])
        if not match:
            index += 1
            continue
        name = match.group(1)
        start = index
        depth = 0
        started = False
        while index < len(lines):
            line = lines[index]
            depth += line.count("{") - line.count("}")
            if "{" in line:
                started = True
            index += 1
            if started and depth <= 0:
                break
            # Signature-only lines before the body brace: a declaration ending
            # in `;` (trait method) has no body.
            if not started and ";" in line:
                break
        spans.append((name, start, index - 1))
    return spans


def enclosing_fn(spans: list[tuple[str, int, int]], line_idx: int) -> tuple[str, int, int] | None:
    best: tuple[str, int, int] | None = None
    for name, start, end in spans:
        if start <= line_idx <= end and (best is None or start >= best[1]):
            best = (name, start, end)
    return best


def split_top_level_args(arg_text: str) -> list[str]:
    args: list[str] = []
    depth = 0
    current: list[str] = []
    i = 0
    n = len(arg_text)
    while i < n:
        ch = arg_text[i]
        if ch == '"':
            j = i + 1
            while j < n:
                if arg_text[j] == "\\":
                    j += 2
                    continue
                if arg_text[j] == '"':
                    break
                j += 1
            current.append(arg_text[i : min(j + 1, n)])
            i = min(j + 1, n)
            continue
        if ch in "([{":
            depth += 1
        elif ch in ")]}":
            depth -= 1
        if ch == "," and depth == 0:
            args.append("".join(current).strip())
            current = []
            i += 1
            continue
        current.append(ch)
        i += 1
    tail = "".join(current).strip()
    if tail:
        args.append(tail)
    return args


def extract_call_args(text: str, open_paren_pos: int) -> str | None:
    """Balanced text between the paren at `open_paren_pos` and its close."""
    depth = 0
    i = open_paren_pos
    n = len(text)
    start = open_paren_pos + 1
    while i < n:
        ch = text[i]
        if ch == '"':
            j = i + 1
            while j < n:
                if text[j] == "\\":
                    j += 2
                    continue
                if text[j] == '"':
                    break
                j += 1
            i = min(j + 1, n)
            continue
        if ch == "(":
            depth += 1
        elif ch == ")":
            depth -= 1
            if depth == 0:
                return text[start:i]
        i += 1
    return None


@dataclass
class StemSite:
    path: str
    line: int
    emitter: str
    stem: str | None  # None = forwarded inside a registered forwarder


def codegen_stem_census(files: dict[str, str]) -> tuple[list[StemSite], list[str]]:
    """Every emitter call site in perry-codegen, with its stem literal."""
    sites: list[StemSite] = []
    errors: list[str] = []
    call_res = {
        name: re.compile(r"\b" + re.escape(name) + r"\s*\(")
        for name in STEM_EMITTER_ARG_INDEX
    }
    for rel in sorted(files):
        if not rel.startswith("crates/perry-codegen/src/"):
            continue
        if rel.endswith(CODEGEN_TESTS_SUFFIX):
            continue
        text = strip_rust_comments(files[rel], keep_strings=True)
        stripped_for_spans = strip_rust_comments(files[rel], keep_strings=False)
        spans = rust_function_spans(stripped_for_spans)
        for name, call_re in call_res.items():
            arg_index = STEM_EMITTER_ARG_INDEX[name]
            for match in call_re.finditer(text):
                before = text[: match.start()]
                # Skip the definition itself.
                if re.search(r"\bfn\s+$", before):
                    continue
                line_no = before.count("\n") + 1
                open_pos = text.index("(", match.start())
                arg_text = extract_call_args(text, open_pos)
                if arg_text is None:
                    errors.append(
                        f"{rel}:{line_no}: unbalanced call to {name} — census cannot parse it"
                    )
                    continue
                args = split_top_level_args(arg_text)
                if len(args) <= arg_index:
                    errors.append(
                        f"{rel}:{line_no}: call to {name} has {len(args)} args; "
                        f"expected stem at index {arg_index} — update "
                        "STEM_EMITTER_ARG_INDEX to match the signature"
                    )
                    continue
                stem_arg = args[arg_index]
                literal = re.fullmatch(r'"([a-z0-9_.]+)"', stem_arg)
                if literal:
                    sites.append(StemSite(rel, line_no, name, literal.group(1)))
                    continue
                if IDENT_RE.match(stem_arg):
                    fn = enclosing_fn(spans, line_no - 1)
                    if fn and fn[0] in STEM_FORWARDERS:
                        sites.append(StemSite(rel, line_no, name, None))
                        continue
                errors.append(
                    f"{rel}:{line_no}: call to {name} passes a non-literal stem "
                    f"({stem_arg!r}) outside a registered forwarder — the IR census "
                    "cannot bind this site to a witness. Pass a string literal, or "
                    "register the enclosing function in STEM_FORWARDERS."
                )
    return sites, errors


def parse_stem_registry(files: dict[str, str]) -> dict[str, str]:
    """stem -> StemKind from the Rust census test file. Raises on rot."""
    text = files.get(STEM_REGISTRY_PATH)
    if text is None:
        raise VerificationRot(
            f"{STEM_REGISTRY_PATH} is missing — the IR witness registry is gone, "
            "so no codegen BARRIERED claim is verified"
        )
    if STEM_REGISTRY_HEADER not in text:
        raise VerificationRot(
            f"{STEM_REGISTRY_PATH} no longer declares {STEM_REGISTRY_HEADER}"
        )
    section = text.split(STEM_REGISTRY_HEADER, 1)[1]
    section = section.split("];", 1)[0]
    registry: dict[str, str] = {}
    for match in STEM_REGISTRY_ENTRY_RE.finditer(section):
        registry[match.group("stem")] = match.group("kind")
    if not registry:
        raise VerificationRot(
            f"{STEM_REGISTRY_HEADER} in {STEM_REGISTRY_PATH} parsed to zero entries "
            "— entry regex rot"
        )
    mod_rs = files.get(STEM_TEST_MOD_HOST, "")
    if STEM_TEST_MOD_DECL not in mod_rs:
        raise VerificationRot(
            f"{STEM_TEST_MOD_HOST} does not declare `{STEM_TEST_MOD_DECL}` — the IR "
            "witness file exists but never compiles (a dark test, see "
            "scripts/check_test_registration.py)"
        )
    return registry


def check_codegen_claims(files: dict[str, str]) -> tuple[list[str], dict]:
    """Stem census == Rust registry, and BARRIERED markers only in bound files."""
    errors: list[str] = []
    sites, census_errors = codegen_stem_census(files)
    errors.extend(census_errors)
    if len(sites) < MIN_STEM_CALL_SITES:
        raise VerificationRot(
            f"stem census matched only {len(sites)} emitter call sites "
            f"(floor {MIN_STEM_CALL_SITES}) — scanner rot, not progress"
        )
    census_stems = {site.stem for site in sites if site.stem is not None}
    if len(census_stems) < MIN_STEMS:
        raise VerificationRot(
            f"stem census resolved only {len(census_stems)} distinct stems "
            f"(floor {MIN_STEMS}) — scanner rot, not progress"
        )
    registry = parse_stem_registry(files)
    for stem in sorted(census_stems - set(registry)):
        errors.append(
            f"stem {stem!r} is emitted by codegen but has NO IR witness in "
            f"{STEM_REGISTRY_PATH} — add it to {STEM_REGISTRY_HEADER} with a probe "
            "that reaches the tier, or the barrier can be deleted without any "
            "gate noticing (#8185)"
        )
    for stem in sorted(set(registry) - census_stems):
        errors.append(
            f"IR witness registry entry {stem!r} matches no emitter call site — "
            "stale evidence; delete the registry entry (and its probe) with the "
            "site it covered"
        )

    # BARRIERED markers in codegen must sit in a file bound to a census stem.
    marker_counts: dict[str, int] = {}
    for rel in sorted(files):
        if not rel.startswith("crates/perry-codegen/src/"):
            continue
        if rel.endswith(CODEGEN_TESTS_SUFFIX):
            continue
        count = sum(
            1
            for line in files[rel].splitlines()
            if "GC_STORE_AUDIT(BARRIERED)" in line
        )
        if count:
            marker_counts[rel] = count
    for rel, count in sorted(marker_counts.items()):
        binding = CODEGEN_BARRIERED_BINDINGS.get(rel)
        if binding is None:
            errors.append(
                f"{rel}: {count} GC_STORE_AUDIT(BARRIERED) marker(s) in a codegen "
                "file with no IR-witness binding — a claim here is exactly the "
                "trusted-comment #8185 exists to end. Route the store through a "
                "stem-labelled emitter and bind the file in "
                "CODEGEN_BARRIERED_BINDINGS."
            )
            continue
        stem, expected = binding
        if stem != "*" and stem not in census_stems:
            errors.append(
                f"{rel}: bound to stem {stem!r} which is not in the census — "
                "stale binding"
            )
        if count != expected:
            errors.append(
                f"{rel}: {count} BARRIERED marker(s) but the binding pins {expected} "
                "— a new claim needs a new witness (or the count updated with the "
                "reviewed diff)"
            )
    for rel in sorted(set(CODEGEN_BARRIERED_BINDINGS) - set(marker_counts)):
        errors.append(
            f"CODEGEN_BARRIERED_BINDINGS entry {rel} matches no BARRIERED marker — "
            "stale binding; delete it with the markers it covered"
        )
    summary = {
        "stem_call_sites": len(sites),
        "stems": sorted(census_stems),
        "registry_kinds": registry,
        "bound_codegen_barriered_markers": sum(marker_counts.values()),
    }
    return errors, summary


def runtime_discharge_index(
    files: dict[str, str],
) -> tuple[set[str], dict[str, str]]:
    """(ground primitive names, fn name -> stripped body for the whole tree)."""
    ground: set[str] = set()
    bodies: dict[str, str] = {}
    for rel in sorted(files):
        if rel.startswith("crates/perry-codegen/"):
            continue
        stripped = strip_rust_comments(files[rel], keep_strings=False)
        lines = stripped.splitlines()
        for name, start, end in rust_function_spans(stripped):
            body = "\n".join(lines[start : end + 1])
            bodies[name] = bodies.get(name, "") + "\n" + body
            if rel.startswith("crates/perry-runtime/src/gc/") and GROUND_BARRIER_NAME_RE.match(
                name
            ):
                ground.add(name)
    if len(ground) < MIN_GROUND_FNS:
        raise VerificationRot(
            f"only {len(ground)} barrier primitives found under "
            f"crates/perry-runtime/src/gc/ (floor {MIN_GROUND_FNS}) — the ground-"
            "truth scan has rotted"
        )
    return ground, bodies


def helper_reaches_ground(
    helper: str, ground: set[str], bodies: dict[str, str]
) -> bool:
    seen = {helper}
    frontier = [helper]
    for _ in range(DISCHARGE_MAX_DEPTH):
        next_frontier: list[str] = []
        for name in frontier:
            body = bodies.get(name)
            if body is None:
                continue
            for callee in re.findall(r"\b([A-Za-z_][A-Za-z0-9_]*)\s*\(", body):
                if callee in ground:
                    return True
                if callee in bodies and callee not in seen:
                    seen.add(callee)
                    next_frontier.append(callee)
        frontier = next_frontier
    return False


def check_runtime_claims(files: dict[str, str]) -> tuple[list[str], dict]:
    errors: list[str] = []
    ground, bodies = runtime_discharge_index(files)
    for helper, why in sorted(RUNTIME_DISCHARGE_HELPERS.items()):
        if helper not in bodies:
            errors.append(
                f"RUNTIME_DISCHARGE_HELPERS entry {helper!r} ({why}) is not defined "
                "anywhere — stale registry entry; delete it with the helper"
            )
        elif not helper_reaches_ground(helper, ground, bodies):
            errors.append(
                f"discharge helper {helper!r} no longer reaches any barrier "
                f"primitive within {DISCHARGE_MAX_DEPTH} calls — the barrier inside "
                "it was deleted or rerouted, and every marker leaning on it is now "
                "a false claim"
            )
    discharge_names = ground | set(RUNTIME_DISCHARGE_HELPERS)
    discharge_re = re.compile(
        r"\b(?:" + "|".join(sorted(re.escape(n) for n in discharge_names)) + r")\s*\("
    )
    checked = 0
    for rel in sorted(files):
        if rel.startswith("crates/perry-codegen/"):
            continue
        if rel.startswith("crates/perry-runtime/src/gc/"):
            continue  # the collector's own internals are the ground truth
        original_lines = files[rel].splitlines()
        stripped = strip_rust_comments(files[rel], keep_strings=False)
        stripped_lines = stripped.splitlines()
        spans = rust_function_spans(stripped)
        for idx, line in enumerate(original_lines):
            match = RUNTIME_MARKER_RE.search(line)
            if not match:
                continue
            checked += 1
            fn = enclosing_fn(spans, idx)
            if fn is None:
                errors.append(
                    f"{rel}:{idx + 1}: {match.group(1)} marker outside any function "
                    "— cannot verify its claim"
                )
                continue
            # Clamp the 3-line lookback to the enclosing fn: without the
            # clamp, a marker near the top of a function inherits the PREVIOUS
            # function's discharge call (caught by self-test shape V-P10).
            window = "\n".join(stripped_lines[max(fn[1], idx - 3) : fn[2] + 1])
            if not discharge_re.search(window):
                errors.append(
                    f"{rel}:{idx + 1}: {match.group(1)} marker in fn {fn[0]!r} but "
                    "no call to a barrier primitive or registered discharge helper "
                    "between the marker and the end of the function — the claim has "
                    "no evidence. Either the barrier was deleted (the #8185 shape) "
                    "or its helper needs registering in RUNTIME_DISCHARGE_HELPERS "
                    "(chain-verified) with a one-line why."
                )
    if checked < MIN_RUNTIME_BARRIER_MARKERS:
        raise VerificationRot(
            f"only {checked} runtime BARRIERED/EXTERNAL_BARRIERED markers scanned "
            f"(floor {MIN_RUNTIME_BARRIER_MARKERS}) — marker scan rot"
        )
    summary = {
        "ground_primitives": len(ground),
        "discharge_helpers": len(RUNTIME_DISCHARGE_HELPERS),
        "runtime_barriered_markers_verified": checked,
    }
    return errors, summary


def load_repo_files() -> dict[str, str]:
    files: dict[str, str] = {}
    for path in iter_scan_roots():
        files[repo_rel(path)] = path.read_text(encoding="utf-8", errors="replace")
    extra = REPO_ROOT / STEM_TEST_MOD_HOST
    for path in (extra, REPO_ROOT / STEM_REGISTRY_PATH):
        if path.is_file():
            files[repo_rel(path)] = path.read_text(encoding="utf-8", errors="replace")
    return files


def verify_claims(files: dict[str, str]) -> tuple[list[str], list[str], dict]:
    """-> (claim failures [exit 1], rot failures [exit 2], summary)."""
    errors: list[str] = []
    rot: list[str] = []
    summary: dict = {}
    try:
        codegen_errors, codegen_summary = check_codegen_claims(files)
        errors.extend(codegen_errors)
        summary["codegen"] = codegen_summary
    except VerificationRot as exc:
        rot.append(str(exc))
    try:
        runtime_errors, runtime_summary = check_runtime_claims(files)
        errors.extend(runtime_errors)
        summary["runtime"] = runtime_summary
    except VerificationRot as exc:
        rot.append(str(exc))
    return errors, rot, summary


def unverified_marker_counts(files: dict[str, str]) -> dict[str, int]:
    """Markers whose class remains human-audited only — declared, not hidden."""
    counts: dict[str, int] = {}
    unverified_re = re.compile(r"GC_STORE_AUDIT\((ROOT|INIT|POINTER_FREE|STACK)\)")
    for rel, text in files.items():
        for line in text.splitlines():
            match = unverified_re.search(line)
            if match:
                counts[match.group(1)] = counts.get(match.group(1), 0) + 1
    return counts


def run_self_tests() -> int:
    failures: list[str] = []

    def check_at(
        rel_path: str, lines: list[str], index: int, expected: str | None
    ) -> None:
        reason = classify_rust_store(REPO_ROOT / rel_path, lines, index)
        where = f"{rel_path}[{index}]"
        if expected is None:
            if reason is not None:
                failures.append(f"{where}: expected clean, got {reason!r}")
        elif reason is None or expected not in reason:
            failures.append(f"{where}: expected {expected!r}, got {reason!r}")

    def check(rel_path: str, lines: list[str], expected: str | None) -> None:
        check_at(rel_path, lines, 0, expected)

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
        "crates/perry-runtime/src/string/intern.rs",
        ["if entry.string_ptr == 0 {"],
        None,
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
    check(
        "crates/perry-runtime/src/promise/then.rs",
        ["(*promise).on_fulfilled = callback;"],
        "raw Promise heap pointer field store",
    )
    check(
        "crates/perry-runtime/src/promise/then.rs",
        ["(*promise).next = next;"],
        "raw Promise heap pointer field store",
    )

    # typedarray was split from typedarray.rs into typedarray/ — the
    # pointer-free carve-out must follow both shapes (regression guard).
    check(
        "crates/perry-runtime/src/typedarray/mod.rs",
        ["*dst.add(i) = load_at(ta, i);"],
        None,
    )
    check(
        "crates/perry-runtime/src/typedarray_view.rs",
        ["ptr::copy_nonoverlapping(src_data, dst, (count as i64 * bpe) as usize);"],
        None,
    )

    # The call window must classify a SPLIT call from its opening line, but must
    # NOT re-report that one store from the lines above it. Before #7258 the
    # unanchored window search made `scan_prototype_addr_cache_roots_mut` a
    # SIX-line finding whose first five lines (`continue;`, a bare `}`, two
    # `if`s, a `let`) contain no store at all.
    split_atomic = [
        "GLOBAL_CACHE.store(",
        "    addr,",
        "    Ordering::Relaxed,",
        ");",
    ]
    check_at(
        "crates/perry-runtime/src/object/mod.rs",
        split_atomic,
        0,
        "raw atomic cache/global pointer store",
    )
    for above in range(1, len(split_atomic)):
        check_at("crates/perry-runtime/src/object/mod.rs", split_atomic, above, None)

    proto_cache_scan = [
        "for cache in [&ARRAY_PROTO_ADDR, &OBJECT_PROTO_ADDR] {",
        "    let cached = cache.load(Ordering::Relaxed);",
        "    if cached == usize::MAX || cached == 0 {",
        "        continue;",
        "    }",
        "    let mut addr = cached;",
        "    if visitor.visit_usize_slot(&mut addr) {",
        "        cache.store(addr, Ordering::Relaxed);",
        "    }",
        "}",
    ]
    store_index = 7
    for index in range(len(proto_cache_scan)):
        check_at(
            "crates/perry-runtime/src/array/indexing.rs",
            proto_cache_scan,
            index,
            "raw atomic cache/global pointer store" if index == store_index else None,
        )

    # store_aligned/store_volatile are store emitters too.
    if not is_risky_codegen_store('.store_aligned(I64, &val, &field_ptr, 8);'):
        failures.append("codegen: store_aligned with heap dest not flagged")
    if not is_risky_codegen_store('.store(DOUBLE, &val, &elem_ptr)'):
        failures.append("codegen: plain store with heap dest not flagged")

    entry = AllowlistEntry("crates/perry-runtime/src/gc/", "*", "test", 1)
    fake = Finding(
        REPO_ROOT / "crates/perry-runtime/src/gc/tests/barrier.rs", 1, "*fields = x;", "r"
    )
    other = Finding(
        REPO_ROOT / "crates/perry-runtime/src/array/generic.rs", 1, "*fields = x;", "r"
    )
    if not entry.matches(fake):
        failures.append("allowlist: gc/ prefix entry should match gc/tests finding")
    if entry.matches(other):
        failures.append("allowlist: gc/ prefix entry must not match array finding")

    scanned_paths = {repo_rel(path) for path in iter_scan_roots()}
    for expected_path in (
        "crates/perry-runtime/src/array/alloc.rs",
        "crates/perry-runtime/src/buffer/from.rs",
        "crates/perry-runtime/src/builtins/globals.rs",
        "crates/perry-runtime/src/json_tape.rs",
        "crates/perry-runtime/src/temporal/mod.rs",
        "crates/perry-codegen/src/lower_call/new.rs",
    ):
        if expected_path not in scanned_paths:
            failures.append(f"scanner roots: missing {expected_path}")

    # ------------------------------------------------------------------
    # #8185 claim-verification shapes: every planted defect must be
    # adjudicated, and every rot shape must refuse to read as a clean pass
    # (model: scripts/gc_rekeyed_key_tables.py --self-test).
    # ------------------------------------------------------------------

    def synthetic_tree() -> dict[str, str]:
        """A minimal tree the verifier accepts with ZERO errors and NO rot."""
        call = (
            "fn lower_{n}(ctx: &mut FnCtx) {{\n"
            "    emit_write_barrier_slot_generation_tested(ctx, a, b, c, d, \"{stem}\");\n"
            "}}\n"
        )
        stems = ["apush", "s.two", "s.three", "s.four", "s.five", "apush"]
        codegen_calls = "".join(
            call.format(n=i, stem=stem) for i, stem in enumerate(stems)
        )
        registry = "".join(
            f'    ("{stem}", StemKind::GenerationTested),\n'
            for stem in sorted(set(stems))
        )
        ground = "".join(
            f"pub(crate) fn runtime_write_barrier_slot{sfx}(a: usize) {{}}\n"
            for sfx in ["", "0", "1", "2", "3", "4", "5", "6"]
        )
        helpers = "".join(
            f"pub(crate) unsafe fn {h}(a: usize) {{\n"
            "    runtime_write_barrier_slot(a);\n"
            "}\n"
            for h in RUNTIME_DISCHARGE_HELPERS
        )
        markers = "".join(
            f"unsafe fn site{i}(p: usize) {{\n"
            "    // GC_STORE_AUDIT(BARRIERED): planted\n"
            "    note_array_slot(p);\n"
            "}\n"
            for i in range(MIN_RUNTIME_BARRIER_MARKERS + 2)
        )
        return {
            "crates/perry-codegen/src/expr/write_barrier.rs": (
                "fn a() {\n"
                "    // GC_STORE_AUDIT(BARRIERED): planted one\n"
                "    // GC_STORE_AUDIT(BARRIERED): planted two\n"
                "}\n"
            ),
            "crates/perry-codegen/src/expr/array_push.rs": (
                "// GC_STORE_AUDIT(BARRIERED): planted\n"
                "// GC_STORE_AUDIT(BARRIERED): planted store\n" + codegen_calls
            ),
            STEM_REGISTRY_PATH: (
                "pub(super) const VERIFIED_BARRIER_STEMS: &[(&str, StemKind)] = &[\n"
                + registry
                + "];\n"
            ),
            STEM_TEST_MOD_HOST: "#[cfg(test)]\nmod barrier_stem_census_tests;\n",
            "crates/perry-runtime/src/gc/barrier_store.rs": ground,
            "crates/perry-runtime/src/array/header.rs": helpers,
            "crates/perry-runtime/src/map.rs": markers,
        }

    def expect_verify(
        label: str,
        tree: dict[str, str],
        want_error_substring: str | None,
        want_rot_substring: str | None = None,
    ) -> None:
        errors, rot, _ = verify_claims(tree)
        if want_rot_substring is not None:
            if not any(want_rot_substring in r for r in rot):
                failures.append(
                    f"{label}: wanted rot containing {want_rot_substring!r}, got "
                    f"rot={rot!r} errors={errors!r}"
                )
            return
        if rot:
            failures.append(f"{label}: unexpected rot {rot!r}")
            return
        if want_error_substring is None:
            if errors:
                failures.append(f"{label}: expected clean, got {errors!r}")
        elif not any(want_error_substring in e for e in errors):
            failures.append(
                f"{label}: wanted an error containing {want_error_substring!r}, "
                f"got {errors!r}"
            )

    base = synthetic_tree()
    expect_verify("V-P1 green baseline", base, None)

    t = dict(base)
    t["crates/perry-codegen/src/expr/array_push.rs"] += (
        'fn lower_x(ctx: &mut FnCtx) {\n'
        '    emit_write_barrier_slot_generation_tested(ctx, a, b, c, d, "s.rogue");\n'
        '}\n'
    )
    expect_verify("V-P2 unregistered stem", t, "has NO IR witness")

    t = dict(base)
    t[STEM_REGISTRY_PATH] = t[STEM_REGISTRY_PATH].replace(
        "];", '    ("s.stale", StemKind::GenerationTested),\n];'
    )
    expect_verify("V-P3 stale registry entry", t, "matches no emitter call site")

    t = dict(base)
    t["crates/perry-codegen/src/expr/array_push.rs"] += (
        "fn lower_y(ctx: &mut FnCtx) {\n"
        "    emit_write_barrier_slot_generation_tested(ctx, a, b, c, d, some_ident);\n"
        "}\n"
    )
    expect_verify("V-P4 non-literal stem", t, "non-literal stem")

    t = dict(base)
    del t[STEM_REGISTRY_PATH]
    expect_verify("V-P5 registry missing", t, None, "witness registry is gone")

    t = dict(base)
    t[STEM_REGISTRY_PATH] = (
        "pub(super) const VERIFIED_BARRIER_STEMS: &[(&str, StemKind)] = &[];\n"
    )
    expect_verify("V-P6 registry empty", t, None, "zero entries")

    t = dict(base)
    t[STEM_TEST_MOD_HOST] = "// no census module here\n"
    expect_verify("V-P7 witness module dark", t, None, "never compiles")

    t = dict(base)
    t["crates/perry-codegen/src/expr/rogue.rs"] = (
        "fn a() {\n    // GC_STORE_AUDIT(BARRIERED): planted rogue claim\n}\n"
    )
    expect_verify("V-P8 unbound codegen BARRIERED", t, "no IR-witness binding")

    t = dict(base)
    t["crates/perry-codegen/src/expr/write_barrier.rs"] += (
        "fn b() {\n    // GC_STORE_AUDIT(BARRIERED): planted third\n}\n"
    )
    expect_verify("V-P9 bound-file count drift", t, "the binding pins")

    t = dict(base)
    t["crates/perry-runtime/src/map.rs"] += (
        "unsafe fn rogue_site(p: usize) {\n"
        "    // GC_STORE_AUDIT(BARRIERED): planted, barrier deleted\n"
        "    let _ = p;\n"
        "}\n"
    )
    expect_verify("V-P10 runtime marker without discharge", t, "no call to a barrier")

    t = dict(base)
    t["crates/perry-runtime/src/array/header.rs"] = t[
        "crates/perry-runtime/src/array/header.rs"
    ].replace("unsafe fn note_array_slot(", "unsafe fn note_array_slot_renamed(", 1)
    expect_verify("V-P11 helper undefined", t, "not defined")

    t = dict(base)
    t["crates/perry-runtime/src/array/header.rs"] = t[
        "crates/perry-runtime/src/array/header.rs"
    ].replace(
        "unsafe fn note_array_slot(a: usize) {\n    runtime_write_barrier_slot(a);\n}",
        "unsafe fn note_array_slot(a: usize) {\n    let _ = a;\n}",
        1,
    )
    expect_verify(
        "V-P12 helper lost its barrier", t, "no longer reaches any barrier primitive"
    )

    t = dict(base)
    t["crates/perry-codegen/src/expr/array_push.rs"] = (
        "// GC_STORE_AUDIT(BARRIERED): planted\n"
        'fn lower_0(ctx: &mut FnCtx) {\n'
        '    emit_write_barrier_slot_generation_tested(ctx, a, b, c, d, "apush");\n'
        "}\n"
    )
    expect_verify("V-P13 census below floor", t, None, "scanner rot")

    t = dict(base)
    t["crates/perry-runtime/src/map.rs"] = (
        "unsafe fn site0(p: usize) {\n"
        "    // GC_STORE_AUDIT(BARRIERED): planted\n"
        "    note_array_slot(p);\n"
        "}\n"
    )
    expect_verify("V-P14 marker scan below floor", t, None, "marker scan rot")

    t = dict(base)
    t["crates/perry-runtime/src/gc/barrier_store.rs"] = (
        "pub(crate) fn runtime_write_barrier_slot(a: usize) {}\n"
    )
    expect_verify("V-P15 ground floor", t, None, "ground-truth scan has rotted")

    # The REAL registry must parse and match the REAL census — the gate run
    # asserts this too, but a self-test that never looks at the tree could go
    # green while the parser rots against real formatting.
    try:
        real_registry = parse_stem_registry(load_repo_files())
        if len(real_registry) < MIN_STEMS:
            failures.append(
                f"real registry parsed to only {len(real_registry)} stems"
            )
    except VerificationRot as exc:
        failures.append(f"real registry failed to parse: {exc}")


    if failures:
        print("GC store-site inventory self-test failed:")
        for failure in failures:
            print(f"  {failure}")
        return 1

    print("GC store-site inventory self-test passed.")
    return 0


def collect_inventory() -> tuple[list[Finding], int, int]:
    findings: list[Finding] = []
    marker_count = 0
    seen: set[Path] = set()
    for path in iter_scan_roots():
        if path in seen:
            continue
        seen.add(path)
        try:
            marker_count += sum(
                1
                for line in path.read_text(encoding="utf-8", errors="replace").splitlines()
                if MARKER_RE.search(line)
            )
        except OSError:
            pass
        findings.extend(scan_file(path))
    return findings, len(seen), marker_count


def write_inventory_json(
    path: Path,
    findings: list[Finding],
    files_scanned: int,
    marker_count: int,
    claim_errors: list[str] | None = None,
    rot_errors: list[str] | None = None,
    verify_summary: dict | None = None,
    unverified: dict[str, int] | None = None,
) -> None:
    claim_errors = claim_errors or []
    rot_errors = rot_errors or []
    failed = bool(findings or claim_errors or rot_errors)
    packet = {
        "schema_version": 2,
        "status": "fail" if failed else "pass",
        "errors": [finding.render() for finding in findings]
        + claim_errors
        + rot_errors,
        "claim_verification": {
            "claim_errors": claim_errors,
            "rot_errors": rot_errors,
            "summary": verify_summary or {},
            "unverified_marker_classes": unverified or {},
        },
        "summary": {
            "files_scanned": files_scanned,
            "audited_sites": marker_count,
            "unaudited_sites": len(findings),
        },
        "unaudited_sites": [
            {
                "path": str(finding.path.relative_to(REPO_ROOT)),
                "line": finding.line_no,
                "reason": finding.reason,
                "text": finding.text.strip(),
            }
            for finding in findings
        ],
    }
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(packet, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def main(argv: list[str] | None = None) -> int:
    argv = sys.argv[1:] if argv is None else argv
    if argv == ["--self-test"]:
        return run_self_tests()

    parser = argparse.ArgumentParser()
    parser.add_argument("--self-test", action="store_true")
    parser.add_argument("--json-out", type=Path)
    parser.add_argument("--gate", action="store_true")
    parser.add_argument("--allowlist", type=Path, default=DEFAULT_ALLOWLIST)
    args = parser.parse_args(argv)
    if args.self_test:
        return run_self_tests()

    entries = load_allowlist(args.allowlist)
    findings, files_scanned, marker_count = collect_inventory()
    findings, suppressed = apply_allowlist(findings, entries)

    files = load_repo_files()
    claim_errors, rot_errors, verify_summary = verify_claims(files)
    unverified = unverified_marker_counts(files)

    if args.json_out:
        write_inventory_json(
            args.json_out,
            findings,
            files_scanned,
            marker_count,
            claim_errors,
            rot_errors,
            verify_summary,
            unverified,
        )

    for entry in entries:
        if entry.hits == 0:
            print(
                f"warning: unused allowlist entry "
                f"{args.allowlist.name}:{entry.source_line} ({entry.path_prefix})"
            )

    status = 0
    if findings:
        print("GC store-site inventory failed; add nearby GC_STORE_AUDIT markers:")
        for finding in findings:
            print(f"  {finding.render()}")
        print(
            "\nAccepted marker form: "
            "// GC_STORE_AUDIT(BARRIERED): reason, with class one of "
            + ", ".join(sorted(AUDIT_CLASSES))
            + "\nOr add a justified entry to scripts/gc_store_site_allowlist.txt."
        )
        status = 1

    if claim_errors:
        print("GC store-site CLAIM verification failed (#8185):")
        for error in claim_errors:
            print(f"  {error}")
        status = 1

    if rot_errors:
        print("GC store-site claim verifier cannot see its subject (exit 2):")
        for error in rot_errors:
            print(f"  {error}")
        return 2

    if status:
        return status

    codegen = verify_summary.get("codegen", {})
    runtime = verify_summary.get("runtime", {})
    unverified_note = (
        ", ".join(f"{k}={v}" for k, v in sorted(unverified.items())) or "none"
    )
    print(
        f"GC store-site inventory passed ({files_scanned} files scanned, "
        f"{marker_count} audited sites, {suppressed} allowlisted)."
    )
    print(
        "Claim verification: "
        f"{codegen.get('stem_call_sites', 0)} codegen barrier sites bound to "
        f"{len(codegen.get('stems', []))} IR-witnessed stems "
        f"({', '.join(codegen.get('stems', []))}); "
        f"{runtime.get('runtime_barriered_markers_verified', 0)} runtime BARRIERED/"
        "EXTERNAL_BARRIERED markers source-verified against "
        f"{runtime.get('ground_primitives', 0)} barrier primitives + "
        f"{runtime.get('discharge_helpers', 0)} chain-verified helpers "
        "(granularity: enclosing function)."
    )
    print(
        f"UNVERIFIED (human-audited only, by class): {unverified_note}. "
        "These classes have no machine-checkable evidence; do not cite this "
        "gate as verifying them."
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
