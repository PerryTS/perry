#!/usr/bin/env python3
"""#8067/#8113/#8047 exact shape-header census and sabotage tests.

#8047 deleted the final `ObjectHeader::keys_array` compatibility mirror. The
name remains census-tracked so any stale/reintroduced raw member read is red;
`assert_header_fields` also pins the exact declared field list.
"""

from __future__ import annotations

import argparse
import json
import re
from collections import Counter
from collections.abc import Callable
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
BASELINE_PATH = ROOT / "scripts" / "shape_descriptor_census_baseline.json"
FIELDS = ("keys_array",)
# The exact public `ObjectHeader` field list, in order. #8047 took it from four
# fields (24 bytes LP64) to three (16). Changing it is an ABI change with a published
# crates.io mirror (`perry-ffi`), so it must be a deliberate edit here too.
OBJECT_HEADER_FIELDS = ("class_id", "parent_class_id", "meta")
RAW_STRING_START = re.compile(r'(?:br|r)(?P<hashes>#{0,255})"')
RUST_SPECIAL = re.compile(
    r"//|/\*|(?:b)?'(?:\\(?:x[0-9A-Fa-f]{2}|u\{[0-9A-Fa-f_]+\}|.)|[^'\\\n])'|(?:br|r)#{0,255}\"|(?:b|c)?\""
)
BLOCK_COMMENT_MARK = re.compile(r'/\*|\*/')
QUOTED_STRING_TAIL = re.compile(r'(?:\\.|[^"\\])*"', re.DOTALL)


class CensusError(RuntimeError):
    pass


def rust_sources() -> dict[str, str]:
    return {
        path.relative_to(ROOT).as_posix(): path.read_text(encoding="utf-8")
        for path in sorted((ROOT / "crates").rglob("*.rs"))
    }


def _rust_without_comments_and_literals(source: str, preserve_offsets: bool) -> str:
    """Blank comments/literals, optionally preserving every source offset."""

    def blank_span(span: str) -> str:
        if preserve_offsets:
            return "".join("\n" if char == "\n" else " " for char in span)
        return " " + "\n" * span.count("\n")

    chunks: list[str] = []
    pos = 0
    while match := RUST_SPECIAL.search(source, pos):
        chunks.append(source[pos : match.start()])
        lexeme = match.group()
        end = match.end()
        if lexeme == "//":
            newline = source.find("\n", end)
            if newline < 0:
                end = len(source)
            else:
                end = newline + 1
            chunks.append(blank_span(source[match.start() : end]))
            pos = end
            continue
        if lexeme == "/*":
            depth = 1
            cursor = end
            while depth and (mark := BLOCK_COMMENT_MARK.search(source, cursor)):
                depth += 1 if mark.group() == "/*" else -1
                cursor = mark.end()
            end = cursor if depth == 0 else len(source)
        else:
            raw = RAW_STRING_START.fullmatch(lexeme)
            if lexeme.endswith("'"):
                end = match.end()
            elif raw:
                terminator = '"' + raw.group("hashes")
                close = source.find(terminator, end)
                end = len(source) if close < 0 else close + len(terminator)
            else:
                tail = QUOTED_STRING_TAIL.match(source, end)
                end = len(source) if tail is None else tail.end()
        chunks.append(blank_span(source[match.start() : end]))
        pos = end
    chunks.append(source[pos:])
    return "".join(chunks)


def strip_rust_comments_and_literals(source: str) -> str:
    """Blank comments/string literals while preserving code and newlines."""

    return _rust_without_comments_and_literals(source, preserve_offsets=False)


def blank_rust_comments_and_literals(source: str) -> str:
    """Blank comments/literals while preserving every source-string offset."""

    return _rust_without_comments_and_literals(source, preserve_offsets=True)


def stripped_sources(sources: dict[str, str]) -> dict[str, str]:
    return {path: strip_rust_comments_and_literals(text) for path, text in sources.items()}


def run_literal_lexer_selftest() -> None:
    fixture = """unsafe fn quote_fixture(o: *mut ObjectHeader) {
        let byte_quote = b'"';
        (*o).keys_array = core::ptr::null_mut();
        let char_quote = '"';
        let dead = "(*o).keys_array = core::ptr::null_mut();";
    }
    """
    clean = strip_rust_comments_and_literals(fixture)
    if len(re.findall(r"\.\s*keys_array\b", clean)) != 1:
        raise CensusError("literal lexer swallowed a real member between quote-char literals")
    brace_fixture = '''fn brace_fixture() {
        let string_brace = "}";
        let char_brace = '{';
        // } must not close the function
        let live_after_literal_braces = 1;
    }
    '''
    if "live_after_literal_braces" not in function_body(brace_fixture, "brace_fixture"):
        raise CensusError("raw function-body extraction counted literal/comment braces")


def normalize_line(line: str) -> str:
    return re.sub(r"\s+", " ", line.strip())


def callsite_multiset(clean: dict[str, str]) -> Counter[str]:
    sites: Counter[str] = Counter()
    for path, source in clean.items():
        for line in source.splitlines():
            normalized = normalize_line(line)
            if not normalized:
                continue
            for field in FIELDS:
                access_count = len(re.findall(rf"\.\s*{field}\b", line))
                declaration_count = len(re.findall(rf"\b{field}\s*:", line))
                if access_count:
                    sites[f"{path}|{field}|access|{normalized}"] += access_count
                if declaration_count:
                    sites[f"{path}|{field}|declaration|{normalized}"] += declaration_count
    return sites


def codegen_header_size_multiset(clean: dict[str, str]) -> Counter[str]:
    sites: Counter[str] = Counter()
    prefix = "crates/perry-codegen/src/"
    for path, source in clean.items():
        if not path.startswith(prefix):
            continue
        for line in source.splitlines():
            count = len(re.findall(r"\bobject_header_size_bytes\b", line))
            if count:
                sites[f"{path}|{normalize_line(line)}"] += count
    return sites


def observed_census(sources: dict[str, str]) -> dict[str, object]:
    candidates = {
        path: text
        for path, text in sources.items()
        if any(field in text for field in FIELDS)
        or "object_header_size_bytes" in text
    }
    clean = stripped_sources(candidates)
    raw_sites = callsite_multiset(clean)
    codegen_sites = codegen_header_size_multiset(clean)
    totals = {field: 0 for field in FIELDS}
    files: set[str] = set()
    for identity, count in raw_sites.items():
        path, field, _, _ = identity.split("|", 3)
        totals[field] += count
        files.add(path)
    return {
        "raw_member_callsite_multiset": dict(sorted(raw_sites.items())),
        "codegen_object_header_size_callsite_multiset": dict(sorted(codegen_sites.items())),
        "summary": {
            "raw_member_sites": totals,
            "raw_member_files": len(files),
            "codegen_object_header_size_sites": sum(codegen_sites.values()),
        },
    }


def function_body(source: str, name: str) -> str:
    blanked = blank_rust_comments_and_literals(source)
    match = re.search(rf"\bfn\s+{re.escape(name)}\b", blanked)
    if not match:
        raise CensusError(f"missing function body: {name}")
    start = blanked.find("{", match.end())
    if start < 0:
        raise CensusError(f"missing opening brace: {name}")
    depth = 0
    for i in range(start, len(blanked)):
        if blanked[i] == "{":
            depth += 1
        elif blanked[i] == "}":
            depth -= 1
            if depth == 0:
                return source[start + 1 : i]
    raise CensusError(f"missing closing brace: {name}")


def require_match(body: str, pattern: str, what: str) -> str:
    """Like require_code, but returns the first capture group.

    Used where the census must reason about a VALUE (a constant's magnitude),
    not merely assert that a line exists. Asserting a value relationship
    survives an encoding change; asserting an instruction does not.
    """
    m = re.search(pattern, body)
    if m is None:
        raise CensusError(f"missing: {what}")
    return m.group(1)


def require_code(source: str, pattern: str, label: str) -> None:
    if not re.search(pattern, source, re.MULTILINE | re.DOTALL):
        raise CensusError(f"shape descriptor authority surface missing: {label}")


def assert_before(body: str, first: str, second: str, label: str) -> None:
    first_at = body.find(first)
    second_at = body.find(second)
    if first_at < 0 or second_at < 0 or first_at >= second_at:
        raise CensusError(f"shape descriptor authority ordering failed: {label}")


def assert_rule3_licenses_the_kind_fence_removal(rule3: str, gc_types: str) -> None:
    """The licence for the generic read PIC having no GC-kind fence.

    The emitted read used to test `obj_type == GC_TYPE_OBJECT` before the
    ShapeId compare, so a non-object cell whose payload `+4` word happened to
    land in `[SHAPE_ID_BASE, SHAPE_ID_END)` could not be read as a shaped
    object. #10828 closed rule 3 -- *no pointer-tagged non-object cell holds a
    ShapeId-range value at payload +4* -- so the compare now carries that
    proof itself and #10843 removed the load.

    What must therefore hold is not a branch but a PROPERTY of the object
    model: `object/shape_rule3.rs` classifies every GC kind's `+4` word, and
    the fence is removable exactly while no row is `Rule3Word::RangeReachable`
    (the verdict meaning "can reach the range AND is pointer-tagged"). Assert
    that, which goes red the moment a new kind reopens the hole -- precisely
    the condition under which the removed load would be needed again.
    """
    table = require_match(
        rule3,
        r"(?s)const\s+RULE3_KINDS\s*:[^=]*=\s*&\[(.*?)\n\];",
        "shape_rule3 declares the RULE3_KINDS verdict table",
    )
    verdicts = re.findall(r"(?s)gc::(GC_TYPE_\w+)\s*,.*?Rule3Word::(\w+)", table)
    # Liveness BEFORE the emptiness test. An emptied or truncated table has no
    # `RangeReachable` row either, so the check below would pass for free --
    # #7024/#7025's failure mode, a green gate whose subject never ran. The
    # table only licenses anything while it still classifies every kind the
    # collector knows about, so re-derive that set from `gc/types.rs` instead
    # of trusting a count. A kind's discriminant is a decimal or hex literal or
    # a codegen ABI constant; `GC_TYPE_MAX` is the id bound, not a kind.
    declared = {
        name
        for name, _ in re.findall(
            r"pub\s+const\s+(GC_TYPE_\w+)\s*:\s*u8\s*=\s*"
            r"(0x[0-9A-Fa-f]+|\d+|crate::codegen_abi::\w+)\s*;",
            gc_types,
        )
    } - {"GC_TYPE_MAX"}
    if not declared:
        raise CensusError("gc/types.rs declares no GC kinds: rule-3 coverage is unknowable")
    classified = {kind for kind, _ in verdicts}
    unclassified = sorted(declared - classified)
    if unclassified:
        raise CensusError(
            "rule 3 is UNPROVEN for " + ", ".join(unclassified) + ": shape_rule3.rs's "
            "RULE3_KINDS gives no verdict for these GC kinds, so 'no kind keeps the "
            "fence' is vacuous and the generic read PIC's missing GC_TYPE_OBJECT load "
            "is unlicensed"
        )
    stale = sorted(classified - declared)
    if stale:
        raise CensusError(
            "RULE3_KINDS names GC kinds that no longer exist: " + ", ".join(stale)
        )
    still_fenced = sorted(kind for kind, verdict in verdicts if verdict == "RangeReachable")
    if still_fenced:
        raise CensusError(
            "rule 3 is REOPENED by " + ", ".join(still_fenced) + ": shape_rule3.rs "
            "classifies the payload +4 word Rule3Word::RangeReachable, so a "
            "pointer-tagged cell of that kind can carry a live ShapeId and a shape "
            "compare alone no longer proves the receiver is a GC_TYPE_OBJECT. The "
            "generic read PIC dropped its GC-kind fence on the promise that no kind "
            "can (#10828/#10843) -- restore the fence, or bound the kind's +4 word "
            "below SHAPE_ID_BASE"
        )
    # "No row is RangeReachable" must not be satisfiable by deleting the
    # concept, nor by deleting the runtime test that keeps the table honest at
    # `cargo test` time. Both are part of the same proof.
    require_code(
        rule3,
        r"RangeReachable\s*\(\s*&\s*'static\s+str\s*\)",
        "shape_rule3 keeps the RangeReachable verdict available to classify a kind honestly",
    )
    require_code(
        rule3,
        r"fn\s+rule3_no_kind_still_requires_the_gc_kind_fence\b",
        "shape_rule3's fence-keeping-set-is-empty test",
    )



# The runtime's receiver layout replaced per-site read/store cache state.
# Pin the live authority and refusal edges, not the deleted PIC writers.
DISPATCH_CHECKS = (
    ("crates/perry-runtime/src/object/shapes.rs", "scan_shape_table_keys_mut",
     r"scan_shape_table_young\(visitor,\s*table,\s*&mut\s+inner,\s*rewrite_phase\)",
     "young metadata scan remains reachable from the full scanner"),
    ("crates/perry-runtime/src/object/shapes.rs", "scan_shape_table_young",
     r"scan_shape_keys_address\(visitor,\s*table,\s*inner,\s*rewrite_phase,\s*keys\)",
     "young metadata scan reaches the same carrier-gated rewrite"),
    ("crates/perry-runtime/src/object/native_call_method.rs", "native_call_method_tower",
     r"class_holder::try_class_holder_fast_dispatch\(object,\s*&key,\s*args_ptr,\s*args_len\)",
     "method tower delegates to the shape-based holder walk"),
    ("crates/perry-runtime/src/object/native_call_method/class_holder.rs", "chain_method",
     r"chain_method_admitting\(start,\s*key,\s*\|_\|\s*true\)",
     "method wrapper delegates to the descriptor walk"),
    ("crates/perry-runtime/src/object/native_call_method/class_holder.rs", "chain_method_admitting",
     r"crate::object::shapes::object_shape_record\(holder\)",
     "method holder resolves its current descriptor"),
    ("crates/perry-runtime/src/object/native_call_method/class_holder.rs", "chain_method_admitting",
     r"key_position\(keys,\s*record\.logical_key_count\(\),\s*key\)",
     "method key lookup is bounded by descriptor count"),
    ("crates/perry-runtime/src/object/layout_probe.rs", "probe_get_at",
     r"let\s+word\s*=\s*\(\*obj\)\.parent_class_id;\s*let\s+r\s*=\s*record_of\(dir,\s*LayoutWord\(word\)\);",
     "read probe resolves the receiver's current layout word"),
    ("crates/perry-runtime/src/object/layout_probe.rs", "probe_get_at",
     r"if\s+r\.is_null\(\)\s*\{\s*return\s+array_named_get::<ABSENT>\(\s*dir,\s*obj,\s*key_bits,\s*site_key_hash::<LAZY_HASH>\(key_bits,\s*hash\),\s*\);",
     "nonordinary read routes to the checked array path"),
    ("crates/perry-runtime/src/object/layout_probe.rs", "perry_probe_set",
     r"let\s+word\s*=\s*\(\*obj\)\.parent_class_id;\s*let\s+r\s*=\s*record_of\(dir,\s*LayoutWord\(word\)\);\s*if\s+r\.is_null\(\)\s*\|\|\s*\(\*r\)\.object_kind\(\)\s*!=\s*super::ShapeObjectKind::Ordinary\s*\{\s*return\s+0;",
     "store probe rejects a missing or nonordinary receiver record"),
    ("crates/perry-runtime/src/object/layout_probe.rs", "perry_probe_set",
     r"key_storage_slot\(keys,\s*pos\)",
     "store slot comes from the receiver's descriptor entry"),
    ("crates/perry-runtime/src/object/shape_store.rs", "dynamic_store_receiver",
     r"let\s+id\s*=\s*super::layout_desc::layout_id_of_word\(\(\*obj\)\.parent_class_id\);",
     "dynamic store derives identity from the receiver"),
    ("crates/perry-runtime/src/object/shape_store.rs", "dynamic_store_receiver",
     r"let\s+record\s*=\s*shapes::shape_record_of_receiver\(id\)\?;",
     "dynamic store resolves its receiver's shape record"),
    ("crates/perry-runtime/src/object/shape_store.rs", "try_dynamic_store",
     r"let\s+record\s*=\s*dynamic_store_receiver\(obj\)\?;",
     "dynamic store cannot bypass receiver admission"),
    ("crates/perry-codegen/src/expr/property_get/generic_dispatch.rs", "lower_property_site_tail",
     r'icmp_ne\(I64,\s*&answered_bits,\s*crate::nanbox::TAG_HOLE_I64\)',
     "only a non-hole probe answer enters the read merge"),
    ("crates/perry-codegen/src/expr/property_get/generic_dispatch.rs", "lower_property_site_tail",
     r'cond_br\(&served,\s*&merge_label,\s*&call_label\)',
     "declined reads enter complete property semantics"),
    ("crates/perry-codegen/src/expr/put_value_store_ic.rs", "emit_static_store_ic_site",
     r'icmp_ne\(I32,\s*&stored,\s*"0"\)',
     "only an admitted store probe succeeds"),
    ("crates/perry-codegen/src/expr/put_value_store_ic.rs", "emit_static_store_ic_site",
     r'cond_br\(&hit,\s*&merge_label,\s*&slow_label\)',
     "declined stores enter complete Set semantics"),
    ("crates/perry-codegen/src/expr/proxy_reflect.rs", "lower_put_value_dyn_ic_inline",
     r'"js_put_value_set_dynamic",\s*&\[\(DOUBLE,\s*t\),\s*\(DOUBLE,\s*k\),\s*\(DOUBLE,\s*v\),\s*\(I32,\s*strict_i32\)\]',
     "dynamic store forwards receiver/key/value/strictness"),
)


def assert_layout_dispatch_authority(sources: dict[str, str]) -> None:
    for path, name, pattern, label in DISPATCH_CHECKS:
        if path not in sources:
            raise CensusError("shape dispatch authority source missing: " + path)
        require_code(function_body(sources[path], name), pattern, label)
    probe = sources["crates/perry-runtime/src/object/layout_probe.rs"]
    body = blank_rust_comments_and_literals(function_body(probe, "probe_get"))
    if not re.fullmatch(
        r"\s*probe_get_at::<ABSENT,\s*false,\s*false>\(dir,\s*obj_biased,\s*key_bits,\s*hash,\s*std::ptr::null\(\)\)\s*", body
    ):
        raise CensusError("probe_get bypasses the stateless receiver-layout authority")
    for name, absence in (("perry_probe_get", "true"), ("perry_probe_get_binding", "false")):
        body = blank_rust_comments_and_literals(function_body(probe, name))
        if not re.fullmatch(
            r"\s*probe_get::<" + absence + r">\(dir,\s*obj_biased,\s*key_bits,\s*hash\)\s*", body
        ):
            raise CensusError(name + " bypasses the receiver-layout authority")
    for path, name, calls in (
        ("crates/perry-codegen/src/expr/property_get/generic_dispatch.rs",
         "probe_then_value", ("perry_probe_get_value", "js_object_get_field_value")),
        ("crates/perry-codegen/src/expr/property_get/generic_dispatch.rs",
         "lower_property_site_tail", ("perry_probe_get", "perry_probe_get_binding",
                                     "perry_probe_get_length", "js_object_get_field_slow")),
        ("crates/perry-codegen/src/expr/put_value_store_ic.rs",
         "emit_static_store_ic_site", ("perry_probe_set", "js_put_value_set")),
    ):
        body = function_body(sources[path], name)
        for callee in calls:
            require_code(body, '"' + callee + '"', name + " retains " + callee)


def run_layout_dispatch_sabotage(sources, expect_rejected) -> None:
    # Each live obligation has its own mutation. Mutate the matched code in
    # the named function so unrelated occurrences cannot satisfy the fixture.
    for path, name, pattern, label in DISPATCH_CHECKS:
        broken = dict(sources)
        body = function_body(broken[path], name)
        changed, count = re.subn(pattern, "CENSUS_REMOVED_AUTHORITY", body, count=1)
        if count != 1:
            raise CensusError("layout dispatch sabotage fixture missing: " + label)
        broken[path] = broken[path].replace(body, changed, 1)
        expect_rejected(label, lambda: assert_layout_dispatch_authority(broken))
    path = "crates/perry-runtime/src/object/layout_probe.rs"
    for name in ("perry_probe_get", "perry_probe_get_binding", "probe_get"):
        broken = dict(sources)
        body = function_body(broken[path], name)
        broken[path] = broken[path].replace(body, "if false { " + body + "; } 0.0", 1)
        expect_rejected(name + " dead delegation", lambda: assert_layout_dispatch_authority(broken))

def assert_authority_surfaces(sources: dict[str, str]) -> None:
    authority_paths = (
        "crates/perry-runtime/src/object/shapes.rs",
        "crates/perry-runtime/src/object/shapes_slot_list.rs",
        "crates/perry-runtime/src/object/shapes_store.rs",
        "crates/perry-runtime/src/object/shapes_store/slab.rs",
        "crates/perry-runtime/src/object/shapes_store/flat.rs",
        "crates/perry-runtime/src/object/mod.rs",
        "crates/perry-runtime/src/object/live_slots.rs",
        "crates/perry-codegen/src/lower_call/new_alloc.rs",
        "crates/perry-runtime/src/gc/layout_slot_visit.rs",
        "crates/perry-runtime/src/gc/copying_object_scan.rs",
        "crates/perry-runtime/src/object/field_set_by_name/tail.rs",
        "crates/perry-runtime/src/typed_feedback/guards.rs",
        "crates/perry-runtime/src/object/native_call_method.rs",
        "crates/perry-runtime/src/object/exotic_expando.rs",
        "crates/perry-runtime/src/object/shape_rule3.rs",
        "crates/perry-runtime/src/object/field_get_set/get_field_by_name_tail.rs",
        "crates/perry-runtime/src/proxy/put_value.rs",
        "crates/perry-runtime/src/gc/types.rs",
        "crates/perry-runtime/src/regex.rs",
        "crates/perry-runtime/src/regex/perex_construct.rs",
        "crates/perry-runtime/src/regex/instance.rs",
        "crates/perry-runtime/src/object/alloc_basic.rs",
        "crates/perry-codegen/src/expr/class_field_inline_guard.rs",
        "crates/perry-codegen/src/expr/element_shape_guard.rs",
        "crates/perry-codegen/src/expr/property_get/generic_dispatch.rs",
        "crates/perry-codegen/src/expr/proxy_reflect.rs",
    )
    missing = [path for path in authority_paths if path not in sources]
    if missing:
        raise CensusError(
            "shape descriptor authority source missing: " + ", ".join(missing)
        )
    clean = stripped_sources({path: sources[path] for path in authority_paths})
    # `shapes.rs` sits against the repo's 2000-line cap, so helpers keep being
    # split into siblings as it grows (`shapes_slot_list.rs`, and since #9706
    # the record store `shapes_store.rs`). Read them as ONE logical unit: every
    # `function_body(shapes, ...)` below then finds its target wherever it
    # currently lives, instead of silently matching nothing the next time a
    # pinned function crosses the split — #8918's exact failure mode, where a
    # census inspecting an empty body reports success.
    shapes = (
        clean["crates/perry-runtime/src/object/shapes.rs"]
        + "\n"
        + clean["crates/perry-runtime/src/object/shapes_slot_list.rs"]
        + "\n"
        + clean["crates/perry-runtime/src/object/shapes_store.rs"]
        + "\n"
        + clean["crates/perry-runtime/src/object/shapes_store/slab.rs"]
        + "\n"
        + clean["crates/perry-runtime/src/object/shapes_store/flat.rs"]
    )
    flat = clean["crates/perry-runtime/src/object/shapes_store/flat.rs"]
    require_code(flat, r"struct\s+Region\s*\{\s*base\s*:\s*\*mut\s+ShapeRecord", "one immovable descriptor Region")
    if re.search(r"\brealloc\s*\(", blank_rust_comments_and_literals(flat)):
        raise CensusError("descriptor Region must never realloc")
    object_mod = clean["crates/perry-runtime/src/object/mod.rs"]
    live_slots = clean["crates/perry-runtime/src/object/live_slots.rs"]
    codegen_alloc = clean["crates/perry-codegen/src/lower_call/new_alloc.rs"]
    layout_visit = clean["crates/perry-runtime/src/gc/layout_slot_visit.rs"]
    copying_object_scan = clean["crates/perry-runtime/src/gc/copying_object_scan.rs"]
    transition_tail = clean[
        "crates/perry-runtime/src/object/field_set_by_name/tail.rs"
    ]
    typed_guards = clean["crates/perry-runtime/src/typed_feedback/guards.rs"]
    native_call_method = clean[
        "crates/perry-runtime/src/object/native_call_method.rs"
    ]
    exotic_expando = clean["crates/perry-runtime/src/object/exotic_expando.rs"]
    rule3 = clean["crates/perry-runtime/src/object/shape_rule3.rs"]
    get_field_tail = clean[
        "crates/perry-runtime/src/object/field_get_set/get_field_by_name_tail.rs"
    ]
    put_value = clean["crates/perry-runtime/src/proxy/put_value.rs"]
    gc_types = clean["crates/perry-runtime/src/gc/types.rs"]
    regex_runtime = clean["crates/perry-runtime/src/regex.rs"]
    regex_construct = clean["crates/perry-runtime/src/regex/perex_construct.rs"]
    class_guard = clean[
        "crates/perry-codegen/src/expr/class_field_inline_guard.rs"
    ]
    element_guard = clean[
        "crates/perry-codegen/src/expr/element_shape_guard.rs"
    ]
    generic_pic = clean[
        "crates/perry-codegen/src/expr/property_get/generic_dispatch.rs"
    ]
    write_pics = clean["crates/perry-codegen/src/expr/proxy_reflect.rs"]
    # Emitted ObjectHeader offsets and fail-closed constants are represented as
    # Rust string literals, so inspect raw function bodies for these checks.
    raw_class_guard = sources[
        "crates/perry-codegen/src/expr/class_field_inline_guard.rs"
    ]
    raw_element_guard = sources[
        "crates/perry-codegen/src/expr/element_shape_guard.rs"
    ]

    for pattern, label in (
        # #9706: the by-id store is a chunked slab indexed by ShapeId. Since
        # #8112 the collector enumerates the record's `keys` word as an
        # ordinary GC slot, and a budgeted dirty scan can hold that address
        # across mutator resumptions that insert descriptors — so a record's
        # address must never move for its lifetime. Chunks are individually
        # allocated (a `Slot`: an owned pointer, or the shared all-empty chunk
        # that is never written) and never reallocated; only the directory of
        # chunk pointers grows. Putting records into one flat `Vec` (or back
        # into a rehashing bucket) moves them under the collector's feet.
        (r"slab\s*:\s*(?:std::cell::)?UnsafeCell\s*<\s*ShapeSlab\s*>", "by-id descriptor slab with stable record addresses"),
        (r"struct\s+Slot\s*<\s*T\s*>\s*\(\s*std::ptr::NonNull\s*<\s*T\s*>\s*\)", "a slab slot is one pointer to its own allocation"),
        (r"type\s+ChunkCells\s*=\s*\[\s*UnsafeCell\s*<\s*\*mut\s+ShapeRecord\s*>\s*;\s*CHUNK_LEN\s*\]", "slab chunks hold pointers to stable Region records"),
        (r"type\s+PageSlots\s*=\s*\[\s*Slot\s*<\s*ChunkCells\s*>\s*;\s*PAGE_LEN\s*\]", "slab pages hold chunk pointers, not records"),
        (r"type\s+Page\s*=\s*Slot\s*<\s*PageSlots\s*>", "slab directory entries are page pointers"),
        (r"pages\s*:\s*Vec\s*<\s*Page\s*>", "slab directory is a vector of page pointers"),
        # `keys` must stay the FIRST field of the `#[repr(C)]` record: the
        # record address IS the rewritable keys slot (`keys_slot`).
        (r"#\[repr\(C(?:,\s*align\(8\))?\)\]\s*(?:#\[[^\]]*\]\s*)*pub\(crate\)\s+struct\s+ShapeRecord\s*\{\s*(?://[^\n]*\n\s*)*pub\(super\)\s+keys\s*:\s*u64", "slab record is repr(C) with the keys word first"),
        (r"logical_key_count\s*:\s*u32", "exact logical-key fact"),
        (r"live_inline_slot_count\s*:\s*u32", "exact live-slot fact"),
        (r"semantic_generation\s*:\s*u64", "semantic transition fact"),
        (r"object_kind\s*:\s*ShapeObjectKind", "authoritative receiver-kind fact"),
        (r"\bfn\s+shape_descriptor_by_id\b", "by-id lookup"),
        (r"\bfn\s+debug_assert_object_shape_parity\b", "parity assertion"),
        # #8112 replaced the post-visit write-back callback with a rewritable
        # location: the lifted descriptor carries the address of its own BOXED
        # record, so the slot visitor writes that record and there is nothing
        # left to reconcile.
        (r"pub\(crate\)\s+record\s*:\s*usize", "authoritative descriptor record address"),
        (r"\bfn\s+keys_slot\b", "authoritative keys-edge slot"),
        # The liveness gate. Rooting the table unconditionally would make every
        # keys array ever minted immortal and turn `prune_dead_shape_keys`'s
        # "is the keys array dead?" into a question it asks of itself; rooting
        # nothing loses the keys array of a shape only OLD objects carry, which
        # a minor never enumerates.
        (r"pub\(crate\)\s+old_carrier\s*:\s*bool", "old-carrier ephemeron gate"),
        (r"pub\(crate\)\s+cache_carrier\s*:\s*bool", "cache-carrier strong metadata owner"),
        (r"\bfn\s+rotate_old_carrier_epoch_after_full_trace\b", "old-carrier gate recomputed by a full trace"),
        (r"is_dead_owner\s*\(\s*descriptor\.keys\s+as\s+usize\s*\)", "dead descriptor pruning"),
    ):
        require_code(shapes, pattern, label)

    allocator = function_body(shapes, "alloc_shape_id_from")
    require_code(allocator, r"\bcompare_exchange_weak\s*\(", "exhaustion park")
    if re.search(r"\bfetch_add\s*\(", allocator):
        raise CensusError("ShapeId allocator wraps instead of parking")
    require_code(shapes, r"\bfn\s+shape_id_exhausted_abort\b", "exhaustion fail-stop")
    public_ensure = function_body(shapes, "layout_for_keys_ensure")
    require_code(
        public_ensure,
        r"publish_shape_result\s*\(",
        "typed shape-mint errors fail stop",
    )

    rekey = function_body(shapes, "scan_shape_table_rekey_mut")
    for callee in ("scan_shape_table_keys_mut", "shapes_prototype::scan_shape_prototype_words_mut"):
        require_code(rekey, re.escape(callee) + r"\(visitor\)", "rekey delegates to " + callee)
    for scanner_name in ("scan_shape_table_keys_mut", "scan_shape_keys_address"):
        scanner = function_body(shapes, scanner_name)
        require_code(scanner, r"\bvisit_metadata_usize_slot\s*\(", "weak metadata rewrite")
        # #8112: the scanner has exactly TWO arms, and which one a descriptor takes
        # IS the liveness protocol. `visit_usize_slot` ROOTS — reserved for a shape
        # an OLD object still carries, which a minor cannot enumerate for itself.
        # `visit_metadata_usize_slot` does not root — a young carrier is traced and
        # emits its own edge, and rooting those from the table would make every
        # keys array ever minted immortal. Pin the whole two-armed expression, not
        # just the set of APIs called: a sabotage that widens the gate, or that
        # swaps the arms, has to be red.
        if not re.search(
            r"if\s+descriptor\.old_carrier\s*\|\|\s*descriptor\.cache_carrier\s*\{\s*"
            r"visitor\.visit_usize_slot\(&mut addr\)\s*\}\s*else\s*\{\s*"
            r"visitor\.visit_metadata_usize_slot\(&mut addr\)\s*\}",
            scanner,
        ):
            raise CensusError(
                "descriptor rooting is not gated on `old_carrier || cache_carrier`: "
                "the shape table either roots unconditionally (every keys array "
                "immortal) or not at all (a shape only old objects carry, or one a "
                "runtime optimization cache can reinstall, loses its keys array)"
            )
        scanner_slot_apis = set(re.findall(r"\b(visit_[A-Za-z0-9_]*slot)\s*\(", scanner))
        if scanner_slot_apis != {"visit_metadata_usize_slot", "visit_usize_slot"}:
            raise CensusError(
                "descriptor scanner slot API allowlist failed: "
                + ", ".join(sorted(scanner_slot_apis))
            )

    # The dispatcher selects either the specialized object walk or the
    # generic enumeration. The copying drain has a third enumeration. Every
    # body must emit the descriptor's keys edge without the deleted mirror.
    dispatch = function_body(layout_visit, "visit_gc_layout_slot_descriptors_inline")
    for callee in ("visit_object_layout_slots", "visit_gc_layout_slot_descriptors_generic"):
        require_code(dispatch, rf"\b{callee}\s*\(header,\s*visit\)",
                     f"layout dispatcher reaches {callee}")
    for body_name, body in (
        ("visit_gc_layout_slot_descriptors_generic",
         function_body(layout_visit, "visit_gc_layout_slot_descriptors_generic")),
        ("visit_object_layout_slots",
         function_body(layout_visit, "visit_object_layout_slots")),
        ("plain_object_plan", function_body(copying_object_scan, "plain_object_plan")),
    ):
        require_code(
            body,
            r"gc_shape_keys_edge_slot\s*\(",
            f"descriptor keys edge enumerated as a child slot ({body_name})",
        )
        # Nothing in the visit reads the deleted mirror. The descriptor record
        # is both the strong edge and the stable rewritable location.
        if re.search(r"keys_array", body):
            raise CensusError(
                f"the GC slot visitor ({body_name}) reads ObjectHeader::keys_array "
                "again; the descriptor is the authoritative edge since #8112"
            )

    # ConstFn adds exact body facts to the same mint. Follow the forwarding
    # chain and check ordering in the body that actually publishes the
    # descriptor and its reverse indexes: the intern's out-of-line miss half,
    # which allocates the id and publishes the record.
    for wrapper, callee in (
        ("shape_descriptor_intern_with_rep", "shape_descriptor_intern_with_special"),
        ("shape_descriptor_intern_with_special", "shape_descriptor_intern_with_special_mode"),
        ("shape_descriptor_intern_with_special_mode", "shape_descriptor_mint_fresh"),
    ):
        require_code(
            function_body(shapes, wrapper),
            rf"\b{callee}\s*\(",
            f"{wrapper} delegates to the shared shape mint",
        )
    ensure = function_body(shapes, "shape_descriptor_mint_fresh")
    # The property is that the by-id descriptor is installed BEFORE the reverse
    # accelerator points at it — never which append spells it. #9768 added
    # `family_append_fresh`, which is `family_push_back` minus a membership scan
    # that is dead work for an id `alloc_shape_id` just minted and never reuses.
    # Both append to the same family list, so pinning only the older name made a
    # strictly cheaper append look like a lost ordering guarantee.
    ensure_append = next(
        (m for m in ("family_append_fresh", "family_push_back") if m in ensure),
        None,
    )
    if ensure_append is None:
        raise CensusError(
            "shape descriptor authority surface missing: family append in "
            "shape_descriptor_mint_fresh"
        )
    assert_before(
        ensure,
        "slab_mut().publish_dynamic",
        ensure_append,
        "by-id descriptor before reverse accelerator",
    )
    assert_before(
        ensure,
        "slab_mut().publish_dynamic",
        "facts_append_fresh",
        "by-id descriptor before facts accelerator",
    )
    # The structural publish body (charter step 5: `publish_object_shape_from`
    # delegates to it with an all-Any rep).
    sync = function_body(shapes, "publish_object_shape_from_rep")
    # #9317 routed every post-birth ShapeId publication through
    # `stamp_object_shape_id_with_carrier_note`, which performs the header
    # write and then arms `old_carrier` for a promoted receiver. The header
    # write is no longer spelled inline here, so name the funnel: the ordering
    # this guards (descriptor authoritative BEFORE the stamp) is unchanged.
    assert_before(
        sync,
        "shape_descriptor_ensure",
        "stamp_object_shape_id_with_carrier_note",
        "descriptor before ObjectHeader ShapeId",
    )
    # #8113 MINT-THEN-STAMP. With `field_count` deleted, the descriptor is the
    # only record of the live inline-slot bound, so a stamp-cleared window is a
    # window in which the collector traces ZERO payload slots. No publication
    # path may clear, and the only surviving `clear_object_shape_stamp` must be
    # test-only.
    for name in (
        "publish_object_shape_from",
        "publish_object_shape_from_rep",
        "publish_object_live_slot_count",
        "publish_object_live_slot_count_rep",
        "birth_publish_object_shape",
        "stamp_object_shape",
        "birth_stamp_object_shape",
    ):
        if "clear_object_shape_stamp" in function_body(shapes, name):
            raise CensusError(f"{name} clears the shape stamp: the live-slot bound has no mirror")
    if any(
        "clear_object_shape_stamp" in function_body(object_mod, name)
        for name in ("set_object_keys_with_live", "set_object_keys_with_live_rep")
    ):
        raise CensusError(
            "set_object_keys_with_live clears the shape stamp: "
            "the live-slot bound has no mirror"
        )
    if not re.search(
        r"#\[cfg\(test\)\]\s*\n\s*#\[inline\]\s*\n\s*pub\(crate\) unsafe fn clear_object_shape_stamp",
        shapes,
    ):
        raise CensusError("clear_object_shape_stamp escaped its #[cfg(test)] gate")
    # #9706: an OWNED keys array's same-address growth history is retired
    # behind the version its single owner now carries. The retirement must be
    # scoped to that array's family (never a scan of the whole table), must
    # keep the cache-carried versions an optimization cache can reinstall,
    # and must run AFTER the successor is stamped and armed (#9200's order).
    retirement = function_body(shapes, "retire_owned_shape_siblings")
    require_code(
        retirement,
        r"families\s*\.\s*get\s*\(\s*&keys\s*\)",
        "keys-scoped descriptor lineage index",
    )
    if re.search(r"slab\(\)\s*\.\s*for_each\s*\(", retirement):
        raise CensusError("owned-history retirement scans the global descriptor table")
    require_code(
        retirement,
        r"RECORD_FLAG_CACHE_CARRIER",
        "cache-carried versions survive same-address retirement",
    )
    assert_before(
        sync,
        "stamp_object_shape_id_with_carrier_note",
        "retire_owned_shape_siblings",
        "successor stamped and armed before the owned history is retired",
    )
    # A SHARED array's versions are immutable prefixes other objects may still
    # carry; growth and drop of the slot index must not touch them.
    for name in ("shape_keys_grown", "shape_drop"):
        if "remove_descriptor_and_reverse_indices" in function_body(shapes, name):
            raise CensusError(f"{name} eagerly deletes a sibling descriptor")

    require_code(
        live_slots,
        r"\bfn\s+set_object_live_slot_count\b",
        "central live-slot publication helper",
    )
    # #8113: that helper must delegate to the mint-then-stamp primitive, not
    # write a header word of its own (there is no longer one to write).
    require_code(
        function_body(live_slots, "set_object_live_slot_count"),
        r"shapes::publish_object_live_slot_count\s*\(",
        "live-slot publication goes through mint-then-stamp",
    )
    # #8113: the derived bound has no header mirror, so it must come from the
    # descriptor and fail CLOSED (0) when there is none.
    live_body = function_body(live_slots, "object_live_slot_count")
    require_code(
        live_body,
        r"live_inline_slot_count",
        "live-slot bound derived from the ShapeId descriptor",
    )
    require_code(live_body, r"unwrap_or\s*\(\s*0\s*\)", "live-slot bound fails closed")
    alloc_body = function_body(codegen_alloc, "emit_instance_alloc_inner")
    require_code(alloc_body, r"\bdescriptor_facts_exact\b", "raw-inline exact-facts admission gate")

    transition = function_body(transition_tail, "set_field_by_name_object_tail")
    cache_arm_at = transition.find("transition_cache_lookup")
    overflow_at = transition.find("overflow_set", cache_arm_at)
    if cache_arm_at < 0 or overflow_at < 0:
        raise CensusError("missing transition-cache publication arm")
    cache_arm = transition[cache_arm_at:overflow_at]
    assert_before(
        cache_arm,
        "set_object_live_slot_count",
        # The checked store funnel (charter step 5 P2b; was the raw slot store).
        "store_object_field_slot",
        "transition-cache count before value",
    )

    # Runtime guard contracts may consume ShapeId/descriptor facts, never the
    # compatibility ObjectHeader mirrors or a keys-pointer token.
    for name in (
        "method_direct_call_contract",
        "class_field_get_contract",
        "class_field_fast_contract",
    ):
        body = function_body(typed_guards, name)
        if re.search(r"expected_keys|\(\s*\*\s*obj\s*\)\s*\.\s*(?:keys_array|field_count|object_type)\b", body):
            raise CensusError(f"{name} reintroduced a legacy header guard fact")
        require_code(
            body,
            r"object_(?:shape(?:_(?:id|descriptor))?|layout_stamp)\s*\(",
            f"{name} ShapeId authority",
        )

    require_code(
        function_body(typed_guards, "js_class_field_set_fallback"),
        r"crate::object::js_object_set_field_by_name\(\s*obj_bits\s+as\s+\*mut\s+ObjectHeader,\s*key_raw\s+as\s+\*const\s+crate::StringHeader,\s*value,?\s*\)",
        "class-field fallback uses the complete named setter",
    )

    # The exported entry point only forwards; the dispatch tower it shares
    # with the call sites' chain-memo entries is the body that holds the
    # guard facts.
    require_code(
        function_body(native_call_method, "js_native_call_method"),
        r"\bnative_call_method_tower\s*\(",
        "js_native_call_method delegates to the dispatch tower",
    )
    for name in ("class_receiver_fast_guard",):
        body = function_body(native_call_method, name)
        if re.search(
            r"\(\s*\*\s*obj\s*\)\s*\.\s*(?:keys_array|field_count|object_type)\b|js_array_length\s*\(\s*keys\s*\)",
            body,
        ):
            raise CensusError(f"{name} reintroduced a legacy method guard fact")
        require_code(
            body,
            r"object_shape_descriptor\s*\(",
            f"{name} ShapeId descriptor authority",
        )
        require_code(
            body,
            r"logical_key_count\b",
            f"{name} exact logical key count",
        )

    # S2: RegExp receivers use the ordinary shape authority. Their private
    # matcher points at a separately traced data cell, never an exotic receiver.
    for name in ("object_is_regular", "object_is_shaped"):
        body = function_body(object_mod, name)
        require_code(body, r"obj_type\s*==\s*crate::gc::GC_TYPE_OBJECT", f"{name} GC kind")
        if re.search(r"regex_header_has_magic|object_type", body):
            raise CensusError(f"{name} reintroduced an old payload discriminator")
    require_code(
        function_body(regex_runtime, "js_regexp_new"),
        r"perex_construct::new\s*\(",
        "RegExp construction delegates to its single birth site",
    )
    # S4 literals and dynamic construction share the same ordinary birth.
    literal_runtime = (ROOT / "crates/perry-runtime/src/regex/literal.rs").read_text()
    require_code(function_body(literal_runtime, "js_regexp_literal"),
                 r"super::instance::new\s*\(", "literal ordinary receiver birth")
    require_code(function_body(literal_runtime, "literal_miss"),
                 r"perex_construct::new_data\s*\(", "literal immutable data construction")
    regexp_alloc = function_body(regex_construct, "new")
    require_code(regexp_alloc, r"new_data\s*\(", "RegExp construction roots immutable data")
    require_code(regexp_alloc, r"super::instance::new\s*\(", "RegExp ordinary receiver birth")
    require_code(
        function_body(regex_construct, "new_data_miss"),
        r"arena_alloc_gc\s*\(\s*std::mem::size_of::<RegExpData>\(\),\s*"
        r"std::mem::align_of::<RegExpData>\(\),\s*crate::gc::GC_TYPE_REGEXP",
        "RegExpData dedicated traced GC birth kind",
    )
    require_code(function_body(regex_construct, "new_data"), r"new_data_miss\s*\(", "RegExpData cache miss delegates to traced birth")
    instance_path = "crates/perry-runtime/src/regex/instance.rs"
    instance = function_body(clean[instance_path], "new")
    instance_slow = function_body(clean[instance_path], "new_slow")
    prepare = function_body(clean[instance_path], "prepare_shape")
    for body, pattern, label in (
        (instance_slow, r"object_alloc_plain_born\s*\(\s*2\s*,\s*shape\s*\)", "slow ordinary birth"),
        (instance, r"arena_alloc_gc_no_collect\(size,\s*8,\s*crate::gc::GC_TYPE_OBJECT\)", "noncollecting ordinary birth"),
        (instance, r"\(\*receiver\)\.parent_class_id\s*=\s*shape", "birth uses the prepared shape"),
        (instance, r"new_slow\(LayoutWord\(shape\),\s*data\)", "declined nursery birth reaches slow birth"),
        (instance_slow, r"prepare_shape\s*\(\s*receiver\s*\)", "canonical birth on miss"),
        (prepare, r"MATCHER_READ\.birth_key\(\)", "qualified intrinsic matcher key"),
        (prepare, r"extend_key_with_entry\s*\(\s*&proof,\s*CanonicalKeys::EMPTY,\s*private_key,\s*PRIVATE_FIELD_ENTRY", "intrinsic private matcher entry"),
        (prepare, r"stamp_linked_final_shape\s*\([\s\S]*?proto_id", "ordinary prototype link"),
        (prepare, r"intrinsic_prototype\s*\(", "real RegExp prototype"),
    ):
        require_code(body, pattern, "RegExp " + label)
    require_code(
        function_body(sources[instance_path], "prepare_shape"),
        r'intern_ascii_literal\(b"lastIndex"\)[\s\S]*?index_key,\s*attr_bits_to_entry\(1\)',
        "RegExp own writable-only lastIndex descriptor",
    )
    require_code(
        function_body(regex_runtime, "regexp_data_of"),
        r"MATCHER_READ\.read\(value\)",
        "RegExp brand uses the intrinsic private read site",
    )
    alloc_basic = clean["crates/perry-runtime/src/object/alloc_basic.rs"]
    for name, pattern in (
        ("object_alloc_plain", r"object_alloc_with_parent_impl::<true,\s*false,\s*false>\(0,\s*0,\s*field_count\)"),
        ("object_alloc_plain_born_slow", r"object_alloc_unpublished\(0,\s*field_count\)"),
    ):
        require_code(function_body(alloc_basic, name), pattern, "base RegExp has no special class id")
    for name in ("object_alloc_with_parent_impl", "object_alloc_born_impl", "object_alloc_unpublished"):
        require_code(function_body(alloc_basic, name), r"arena_alloc_gc\([^;]*GC_TYPE_OBJECT", "ordinary receiver GC birth")
    require_code(
        function_body(exotic_expando, "exotic_expando_kind"),
        r"\bexotic_kind_of_gc_type\s*\(",
        "exotic_expando_kind delegates to the type -> kind table",
    )
    require_code(
        function_body(get_field_tail, "get_field_by_name_object_tail"),
        r"\bobject_keys_and_live_slot_count\s*\(\s*obj\s*\)",
        "ordinary getter obtains keys and live slots from the receiver descriptor",
    )
    for body in (instance, prepare, regexp_alloc, exotic_expando, get_field_tail):
        if re.search(r"GC_TYPE_REGEXP|ExoticKind::RegExp|regex_header_has_magic|is_regex_pointer", body):
            raise CensusError("RegExp receiver reintroduced an exotic/magic property path")

    # Emitted guards may read exactly two header offsets: `class_id` @0 and the
    # ShapeId @4. Guards have no reason to address anything at or past 8.
    #
    # #8113 also fixed this arm's VACUITY. It used to match only
    # `add(..., "N")`, while these guard functions emit
    # `gep(I8, &p, &[(I64, "N")])` — so planting a keys-offset read left it
    # green. Both spellings are matched now, and each function must be shown to
    # read the ShapeId at all, so a guard that stops reading the header
    # entirely cannot pass by emitting nothing.
    for source, names in (
        (raw_class_guard, (
            "emit_proven_shape_recheck",
            "emit_class_field_read_precheck",
        )),
        # #10185: the element clone's residual moved out of
        # `emit_element_shape_field_load` into the deref helper both the
        # per-read path and the shared once-per-iteration prologue call, so the
        # ShapeId read this asserts now lives there. Naming the function that
        # actually emits the check is what keeps the assertion from going
        # vacuous the next time the emitter is split.
        (raw_element_guard, ("emit_element_deref_with_residual",)),
    ):
        for name in names:
            body = function_body(source, name)
            # #8113: `class_id` lives at offset 0 and the ShapeId at 4 now, so
            # the offset rule is `forbidden_header_offsets` (below), which
            # encodes the removed words' offsets for the CURRENT layout and
            # matches both the `add(..)` and `gep(I8, ..)` spellings.
            if re.search(r"expected_keys", body):
                raise CensusError(f"{name} emits a removed ObjectHeader fact")
            if forbidden_header_offsets(body):
                raise CensusError(f"{name} emits a removed ObjectHeader fact")
            if name == "emit_class_field_read_precheck":
                # The per-access guard reads the ObjectHeader's first WORD —
                # `class_id` @0 and the ShapeId @4 in one little-endian i64 —
                # and compares it with `expected_class_identity`, which puts
                # the ShapeId in the high 32 bits. All three halves are
                # required, so dropping the ShapeId from the compare fails.
                #
                # The expectation is the class's own ShapeId: the driver's
                # static id as an immediate when there is one (#11653),
                # otherwise the `@perry_class_shape_id_*` global, read VOLATILE
                # per access (the runtime rewrites it once when an imported
                # class's typed id is published). The precheck asks
                # `class_shape_id_operand_on_block` for it with `volatile`
                # true; that helper owns the immediate-or-volatile-load
                # choice. S6 retired
                # the poisonable `@perry_class_guard_shape_*` twin: nothing may
                # tell this compare not to trust the shape.
                require_code(
                    body,
                    r"load\s*\(\s*I64\s*,\s*&obj_ptr\s*\)",
                    f"{name} loads the ObjectHeader identity word",
                )
                require_code(
                    body,
                    r"expected_class_identity\s*\(\s*blk\s*,\s*expected_class_id\s*,\s*&live_shape\s*\)",
                    f"{name} compares the identity word with the live expectation",
                )
                require_code(
                    body,
                    r"class_shape_id_operand_on_block\s*\(\s*blk\s*,\s*keys_global_name\s*,\s*true\s*\)",
                    f"{name} reads the class ShapeId expectation VOLATILE (immediate or volatile load), per access",
                )
                require_code(
                    function_body(raw_class_guard, "expected_class_identity"),
                    r"shl\s*\(\s*I64\s*,\s*&shape_bits\s*,\s*\"32\"\s*\)",
                    "expected_class_identity places the ShapeId at header offset 4",
                )
                continue
            require_code(
                body,
                r"\(\s*I64\s*,\s*\"4\"\s*\)",
                f"{name} reads the authoritative ShapeId at header offset 4",
            )

    assert_layout_dispatch_authority(sources)
    assert_rule3_licenses_the_kind_fence_removal(rule3, gc_types)

    require_code(gc_types, r"GC_TYPE_REGEXP\s*:\s*u8", "RegExpData GC discriminator")
    regexp_info_match = re.search(
        r"gc_type_info_entry\(\s*GC_TYPE_REGEXP\b[\s\S]*?\n\s*\)\s*\)",
        gc_types,
    )
    if not regexp_info_match:
        raise CensusError("shape descriptor authority surface missing: RegExp type metadata")
    regexp_info = regexp_info_match.group(0)
    # Data has only traced strings/program edges. It has no receiver identity,
    # expando owner, relocation registry or per-object finalization work.
    for pattern, label in (
        (r"GcMoveHookKind::None", "no RegExpData move hook"),
        (r"GcRewriteHookKind::None", "no RegExpData address registry"),
        (r"GcFinalizeHookKind::None", "no RegExpData finalize hook"),
        (r"GcLayoutSlotKind::RegExpFields", "RegExpData traced field layout"),
    ):
        require_code(regexp_info, pattern, label)
    require_code(function_body(regex_runtime, "regex_gc_slot_ptrs"), r"pattern_ptr", "RegExpData traced source")
    require_code(function_body(regex_runtime, "regex_gc_slot_ptrs"), r"flags_ptr", "RegExpData traced flags")
    require_code(function_body(regex_runtime, "regex_program_slot"), r"perex_program", "RegExpData traced program")
    if "OBJ_FLAG_CLASS_OBJECT" in gc_types + class_guard + element_guard + write_pics:
        raise CensusError("class kind reintroduced a GcHeader layout-bit alias")
    assert_header_fields(object_mod)
    class_probe = function_body(object_mod, "object_is_regular")
    require_code(
        class_probe,
        r"shape_object_kind_of_receiver\([\s\S]*?\)\s*\.is_some_and\(\|kind\| kind\.is_ordinary_layout\(\)\)",
        "ordinary-object descriptor kind authority",
    )


def forbidden_header_offsets(body: str) -> list[str]:
    """Positive `ObjectHeader` byte offsets an emitted guard must not address.

    Matches both emitter spellings: a gep index tuple `(I64, "N")` and an
    `add(I64, &base, "N")`. `sub(...)` is deliberately NOT matched — it is how
    the GcHeader bytes at -8/-7/-6 are reached — and neither is a negative
    literal.
    """
    gep = re.findall(r'\(\s*I64\s*,\s*"(-?\d+)"\s*\)', body)
    add = re.findall(r'\.add\s*\(\s*I64\s*,\s*&\w[\w.]*\s*,\s*"(-?\d+)"\s*\)', body)
    return sorted({off for off in gep + add if int(off) >= 8})


def assert_header_fields(object_mod: str) -> None:
    """Pin `ObjectHeader`'s exact declared field list (#8113).

    The multiset census only sees fields named in `FIELDS`, so re-adding a
    `field_count` word would slip past it entirely. This does not: the header is
    an ABI with a published crates.io mirror (`perry-ffi::ObjectHeader`) and a
    runtime revision constant (`perry_object_header_abi_revision`), and a change
    here has to be made on purpose in all three places.
    """
    match = re.search(
        r"pub struct ObjectHeader\s*\{(?P<body>[^}]*)\}",
        object_mod,
    )
    if not match:
        raise CensusError("shape descriptor authority surface missing: ObjectHeader declaration")
    fields = tuple(re.findall(r"pub\s+(\w+)\s*:", match.group("body")))
    if fields != OBJECT_HEADER_FIELDS:
        raise CensusError(
            "ObjectHeader field list changed: "
            f"{fields} != {OBJECT_HEADER_FIELDS}. This is an ABI change — update "
            "OBJECT_HEADER_FIELDS here, perry-ffi's mirror + "
            "OBJECT_HEADER_ABI_REVISION, perry_object_header_abi_revision(), "
            "target_layout::object_header_size_bytes, and the emitted header "
            "offsets, in one commit."
        )


def swap_once(source: str, left: str, right: str) -> str:
    left_at = source.find(left)
    right_at = source.find(right)
    if left_at < 0 or right_at < 0:
        raise CensusError(f"sabotage fixture missing: {left!r} / {right!r}")
    marker_left = "__CENSUS_SWAP_LEFT__"
    marker_right = "__CENSUS_SWAP_RIGHT__"
    return source.replace(left, marker_left, 1).replace(right, marker_right, 1).replace(
        marker_left, right, 1
    ).replace(marker_right, left, 1)


def expect_rejected(label: str, check: Callable[[], None]) -> None:
    try:
        check()
    except CensusError:
        return
    raise CensusError(f"sabotage self-test was not rejected: {label}")


def run_sabotage_selftests(sources: dict[str, str], baseline: dict[str, object]) -> None:
    # These source-only mutations must fail the new representation contract;
    # replacing the retired exotic checks must not erase the authority gate.
    for path, before, after, label in (
        ("crates/perry-runtime/src/regex/instance.rs", "object_alloc_plain_born(2, shape)", "object_alloc_plain_born(3, shape)", "ordinary RegExp birth removed"),
        ("crates/perry-runtime/src/regex/instance.rs", "private_key,\n            PRIVATE_FIELD_ENTRY,", "private_key,\n            0,", "matcher entry not defined"),
        ("crates/perry-runtime/src/regex/instance.rs", 'attr_bits_to_entry(1)', 'attr_bits_to_entry(7)', "lastIndex attributes widened"),
        ("crates/perry-runtime/src/regex.rs", "MATCHER_READ.read(value)", "Some(value)", "RegExp brand accepts every value"),
        ("crates/perry-runtime/src/object/alloc_basic.rs", "object_alloc_unpublished(0, field_count)", "object_alloc_unpublished(0xFFFF0021, field_count)", "special RegExp class id restored"),
    ):
        broken = dict(sources)
        if broken[path].count(before) != 1:
            raise CensusError("RegExp authority sabotage fixture missing: " + label)
        broken[path] = broken[path].replace(before, after, 1)
        expect_rejected(label, lambda: assert_authority_surfaces(broken))
    for path, name, before, after in (
        ("crates/perry-runtime/src/regex/instance.rs", "new", "crate::gc::GC_TYPE_OBJECT", "crate::gc::GC_TYPE_REGEXP"),
        ("crates/perry-runtime/src/regex/instance.rs", "new", "(*receiver).parent_class_id = shape", "(*receiver).parent_class_id = 0"),
        ("crates/perry-runtime/src/object/shapes.rs", "scan_shape_keys_address", "descriptor.old_carrier || descriptor.cache_carrier", "true"),
    ):
        broken = dict(sources)
        body = function_body(broken[path], name)
        if before not in body:
            raise CensusError("authority sabotage fixture missing: " + name)
        broken[path] = broken[path].replace(body, body.replace(before, after, 1), 1)
        expect_rejected(name + " loses its authority", lambda: assert_authority_surfaces(broken))
    path = "crates/perry-runtime/src/regex/perex_construct.rs"
    broken = dict(sources)
    body = function_body(broken[path], "new_data_miss")
    broken[path] = broken[path].replace(body, body.replace("crate::gc::GC_TYPE_REGEXP", "crate::gc::GC_TYPE_OBJECT"), 1)
    expect_rejected("data born as a receiver", lambda: assert_authority_surfaces(broken))
    path = "crates/perry-runtime/src/object/exotic_expando.rs"
    broken = dict(sources)
    broken[path] += "\nfn regexp_exotic_sabotage() { let _ = ExoticKind::RegExp; }\n"
    expect_rejected("exotic RegExp property path restored", lambda: assert_authority_surfaces(broken))

    path = "crates/perry-runtime/src/gc/types.rs"
    broken = dict(sources)
    match = re.search(r"gc_type_info_entry\(\s*GC_TYPE_REGEXP\b[\s\S]*?\n\s*\)\s*\)", broken[path])
    if match is None:
        raise CensusError("RegExpData move-hook sabotage fixture missing")
    body = match.group(0)
    broken[path] = broken[path].replace(body, body.replace("GcMoveHookKind::None", "GcMoveHookKind::ExoticExpandoOwner"), 1)
    expect_rejected("data given an expando move hook", lambda: assert_authority_surfaces(broken))
    path = "crates/perry-runtime/src/regex.rs"
    broken = dict(sources)
    body = function_body(broken[path], "regex_program_slot")
    broken[path] = broken[path].replace(body, "None", 1)
    expect_rejected("data program edge untraced", lambda: assert_authority_surfaces(broken))

    run_layout_dispatch_sabotage(sources, expect_rejected)

    missing_authority = dict(sources)
    missing_authority.pop("crates/perry-runtime/src/object/shapes.rs")
    expect_rejected(
        "missing authority source",
        lambda: assert_authority_surfaces(missing_authority),
    )

    raw_mutation = dict(sources)
    path = "crates/perry-runtime/src/object/mod.rs"
    raw_mutation[path] += "\nunsafe fn census_sabotage(o: *mut ObjectHeader) { (*o).keys_array = core::ptr::null_mut(); }\n"
    expect_rejected(
        "raw ObjectHeader mutation",
        lambda: compare_exact_census(observed_census(raw_mutation), baseline),
    )

    strong_root = dict(sources)
    path = "crates/perry-runtime/src/object/shapes.rs"
    strong_root[path] = strong_root[path].replace(
        "visitor.visit_metadata_usize_slot(&mut addr)",
        "visitor.visit_usize_slot(&mut addr)",
        1,
    )
    expect_rejected(
        "strong descriptor-table root",
        lambda: assert_authority_surfaces(strong_root),
    )

    alternate_strong_root = dict(sources)
    path = "crates/perry-runtime/src/object/shapes.rs"
    alternate_strong_root[path] = alternate_strong_root[path].replace(
        "visitor.visit_metadata_usize_slot(&mut addr)",
        "{ let moved = unsafe { visitor.visit_usize_raw_slot(&mut addr) }; "
        "visitor.visit_metadata_usize_slot(&mut addr); moved }",
        1,
    )
    expect_rejected(
        "alternate strong raw-slot API plus dead metadata call",
        lambda: assert_authority_surfaces(alternate_strong_root),
    )

    shapes_path = "crates/perry-runtime/src/object/shapes.rs"
    store_path = "crates/perry-runtime/src/object/shapes_store.rs"
    region_path = "crates/perry-runtime/src/object/shapes_store/flat.rs"
    changed_region = dict(sources)
    old = "base: *mut ShapeRecord"
    if changed_region[region_path].count(old) != 1:
        raise CensusError("Region stability sabotage subject missing")
    changed_region[region_path] = changed_region[region_path].replace(old, "base: Vec<ShapeRecord>", 1)
    expect_rejected("Region records replaced by a moving vector", lambda: assert_authority_surfaces(changed_region))
    reallocating_region = dict(sources)
    reallocating_region[region_path] += "\nfn census_bad_realloc() { realloc(); }\n"
    expect_rejected("Region reallocation", lambda: assert_authority_surfaces(reallocating_region))
    flat_slab = dict(sources)
    flat_slab[store_path] = flat_slab[store_path].replace(
        "type ChunkCells = [UnsafeCell<*mut ShapeRecord>; CHUNK_LEN];",
        "type ChunkCells = Vec<UnsafeCell<*mut ShapeRecord>>;",
        1,
    )
    expect_rejected(
        "slab chunk turned into a reallocating Vec",
        lambda: assert_authority_surfaces(flat_slab),
    )
    keys_not_first = dict(sources)
    keys_not_first[store_path] = keys_not_first[store_path].replace(
        "    pub(super) keys: u64,\n    pub(super) semantic_generation: u64,",
        "    pub(super) semantic_generation: u64,\n    pub(super) keys: u64,",
        1,
    )
    expect_rejected(
        "keys word moved off the front of the slab record",
        lambda: assert_authority_surfaces(keys_not_first),
    )

    ungated_root = dict(sources)
    ungated_root[shapes_path] = ungated_root[shapes_path].replace(
        "let moved = if descriptor.old_carrier || descriptor.cache_carrier {",
        "let moved = if true {",
        1,
    )
    expect_rejected(
        "descriptor rooting un-gated into an unconditional table root",
        lambda: assert_authority_surfaces(ungated_root),
    )

    header_fact_read = dict(sources)
    path = "crates/perry-runtime/src/gc/layout_slot_visit.rs"
    header_fact_read[path] = header_fact_read[path].replace(
        "let shape_keys_edge = if",
        "let _mirror = (*obj).keys_array;\n    let shape_keys_edge = if",
        1,
    )
    expect_rejected(
        "GC slot visitor reads the header mirror for a fact",
        lambda: assert_authority_surfaces(header_fact_read),
    )

    plan_fact_read = dict(sources)
    path = "crates/perry-runtime/src/gc/copying_object_scan.rs"
    plan_fact_read[path] = plan_fact_read[path].replace(
        "let keys_edge = crate::object::gc_shape_keys_edge_slot(shape);",
        "let _mirror = (*obj).keys_array;\n    let keys_edge = crate::object::gc_shape_keys_edge_slot(shape);",
        1,
    )
    expect_rejected(
        "copying plain-object scan reads the header mirror for a fact",
        lambda: assert_authority_surfaces(plan_fact_read),
    )

    for path, name in (
        ("crates/perry-runtime/src/gc/layout_slot_visit.rs", "visit_gc_layout_slot_descriptors_generic"),
        ("crates/perry-runtime/src/gc/layout_slot_visit.rs", "visit_object_layout_slots"),
        ("crates/perry-runtime/src/gc/copying_object_scan.rs", "plain_object_plan"),
    ):
        missing_edge = dict(sources)
        body = function_body(sources[path], name)
        changed = body.replace("gc_shape_keys_edge_slot", "lost_keys_edge_slot", 1)
        assert changed != body, f"missing mutation subject: {name}"
        missing_edge[path] = sources[path].replace(body, changed, 1)
        expect_rejected(f"{name} loses descriptor keys edge",
                        lambda: assert_authority_surfaces(missing_edge))

    for name in ("visit_object_layout_slots", "visit_gc_layout_slot_descriptors_generic"):
        missing_route = dict(sources)
        path = "crates/perry-runtime/src/gc/layout_slot_visit.rs"
        call = f"{name}(header, visit);"
        assert call in sources[path], f"missing mutation subject: {name}"
        missing_route[path] = sources[path].replace(call, "return;", 1)
        expect_rejected(f"dispatcher loses {name}",
                        lambda: assert_authority_surfaces(missing_route))

    # Both reverse accelerators must remain downstream of the actual mint,
    # including after a wrapper refactor. Exercise each ordering separately.
    for accelerator in ("family_append_fresh", "facts_append_fresh"):
        inverted_mint = dict(sources)
        mint_body = function_body(
            inverted_mint[shapes_path], "shape_descriptor_mint_fresh"
        )
        inverted_body = swap_once(mint_body, "slab_mut().publish_dynamic", accelerator)
        inverted_mint[shapes_path] = inverted_mint[shapes_path].replace(
            mint_body, inverted_body, 1
        )
        expect_rejected(
            f"{accelerator} before descriptor publication",
            lambda: assert_authority_surfaces(inverted_mint),
        )

    inverted_publication = dict(sources)
    path = "crates/perry-runtime/src/object/shapes.rs"
    publication_body = function_body(
        inverted_publication[path], "publish_object_shape_from_rep"
    )
    inverted_body = swap_once(
        publication_body,
        # #9029 tombstones: the lineage publish carries hole_count, so the
        # mint call in the structural publish is the form taking a rep
        # (charter step 5).
        "shape_descriptor_ensure_with_rep(",
        "stamp_object_shape_id_with_carrier_note",
    )
    inverted_publication[path] = inverted_publication[path].replace(
        publication_body, inverted_body, 1
    )
    expect_rejected(
        "ObjectHeader id before descriptor publication",
        lambda: assert_authority_surfaces(inverted_publication),
    )

    unscoped_retirement = dict(sources)
    path = "crates/perry-runtime/src/object/shapes.rs"
    retirement_body = function_body(
        unscoped_retirement[path], "retire_owned_shape_siblings"
    )
    unscoped_body, substitutions = re.subn(
        r"families\s*\.\s*get\s*\(\s*&keys\s*\)",
        "families.get(&0)",
        retirement_body,
        count=1,
    )
    if substitutions != 1:
        raise CensusError("descriptor retirement sabotage fixture missing")
    unscoped_retirement[path] = unscoped_retirement[path].replace(
        retirement_body, unscoped_body, 1
    )
    expect_rejected(
        "descriptor retirement without keys index",
        lambda: assert_authority_surfaces(unscoped_retirement),
    )

    early_retirement = dict(sources)
    publish_body = function_body(early_retirement[path], "publish_object_shape_from_rep")
    early_body = swap_once(
        publish_body,
        "stamp_object_shape_id_with_carrier_note",
        "retire_owned_shape_siblings",
    )
    early_retirement[path] = early_retirement[path].replace(publish_body, early_body, 1)
    expect_rejected(
        "owned history retired before the successor is stamped",
        lambda: assert_authority_surfaces(early_retirement),
    )

    # #10843 removed the emitted GC-kind fence; what licenses that removal is
    # rule 3 being CLOSED, so that is what the census asserts and that is what
    # this arm sabotages. Put a pointer-tagged kind back into the fence-keeping
    # set -- the exact condition under which the removed GC_TYPE_OBJECT load
    # would be needed again. `DateCell` is the honest choice: it is the one
    # kind #10828 could not bound and had to RELAYOUT (`new Date(-1)` put
    # 0xBFF0_0000 at +4), so this plants the real historical hole rather than
    # an invented one.
    reopened_rule3 = dict(sources)
    path = "crates/perry-runtime/src/object/shape_rule3.rs"
    reopened_body, substitutions = re.subn(
        r"(gc::GC_TYPE_DATE_CELL,.*?)Rule3Word::StructurallySmall",
        r'\1Rule3Word::RangeReachable("timestamp back at +0")',
        reopened_rule3[path],
        count=1,
        flags=re.DOTALL,
    )
    if substitutions != 1:
        raise CensusError("rule-3 fence-keeping sabotage fixture missing")
    reopened_rule3[path] = reopened_body
    expect_rejected(
        "a GC kind reopened rule 3 while the emitted read has no GC-kind fence",
        lambda: assert_authority_surfaces(reopened_rule3),
    )

    # The emptiness test must not be satisfiable by DELETING the table: an
    # empty RULE3_KINDS has no RangeReachable row either (#7024/#7025 -- a
    # green gate whose subject never ran). Drop one row and prove the
    # coverage half catches it.
    thinned_rule3 = dict(sources)
    thinned_body, substitutions = re.subn(
        r"\(\s*gc::GC_TYPE_DATE_CELL,.*?\),\n",
        "",
        thinned_rule3[path],
        count=1,
        flags=re.DOTALL,
    )
    if substitutions != 1:
        raise CensusError("rule-3 coverage sabotage fixture missing")
    thinned_rule3[path] = thinned_body
    expect_rejected(
        "a GC kind lost its rule-3 verdict, making the fence-removal licence vacuous",
        lambda: assert_authority_surfaces(thinned_rule3),
    )

    # #8113: the gep-spelled emitted guards. This arm was VACUOUS before —
    # it matched only `add(..., "N")` — so plant a keys-offset gep and prove
    # it is caught now.
    gep_ir = dict(sources)
    path = "crates/perry-codegen/src/expr/class_field_inline_guard.rs"
    gep_body, substitutions = re.subn(
        r'gep\(I8, &obj_ptr, &\[\(I64, "4"\)\]\)',
        'gep(I8, &obj_ptr, &[(I64, "8")])',
        gep_ir[path],
        count=1,
    )
    if substitutions != 1:
        raise CensusError("gep emitted-offset sabotage fixture missing")
    gep_ir[path] = gep_body
    expect_rejected(
        "keys-array header offset in a gep-spelled emitted guard",
        lambda: assert_authority_surfaces(gep_ir),
    )

    # #8113: re-adding a deleted header word must be red, not merely
    # un-baselined (the multiset census cannot see a field it does not track).
    readded_field = dict(sources)
    path = "crates/perry-runtime/src/object/mod.rs"
    readded_field[path] = readded_field[path].replace(
        "    pub meta: *mut ObjectMeta,",
        "    pub keys_array: *mut ArrayHeader,\n    pub meta: *mut ObjectMeta,",
        1,
    )
    expect_rejected(
        "re-added ObjectHeader payload word",
        lambda: assert_authority_surfaces(readded_field),
    )

    # #8113: a re-introduced clear-then-remint window.
    cleared_publication = dict(sources)
    path = "crates/perry-runtime/src/object/shapes.rs"
    cleared_body = function_body(cleared_publication[path], "publish_object_live_slot_count_rep")
    cleared_publication[path] = cleared_publication[path].replace(
        cleared_body,
        cleared_body.replace(
            "let predecessor = object_shape_descriptor(obj);",
            "let predecessor = object_shape_descriptor(obj);\n    clear_object_shape_stamp(obj);",
            1,
        ),
        1,
    )
    expect_rejected(
        "clear-then-remint window in the live-slot publication",
        lambda: assert_authority_surfaces(cleared_publication),
    )

    stale_summary = json.loads(json.dumps(baseline))
    stale_summary["summary"]["raw_member_files"] += 1
    expect_rejected(
        "stale baseline summary",
        lambda: compare_exact_census(observed_census(sources), stale_summary),
    )


def compare_exact_census(observed: dict[str, object], baseline: dict[str, object]) -> None:
    for key in (
        "raw_member_callsite_multiset",
        "codegen_object_header_size_callsite_multiset",
        "summary",
    ):
        actual = observed.get(key)
        expected = baseline.get(key)
        if actual != expected:
            if key == "summary":
                raise CensusError(
                    f"exact shape census summary changed; actual={actual}, expected={expected}"
                )
            actual_counter = Counter(actual or {})
            expected_counter = Counter(expected or {})
            added = list((actual_counter - expected_counter).items())[:8]
            removed = list((expected_counter - actual_counter).items())[:8]
            raise CensusError(
                f"exact callsite census changed for {key}; added={added}, removed={removed}"
            )


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "--emit-baseline",
        action="store_true",
        help="print the current exact multiset for reviewed baseline refresh",
    )
    args = parser.parse_args()
    try:
        run_literal_lexer_selftest()
        sources = rust_sources()
        observed = observed_census(sources)
        if args.emit_baseline:
            print(json.dumps(observed, indent=2, sort_keys=True))
            return 0
        baseline = json.loads(BASELINE_PATH.read_text(encoding="utf-8"))
        compare_exact_census(observed, baseline)
        assert_authority_surfaces(sources)
        run_sabotage_selftests(sources, baseline)
    except (CensusError, AssertionError) as error:
        # A detected drift must read as a verdict, not as a traceback. This census
        # stacks dozens of independent assertions, so every run reports exactly one
        # of them; an escaping exception buries which subject moved under a stack
        # trace and makes the next one harder to reach.
        print(f"::error::shape_descriptor_census: {error}")
        return 1
    print(json.dumps(observed["summary"], indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
