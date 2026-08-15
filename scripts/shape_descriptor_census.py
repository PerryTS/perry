#!/usr/bin/env python3
"""#8067/#8113 exact shape-header census plus authority-order sabotage tests.

#8113 deleted `ObjectHeader::object_type` and `::field_count`, so `keys_array`
is the last compatibility mirror and the only field this census tracks. The
deleted pair is now guarded structurally instead: `assert_header_fields` pins
the exact declared field list, so re-adding a word is red rather than merely
un-baselined.
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
# The exact `ObjectHeader` field list, in order. #8113 took it from six fields
# (32 bytes LP64) to four (24). Changing it is an ABI change with a published
# crates.io mirror (`perry-ffi`), so it must be a deliberate edit here too.
OBJECT_HEADER_FIELDS = ("class_id", "parent_class_id", "keys_array", "meta")
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


def require_code(source: str, pattern: str, label: str) -> None:
    if not re.search(pattern, source, re.MULTILINE | re.DOTALL):
        raise CensusError(f"shape descriptor authority surface missing: {label}")


def assert_before(body: str, first: str, second: str, label: str) -> None:
    first_at = body.find(first)
    second_at = body.find(second)
    if first_at < 0 or second_at < 0 or first_at >= second_at:
        raise CensusError(f"shape descriptor authority ordering failed: {label}")


def assert_authority_surfaces(sources: dict[str, str]) -> None:
    authority_paths = (
        "crates/perry-runtime/src/object/shapes.rs",
        "crates/perry-runtime/src/object/mod.rs",
        "crates/perry-runtime/src/object/live_slots.rs",
        "crates/perry-codegen/src/lower_call/new_alloc.rs",
        "crates/perry-runtime/src/gc/layout_slot_visit.rs",
        "crates/perry-runtime/src/object/field_set_by_name/tail.rs",
        "crates/perry-runtime/src/typed_feedback/guards.rs",
        "crates/perry-runtime/src/object/native_call_method.rs",
        "crates/perry-runtime/src/object/exotic_expando.rs",
        "crates/perry-runtime/src/object/field_get_set/get_field_by_name_tail.rs",
        "crates/perry-runtime/src/object/field_get_set/ic_miss.rs",
        "crates/perry-runtime/src/proxy/put_value.rs",
        "crates/perry-runtime/src/gc/types.rs",
        "crates/perry-runtime/src/regex.rs",
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
    shapes = clean["crates/perry-runtime/src/object/shapes.rs"]
    object_mod = clean["crates/perry-runtime/src/object/mod.rs"]
    live_slots = clean["crates/perry-runtime/src/object/live_slots.rs"]
    codegen_alloc = clean["crates/perry-codegen/src/lower_call/new_alloc.rs"]
    layout_visit = clean["crates/perry-runtime/src/gc/layout_slot_visit.rs"]
    transition_tail = clean[
        "crates/perry-runtime/src/object/field_set_by_name/tail.rs"
    ]
    typed_guards = clean["crates/perry-runtime/src/typed_feedback/guards.rs"]
    native_call_method = clean[
        "crates/perry-runtime/src/object/native_call_method.rs"
    ]
    exotic_expando = clean["crates/perry-runtime/src/object/exotic_expando.rs"]
    get_field_tail = clean[
        "crates/perry-runtime/src/object/field_get_set/get_field_by_name_tail.rs"
    ]
    ic_miss = clean["crates/perry-runtime/src/object/field_get_set/ic_miss.rs"]
    put_value = clean["crates/perry-runtime/src/proxy/put_value.rs"]
    gc_types = clean["crates/perry-runtime/src/gc/types.rs"]
    regex_runtime = clean["crates/perry-runtime/src/regex.rs"]
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
    raw_generic_pic = sources[
        "crates/perry-codegen/src/expr/property_get/generic_dispatch.rs"
    ]
    raw_write_pics = sources["crates/perry-codegen/src/expr/proxy_reflect.rs"]

    for pattern, label in (
        # `PtrHashMap` since #8157 (SipHash on a bare u32 was 25% of self time in
        # `shapes`). The fact this asserts is that a by-id table EXISTS, not which
        # hasher backs it, so accept either spelling.
        (r"descriptors\s*:\s*(?:[\w:]+::)?(?:Ptr)?HashMap\s*<\s*u32\s*,\s*ShapeDescriptor", "by-id descriptor table"),
        (r"logical_key_count\s*:\s*u32", "exact logical-key fact"),
        (r"live_inline_slot_count\s*:\s*u32", "exact live-slot fact"),
        (r"semantic_generation\s*:\s*u64", "semantic transition fact"),
        (r"object_kind\s*:\s*ShapeObjectKind", "authoritative receiver-kind fact"),
        (r"\bfn\s+shape_descriptor_by_id\b", "by-id lookup"),
        (r"\bfn\s+debug_assert_object_shape_parity\b", "parity assertion"),
        (r"\bfn\s+synchronize_live_object_shape_descriptor_after_header_visit\b", "live-object descriptor mirror"),
        (r"is_dead_owner\s*\(\s*descriptor\.keys\s+as\s+usize\s*\)", "dead descriptor pruning"),
    ):
        require_code(shapes, pattern, label)

    allocator = function_body(shapes, "alloc_shape_id_from")
    require_code(allocator, r"\bcompare_exchange_weak\s*\(", "exhaustion park")
    if re.search(r"\bfetch_add\s*\(", allocator):
        raise CensusError("ShapeId allocator wraps instead of parking")
    require_code(shapes, r"\bfn\s+shape_id_exhausted_abort\b", "exhaustion fail-stop")
    public_ensure = function_body(shapes, "shape_id_for_keys_ensure")
    require_code(
        public_ensure,
        r"publish_shape_result\s*\(",
        "typed shape-mint errors fail stop",
    )

    scanner = function_body(shapes, "scan_shape_table_rekey_mut")
    require_code(scanner, r"\bvisit_metadata_usize_slot\s*\(", "weak metadata rewrite")
    scanner_slot_apis = set(re.findall(r"\b(visit_[A-Za-z0-9_]*slot)\s*\(", scanner))
    if scanner_slot_apis != {"visit_metadata_usize_slot"}:
        raise CensusError(
            "descriptor scanner slot API allowlist failed: "
            + ", ".join(sorted(scanner_slot_apis))
        )

    layout_body = function_body(layout_visit, "visit_gc_layout_slot_descriptors")
    assert_before(
        layout_body,
        "child_slots.take_prefix_child_slot()",
        "synchronize_live_object_shape_descriptor_after_header_visit(",
        "authoritative header visit before descriptor mirror",
    )
    assert_before(
        layout_body,
        "try_read_tracked_gc_header(old_keys as usize)",
        "keys_array_len_capped_to_capacity(old_keys)",
        "array-header validation before descriptor fact read",
    )
    require_code(
        layout_body,
        r"\(\s*\*\s*keys_header\.as_ptr\s*\(\s*\)\s*\)\.obj_type\s*==\s*GC_TYPE_ARRAY",
        "descriptor fact capture exact array type",
    )

    ensure = function_body(shapes, "shape_descriptor_ensure_with_generation")
    assert_before(
        ensure,
        "inner.descriptors.insert",
        "inner.ids_by_facts.entry",
        "by-id descriptor before reverse accelerator",
    )
    sync = function_body(shapes, "publish_object_shape_from")
    assert_before(
        sync,
        "shape_descriptor_ensure",
        "(*obj).parent_class_id = id",
        "descriptor before ObjectHeader ShapeId",
    )
    # #8113 MINT-THEN-STAMP. With `field_count` deleted, the descriptor is the
    # only record of the live inline-slot bound, so a stamp-cleared window is a
    # window in which the collector traces ZERO payload slots. No publication
    # path may clear, and the only surviving `clear_object_shape_stamp` must be
    # test-only.
    for name in (
        "publish_object_shape_from",
        "publish_object_live_slot_count",
        "birth_publish_object_shape",
        "stamp_object_shape",
        "birth_stamp_object_shape",
    ):
        if "clear_object_shape_stamp" in function_body(shapes, name):
            raise CensusError(f"{name} clears the shape stamp: the live-slot bound has no mirror")
    if "clear_object_shape_stamp" in function_body(object_mod, "set_object_keys_array_with_live"):
        raise CensusError(
            "set_object_keys_array_with_live clears the shape stamp: "
            "the live-slot bound has no mirror"
        )
    if not re.search(
        r"#\[cfg\(test\)\]\s*\n\s*#\[inline\]\s*\n\s*pub\(crate\) unsafe fn clear_object_shape_stamp",
        shapes,
    ):
        raise CensusError("clear_object_shape_stamp escaped its #[cfg(test)] gate")
    retirement = function_body(shapes, "retain_key_count_versions")
    require_code(
        retirement,
        r"ids_by_keys\s*\.\s*remove\s*\(\s*&keys\s*\)",
        "keys-scoped descriptor lineage index",
    )
    if re.search(r"descriptors\s*\.\s*(?:iter|values|keys)\s*\(", retirement):
        raise CensusError("shape descriptor lineage repair scans the global descriptor table")
    if "descriptors.remove" in retirement:
        raise CensusError("live-key lineage repair eagerly deletes published descriptors")
    for name in ("shape_keys_grown", "shape_drop"):
        if "descriptors.remove" in function_body(shapes, name):
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
        "runtime_store_jsvalue_slot",
        "transition-cache count before value",
    )

    # Runtime guard contracts may consume ShapeId/descriptor facts, never the
    # compatibility ObjectHeader mirrors or a keys-pointer token.
    for name in (
        "method_direct_call_contract",
        "class_field_get_contract",
        "class_field_fast_contract",
        "class_field_set_contract",
    ):
        body = function_body(typed_guards, name)
        if re.search(r"expected_keys|\(\s*\*\s*obj\s*\)\s*\.\s*(?:keys_array|field_count|object_type)\b", body):
            raise CensusError(f"{name} reintroduced a legacy header guard fact")
        require_code(
            body,
            r"object_shape(?:_(?:id|descriptor))?\s*\(",
            f"{name} ShapeId authority",
        )

    for name in ("class_vtable_fast_guard", "js_native_call_method"):
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

    # RegExp identity lives in the GcHeader kind. No ObjectHeader payload word
    # or registry/magic conjunction may decide these ordinary-object forks.
    for name in ("object_is_regular", "object_is_shaped"):
        body = function_body(object_mod, name)
        require_code(body, r"obj_type\s*==\s*crate::gc::GC_TYPE_OBJECT", f"{name} GC kind")
        if re.search(r"regex_header_has_magic|object_type", body):
            raise CensusError(f"{name} reintroduced an old payload discriminator")
    regexp_alloc = function_body(regex_runtime, "js_regexp_new")
    require_code(
        regexp_alloc,
        r"gc_malloc\s*\([^;]*crate::gc::GC_TYPE_REGEXP",
        "RegExp dedicated GC birth kind",
    )
    expando_kind = function_body(exotic_expando, "exotic_expando_kind")
    require_code(
        expando_kind,
        r"crate::gc::GC_TYPE_REGEXP\s*=>\s*Some\s*\(\s*ExoticKind::RegExp",
        "RegExp expando dedicated kind",
    )
    regexp_get = function_body(get_field_tail, "get_field_by_name_object_tail")
    require_code(
        regexp_get,
        r"gc_type\s*==\s*crate::gc::GC_TYPE_REGEXP",
        "RegExp property dispatch dedicated kind",
    )
    if re.search(
        r"GC_TYPE_OBJECT[^{};]*is_regex_pointer|is_regex_pointer[^{};]*GC_TYPE_OBJECT",
        expando_kind + regexp_get,
    ):
        raise CensusError("RegExp dispatch reintroduced the former object-kind probe")

    read_miss = function_body(ic_miss, "js_object_get_field_ic_miss")
    for body, label in (
        (read_miss, "read PIC miss"),
        (function_body(put_value, "js_put_value_set_ic_miss"), "static write PIC miss"),
        (function_body(put_value, "dyn_ic_try_store"), "dynamic write PIC hit"),
        (function_body(put_value, "js_put_value_set_dyn_ic_miss"), "dynamic write PIC miss"),
    ):
        if re.search(r"else\s*\{\s*(?:keys|\(\s*\*\s*obj\s*\)\.keys_array)\s+as\s+u64", body):
            raise CensusError(f"{label} reintroduced a keys-pointer token")

    # Emitted guards may read exactly two header offsets: `class_id` @0 and the
    # ShapeId @4. Everything at or past 8 is a mirror (`keys_array` @8, `meta`
    # @16) that #8047 removes, and reading one as a shape fact is the bug this
    # census exists to catch.
    #
    # #8113 also fixed this arm's VACUITY. It used to match only
    # `add(..., "N")`, while all four functions below emit
    # `gep(I8, &p, &[(I64, "N")])` — so planting a keys-offset read left it
    # green. Both spellings are matched now, and each function must be shown to
    # read the ShapeId at all, so a guard that stops reading the header
    # entirely cannot pass by emitting nothing.
    for source, names in (
        (raw_class_guard, (
            "emit_class_field_loop_preheader_check",
            "emit_proven_shape_recheck",
            "emit_class_field_inline_precheck",
        )),
        (raw_element_guard, ("emit_element_shape_field_load",)),
    ):
        for name in names:
            body = function_body(source, name)
            # #8110 hardened this arm because an `add`-only pattern was
            # vacuous: these four guards build their header address with
            # `blk.gep(I8, &p, &[(I64, "N")])`. That coverage now lives in
            # `forbidden_header_offsets()` below, which matches BOTH spellings
            # and carries the post-#8113 offsets (keys_array @8, meta @16);
            # the old 0/12/16 list described a layout this commit deletes --
            # offset 0 is now `class_id` and legitimately readable. The
            # `require_code` on offset 4 keeps "emits nothing" from passing.
            if re.search(r"expected_keys", body):
                raise CensusError(f"{name} emits a removed ObjectHeader fact")
            if forbidden_header_offsets(body):
                raise CensusError(f"{name} emits a removed ObjectHeader fact")
            require_code(
                body,
                r"\(\s*I64\s*,\s*\"4\"\s*\)",
                f"{name} reads the authoritative ShapeId at header offset 4",
            )

    generic_body = function_body(raw_generic_pic, "lower_generic_property_get")
    if re.search(r"add\s*\(\s*I64\s*,\s*&obj_handle\s*,\s*\"(?:8|16)\"", generic_body):
        raise CensusError("generic read PIC emits a removed ObjectHeader fact")
    require_code(
        generic_body,
        r"add\s*\(\s*I64\s*,\s*&obj_handle\s*,\s*\"4\"\s*\)",
        "generic read PIC reads the authoritative ShapeId at header offset 4",
    )
    require_code(
        generic_body,
        r"select\s*\(\s*I1\s*,\s*&is_stamp\s*,\s*I64\s*,\s*&id_token\s*,\s*\"0\"\s*\)",
        "generic read PIC invalid-id fail-closed token",
    )
    for name in ("lower_put_value_static_write_ic", "lower_put_value_dyn_ic_inline"):
        body = function_body(raw_write_pics, name)
        if re.search(r"add\s*\(\s*I64\s*,\s*&(safe_target|t_handle)\s*,\s*\"(?:8|16)\"", body):
            raise CensusError(f"{name} emits a removed ObjectHeader fact")
        require_code(
            body,
            r"add\s*\(\s*I64\s*,\s*&(?:safe_target|t_handle)\s*,\s*\"4\"\s*\)",
            f"{name} reads the authoritative ShapeId at header offset 4",
        )

    require_code(gc_types, r"GC_TYPE_REGEXP\s*:\s*u8", "RegExp external discriminator")
    regexp_info_match = re.search(
        r"gc_type_info_entry\(\s*GC_TYPE_REGEXP\b[\s\S]*?\n\s*\)\s*\)",
        gc_types,
    )
    if not regexp_info_match:
        raise CensusError("shape descriptor authority surface missing: RegExp type metadata")
    regexp_info = regexp_info_match.group(0)
    require_code(
        regexp_info,
        r"GcMoveHookKind::RegExpSideTables",
        "RegExp address-owned relocation hook",
    )
    require_code(
        regexp_info,
        r"GcFinalizeHookKind::RegExpSideTables",
        "RegExp malloc-finalize side-table hook",
    )
    if "OBJ_FLAG_CLASS_OBJECT" in gc_types + class_guard + element_guard + write_pics:
        raise CensusError("class kind reintroduced a GcHeader layout-bit alias")
    assert_header_fields(object_mod)
    class_probe = function_body(object_mod, "object_is_regular")
    require_code(
        class_probe,
        r"ShapeObjectKind::Ordinary",
        "ordinary-object descriptor kind authority",
    )


def forbidden_header_offsets(body: str) -> list[str]:
    """Positive `ObjectHeader` byte offsets an emitted guard must not address.

    Matches both emitter spellings: a gep index tuple `(I64, "N")` and an
    `add(I64, &base, "N")`. `sub(...)` is deliberately NOT matched — it is how
    the GcHeader bytes at -8/-7/-6 are reached — and neither is a negative
    literal.
    """
    forbidden = {"8", "16"}
    gep = re.findall(r'\(\s*I64\s*,\s*"(-?\d+)"\s*\)', body)
    add = re.findall(r'\.add\s*\(\s*I64\s*,\s*&\w[\w.]*\s*,\s*"(-?\d+)"\s*\)', body)
    return sorted({off for off in gep + add if off in forbidden})


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

    inverted_gc = dict(sources)
    path = "crates/perry-runtime/src/gc/layout_slot_visit.rs"
    inverted_gc[path] = swap_once(
        inverted_gc[path],
        "child_slots.take_prefix_child_slot()",
        "synchronize_live_object_shape_descriptor_after_header_visit(",
    )
    expect_rejected("descriptor before header visit", lambda: assert_authority_surfaces(inverted_gc))

    inverted_publication = dict(sources)
    path = "crates/perry-runtime/src/object/shapes.rs"
    publication_body = function_body(
        inverted_publication[path], "publish_object_shape_from"
    )
    inverted_body = swap_once(
        publication_body,
        "shape_descriptor_ensure_with_generation(",
        "(*obj).parent_class_id = id",
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
        unscoped_retirement[path], "retain_key_count_versions"
    )
    unscoped_body, substitutions = re.subn(
        r"ids_by_keys\s*\.\s*remove\s*\(\s*&keys\s*\)",
        "ids_by_keys.get(&keys).cloned()",
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

    legacy_ir = dict(sources)
    path = "crates/perry-codegen/src/expr/property_get/generic_dispatch.rs"
    legacy_body, substitutions = re.subn(
        r'add\(I64, &obj_handle, "4"\)',
        'add(I64, &obj_handle, "16")',
        legacy_ir[path],
        count=1,
    )
    if substitutions != 1:
        raise CensusError("legacy emitted-offset sabotage fixture missing")
    legacy_ir[path] = legacy_body
    expect_rejected(
        "legacy keys-header offset in emitted PIC",
        lambda: assert_authority_surfaces(legacy_ir),
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
        "    pub keys_array: *mut ArrayHeader,",
        "    pub field_count: u32,\n    pub keys_array: *mut ArrayHeader,",
        1,
    )
    expect_rejected(
        "re-added ObjectHeader payload word",
        lambda: assert_authority_surfaces(readded_field),
    )

    # #8113: a re-introduced clear-then-remint window.
    cleared_publication = dict(sources)
    path = "crates/perry-runtime/src/object/shapes.rs"
    cleared_body = function_body(cleared_publication[path], "publish_object_live_slot_count")
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


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "--emit-baseline",
        action="store_true",
        help="print the current exact multiset for reviewed baseline refresh",
    )
    args = parser.parse_args()
    run_literal_lexer_selftest()
    sources = rust_sources()
    observed = observed_census(sources)
    if args.emit_baseline:
        print(json.dumps(observed, indent=2, sort_keys=True))
        return
    baseline = json.loads(BASELINE_PATH.read_text(encoding="utf-8"))
    compare_exact_census(observed, baseline)
    assert_authority_surfaces(sources)
    run_sabotage_selftests(sources, baseline)
    print(json.dumps(observed["summary"], indent=2, sort_keys=True))


if __name__ == "__main__":
    main()
