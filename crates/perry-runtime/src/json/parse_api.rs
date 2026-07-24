//! Public FFI entry points for JSON parsing.
//!
//! Includes `js_json_parse`, the null-tolerant `js_json_parse_or_null` shim,
//! the tape-mode selector, and the schema-directed `js_json_parse_typed_array`
//! fast path used by `JSON.parse<T[]>(blob)`.

use super::*;
use crate::{js_string_from_bytes, JSValue, StringHeader};

/// ECMA-262 JSON.parse step 1: `JText = ? ToString(text)`. Coerces the
/// argument to a heap string with full ToString semantics (so `null` → "null",
/// `123` → "123", `true` → "true", etc.), but — unlike `String(x)` — throws a
/// TypeError for a Symbol argument, matching `ToString(symbol)`. Used by the
/// `JSON.parse` codegen arm. (#4578)
#[no_mangle]
pub extern "C" fn js_json_text_to_string(value: f64) -> *mut StringHeader {
    if unsafe { crate::symbol::js_is_symbol(value) != 0 } {
        crate::collection_iter::throw_type_error("Cannot convert a Symbol value to a string");
    }
    crate::builtins::js_string_coerce(value)
}

// Anchor so the auto-optimize bitcode rebuild doesn't dead-strip this
// codegen-only `#[no_mangle]` (see KEEP_RAW_JSON in json/raw_json.rs).
#[used]
static KEEP_JSON_TEXT_TO_STRING: extern "C" fn(f64) -> *mut StringHeader = js_json_text_to_string;

// ─── JSON.parse ───────────────────────────────────────────────────────────────

/// JSON.parse(text) shim that returns `null` for a null input instead
/// of throwing `Unexpected end of JSON input`. Used by codegen dispatch
/// rows whose runtime FFI returns `*mut StringHeader` containing JSON
/// for the success path and a null pointer for the failure path (e.g.
/// `jwt.verify` on a bad signature). User code expects a null-return,
/// not an uncaught exception that aborts the process. Issue #927.
///
/// # Safety
///
/// `text_ptr` must be null or a Perry-runtime `StringHeader`.
#[no_mangle]
pub unsafe extern "C" fn js_json_parse_or_null(text_ptr: *const StringHeader) -> JSValue {
    if text_ptr.is_null() {
        return JSValue::null();
    }
    js_json_parse(text_ptr)
}

#[cfg(test)]
pub(crate) unsafe fn test_json_parse_direct(text_ptr: *const StringHeader) -> JSValue {
    assert!(!text_ptr.is_null());
    let len = (*text_ptr).byte_len as usize;
    let data_ptr = (text_ptr as *const u8).add(std::mem::size_of::<StringHeader>());
    let bytes = std::slice::from_raw_parts(data_ptr, len);

    crate::gc::gc_suppress();
    let text_root = parse_root_push(JSValue::string_ptr(text_ptr as *mut StringHeader));
    let mut parser = DirectParser::new(bytes);
    let result = parser.parse_value();
    parse_root_push(result);
    crate::gc::gc_unsuppress();
    parse_root_restore(text_root);
    result
}

fn syntax_error_value(message: &str) -> f64 {
    let msg_ptr = js_string_from_bytes(message.as_ptr(), message.len() as u32);
    let err = crate::error::js_syntaxerror_new(msg_ptr);
    f64::from_bits(JSValue::pointer(err as *const u8).bits())
}

fn throw_syntax_error(message: &str) -> ! {
    crate::exception::js_throw(syntax_error_value(message))
}

fn is_json_null_literal(bytes: &[u8]) -> bool {
    let Some(start) = bytes
        .iter()
        .position(|b| !matches!(b, b' ' | b'\t' | b'\n' | b'\r'))
    else {
        return false;
    };
    let end = bytes
        .iter()
        .rposition(|b| !matches!(b, b' ' | b'\t' | b'\n' | b'\r'))
        .map(|idx| idx + 1)
        .unwrap_or(start);
    &bytes[start..end] == b"null"
}

/// Non-throwing JSON parse entry for APIs that must reject a Promise rather than
/// synchronously throwing through `JSON.parse`'s FFI boundary.
///
/// # Safety
///
/// `text_ptr` must be null or a Perry-runtime `StringHeader`.
pub unsafe fn js_json_parse_result(text_ptr: *const StringHeader) -> Result<JSValue, f64> {
    if text_ptr.is_null() {
        return Err(syntax_error_value("Unexpected end of JSON input"));
    }
    let len = (*text_ptr).byte_len as usize;
    let data_ptr = (text_ptr as *const u8).add(std::mem::size_of::<StringHeader>());
    let bytes = std::slice::from_raw_parts(data_ptr, len);

    if len == 0 {
        return Err(syntax_error_value("Unexpected end of JSON input"));
    }

    // Validate without constructing a second full JSON tree. The Perry parser
    // below owns the runtime representation; asking serde_json for `Value`
    // here doubled peak live memory (and allocation work) on large payloads.
    if let Err(err) = serde_json::from_slice::<serde::de::IgnoredAny>(bytes) {
        return Err(syntax_error_value(&format!("JSON parse error: {}", err)));
    }

    crate::gc::gc_collect_pending_suppressed_parse();
    crate::gc::gc_check_trigger();
    crate::gc::gc_suppress();

    let text_root = parse_root_push(JSValue::string_ptr(text_ptr as *mut StringHeader));
    let mut parser = DirectParser::new(bytes);
    let result = parser.parse_value();
    parse_root_push(result);
    crate::gc::gc_unsuppress();
    crate::gc::gc_bump_malloc_trigger();
    crate::gc::gc_schedule_parse_boundary_collection_if_pressure();
    parse_root_restore(text_root);

    PARSE_KEY_CACHE.with(|c| {
        let cache = c.borrow();
        if cache.len() > 4096 {
            drop(cache);
            c.borrow_mut().clear();
            clear_parse_key_ring();
        }
    });

    if result.is_null() && !is_json_null_literal(bytes) {
        let preview_len = len.min(50);
        let preview = std::str::from_utf8(&bytes[..preview_len]).unwrap_or("???");
        let msg = format!("JSON parse error: Unexpected token: {}", preview);
        return Err(syntax_error_value(&msg));
    }

    Ok(result)
}

/// JSON.parse(text) -> any
///
/// Uses a direct recursive-descent parser that constructs Perry JSValues
/// without any intermediate representation.
#[no_mangle]
pub unsafe extern "C" fn js_json_parse(text_ptr: *const StringHeader) -> JSValue {
    if text_ptr.is_null() {
        throw_syntax_error("Unexpected end of JSON input");
    }
    let len = (*text_ptr).byte_len as usize;
    let data_ptr = (text_ptr as *const u8).add(std::mem::size_of::<StringHeader>());
    let bytes = std::slice::from_raw_parts(data_ptr, len);

    if len == 0 {
        throw_syntax_error("Unexpected end of JSON input");
    }
    // Keep serde_json's strict syntax validation, but discard tokens as they
    // are read instead of allocating an intermediate `serde_json::Value`
    // immediately before Perry builds its own tree.
    if let Err(err) = serde_json::from_slice::<serde::de::IgnoredAny>(bytes) {
        throw_syntax_error(&format!("JSON parse error: {}", err));
    }

    crate::gc::gc_collect_pending_suppressed_parse();

    // Issue #179 Step 2 Phase 1 → default-on: tape-based lazy parse
    // is now the default for top-level arrays on blobs larger than
    // the size threshold. v0.5.209 runtime adaptive handling (walk
    // cursor + cumulative-walk threshold + sparse cache + force-
    // materialize-on-mutate) means lazy is safe for non-tiny blobs.
    // The lower bound keeps tape-build overhead off sub-1 KB parses;
    // #437 briefly raised it for a 21 KB iterate-all fixture, but
    // #1090 restores 1 KB so RSS-sensitive parse-churn loops use the
    // tape path once payloads are larger than the direct tiny case.
    //
    // Escape hatches: `PERRY_JSON_TAPE=0` forces the direct parser
    // for every parse (correctness fallback if a workload hits an
    // unaudited code path on the lazy side). `PERRY_JSON_TAPE=1`
    // forces tape for every parse including small ones (useful for
    // testing). Any other value is treated as "auto" (the default).
    // Lazy parse is a win on workloads that touch only a subset of
    // a parsed top-level array — the tape build cost is ~one-shot
    // O(n) but each unread element saves the full subtree
    // materialization. For workloads that iterate the whole array
    // (the canonical "filter all records, stringify the result"
    // shape — `benchmarks/honest_bench/workloads/1_json_pipeline`
    // for example), lazy is strictly slower than direct: tape walk
    // + sparse-cache management on every access plus a forced
    // materialize at the end is more work than the direct parser's
    // single tree build. The cumulative walk-steps trigger in
    // `lazy_get` only catches *random* access, not sequential.
    //
    // The auto-mode size window: lazy fires at 1 KB and above (tiny
    // parses don't pay the tape build) and below 16 MB (very large
    // blobs are dominated by the iterate-all idiom in practice —
    // 108 MB honest_bench full fixture, server log dumps, dataset
    // ETL — and the direct parser's tree-build is faster end-to-
    // end than tape + materialize). The upper bound keeps those
    // very large cases direct while the restored lower bound keeps
    // JSON churn bounded for sub-collector-scale payloads.
    //
    // Escape hatch via PERRY_JSON_TAPE=1 (force lazy regardless
    // of size, useful for testing) / =0 (force direct, useful as
    // a correctness fallback).
    const LAZY_MIN_BLOB_BYTES: usize = 1024;
    const LAZY_MAX_BLOB_BYTES: usize = 16 * 1024 * 1024;
    let tape_mode = tape_mode_from_env();
    let use_tape = match tape_mode {
        TapeMode::ForceOn => true,
        TapeMode::ForceOff => false,
        TapeMode::Auto => {
            // Tape laziness currently pays off only for top-level arrays:
            // object/scalar roots materialize eagerly after building the tape,
            // so they do two parses' worth of work. Peek the first meaningful
            // byte and keep object-root API payloads on the direct parser.
            (LAZY_MIN_BLOB_BYTES..=LAZY_MAX_BLOB_BYTES).contains(&len)
                && bytes
                    .iter()
                    .copied()
                    .find(|b| !matches!(b, b' ' | b'\t' | b'\n' | b'\r'))
                    == Some(b'[')
        }
    };
    if use_tape {
        if let Some(result) = try_parse_via_tape(text_ptr, bytes) {
            return result;
        }
        // Malformed input or non-array top-level — fall through to
        // direct parser, which has the full error-reporting path
        // and handles non-array roots.
    }

    // #64 follow-up: opportunistic pre-parse cleanup. When parse runs in a
    // tight loop (e.g. `for (let i=0; i<N; i++) JSON.parse(blob);`), each
    // call suppresses GC for its duration, and the post-parse malloc-trigger
    // bump defers collection of the PREVIOUS iteration's now-dead tree.
    // Garbage accumulates until the arena trigger fires — typically during
    // the next stringify, producing one 100ms+ pause that walks every dead
    // block from every iteration. Calling `gc_check_trigger` here (before
    // suppression) lets the trigger fire normally between iterations so
    // garbage is shed incrementally. The new <10%-freed-doubles-step rule
    // in `gc_check_trigger` protects adversarial cases (previous stringify
    // result strings sharing blocks with interned keys) from retrigger
    // thrash when block-persistence keeps everything alive.
    crate::gc::gc_check_trigger();

    // Suppress GC for the duration of the parse. Parse is synchronous and
    // roots all intermediates in PARSE_ROOTS, so no collection is needed
    // until we're done. This eliminates O(n*m) overhead from mid-parse GC
    // cycles walking an ever-growing live set (issue #59).
    crate::gc::gc_suppress();

    let text_root = parse_root_push(JSValue::string_ptr(text_ptr as *mut StringHeader));

    let mut parser = DirectParser::new(bytes);
    let result = parser.parse_value();
    parse_root_push(result);

    // Re-enable GC and rebaseline triggers while the result is still
    // rooted. Tiny parse-churn pressure may collect here; keeping the
    // parse roots until after the bump protects the value being returned.
    crate::gc::gc_unsuppress();
    crate::gc::gc_bump_malloc_trigger();
    crate::gc::gc_schedule_parse_boundary_collection_if_pressure();
    parse_root_restore(text_root);

    // Keep key intern cache across parses — scan_parse_roots marks cached
    // strings as GC roots so they survive collection. This saves ~10k
    // gc_malloc calls per repeated parse of homogeneous JSON (same keys).
    // Cap at 4096 entries to bound memory for varied-schema workloads.
    PARSE_KEY_CACHE.with(|c| {
        let cache = c.borrow();
        if cache.len() > 4096 {
            drop(cache);
            c.borrow_mut().clear();
            clear_parse_key_ring();
        }
    });

    // If parser didn't consume meaningful input (result is null and input wasn't "null"),
    // the input was invalid JSON — throw SyntaxError
    if result.is_null() {
        let is_literal_null = len >= 4 && bytes.starts_with(b"null");
        if !is_literal_null {
            let preview_len = len.min(50);
            let preview = std::str::from_utf8(&bytes[..preview_len]).unwrap_or("???");
            let msg = format!("JSON parse error: Unexpected token: {}", preview);
            throw_syntax_error(&msg);
        } else if parser.has_trailing_content() {
            // Literal `null` followed by trailing tokens (`JSON.parse("null x")`)
            // — reject like any other trailing-token case.
            throw_syntax_error("Unexpected non-whitespace character after JSON");
        }
    } else if parser.has_trailing_content() {
        // A valid value was parsed but non-whitespace input remains
        // (`JSON.parse("{}x")`, `JSON.parse("1 2")`). Node rejects trailing
        // tokens with a SyntaxError; trailing whitespace is allowed.
        crate::exception::js_throw(syntax_error_value(
            "Unexpected non-whitespace character after JSON",
        ));
    }

    result
}

/// v0.5.210: tape-mode selector. Cached at first JSON.parse so we
/// pay the env-var lookup once per process, not once per parse.
#[derive(Copy, Clone)]
pub(crate) enum TapeMode {
    Auto,
    ForceOn,
    ForceOff,
}

/// SSO Step 1 test gate. `PERRY_SSO_FORCE=1` (or `on`/`true`) flips
/// `DirectParser::parse_string_value` to emit inline SSO values for
/// strings of length ≤ 5. Used by the migration test suite to
/// exercise every stringify / equality / compare consumer arm
/// across both representations. Cached so the per-parse-call cost
/// is one relaxed atomic load.
// #854: SSO-emission gate retained for the JSON parse fast path
#[allow(dead_code)]
pub(crate) fn sso_emit_enabled() -> bool {
    use std::sync::OnceLock;
    static CACHED: OnceLock<bool> = OnceLock::new();
    *CACHED.get_or_init(|| {
        matches!(
            std::env::var("PERRY_SSO_FORCE").as_deref(),
            Ok("1") | Ok("on") | Ok("true")
        )
    })
}

pub(crate) fn tape_mode_from_env() -> TapeMode {
    use std::sync::OnceLock;
    static CACHED: OnceLock<TapeMode> = OnceLock::new();
    *CACHED.get_or_init(|| match std::env::var("PERRY_JSON_TAPE").as_deref() {
        Ok("0") | Ok("off") | Ok("false") => TapeMode::ForceOff,
        Ok("1") | Ok("on") | Ok("true") => TapeMode::ForceOn,
        _ => TapeMode::Auto,
    })
}

/// Issue #179 Step 2 Phase 1: tape-path entry. Builds a tape from
/// the input bytes, then materializes the full JSValue tree via
/// `json_tape::materialize`. Returns `None` on malformed input so
/// the caller can fall through to the direct parser.
///
/// Wraps the tape path in the same GC-safety contract as the direct
/// parser (pending parse-boundary collection → gc_check_trigger →
/// suppress → parse → unsuppress → bump malloc trigger + cache trim) so
/// it's a drop-in replacement behind the feature flag.
pub(crate) unsafe fn try_parse_via_tape(
    text_ptr: *const StringHeader,
    bytes: &[u8],
) -> Option<JSValue> {
    crate::json_tape::with_built_tape(bytes, |tape_entries| {
        crate::gc::gc_collect_pending_suppressed_parse();
        crate::gc::gc_check_trigger();
        crate::gc::gc_suppress();
        let text_root = parse_root_push(JSValue::string_ptr(text_ptr as *mut StringHeader));

        // Phase 2: if the top-level value is an array, return a lazy
        // array header instead of materializing the tree. Every other
        // shape (objects, scalars) still materializes eagerly — this
        // commit's scope is top-level arrays only (the shape that
        // dominates `bench_json_roundtrip` and most realistic JSON.parse
        // workloads). Extending to top-level objects in a follow-up is a
        // straightforward mirror of the same construction.
        let result = if !tape_entries.is_empty()
            && tape_entries[0].kind == crate::json_tape::KIND_ARR_START
        {
            let len = crate::json_tape::count_array_length(tape_entries, 0);
            let hdr = crate::json_tape::alloc_lazy_array(tape_entries, 0, len, text_ptr);
            JSValue::object_ptr(hdr as *mut u8)
        } else {
            crate::json_tape::materialize_from_idx(tape_entries, bytes, 0)
        };
        parse_root_push(result);

        crate::gc::gc_unsuppress();
        crate::gc::gc_bump_malloc_trigger();
        parse_root_restore(text_root);

        PARSE_KEY_CACHE.with(|c| {
            let cache = c.borrow();
            if cache.len() > 4096 {
                drop(cache);
                c.borrow_mut().clear();
                clear_parse_key_ring();
            }
        });

        result
    })
}

// ─── JSON.parse<T[]>: schema-directed typed parse ─────────────────────────────

/// Issue #179 typed-parse plan, Step 1b. Entry point for
/// `JSON.parse<T[]>(blob)` where T is an object type whose field names
/// are known at codegen time.
///
/// `packed_keys` is null-separated UTF-8 field names in declared order:
/// `b"id\0name\0value\0"`. `field_count` is the number of fields
/// (== number of `\0` separators).
///
/// Runtime behavior is identical to `js_json_parse(text_ptr)` —
/// semantically the same JSON, same JSValue tree, same Node parity.
/// The specialization just skips:
/// - Per-record shape-cache lookup (shape built once per call)
/// - Per-field `PARSE_KEY_CACHE` hash when fields arrive in declared
///   order (the common case for stringify output and most machine-
///   generated JSON)
/// - Per-field transition-cache dance inside `js_object_set_field_by_name`
///   for in-order fields (direct field-index write)
///
/// Out-of-order, extra, or missing fields all fall through to the
/// generic named-setter path — correctness-preserving.
///
/// On input shape mismatch (top-level isn't an array, records aren't
/// objects), also falls through to the generic parser. No user-
/// visible difference from `JSON.parse(blob) as T[]`.
#[no_mangle]
pub unsafe extern "C" fn js_json_parse_typed_array(
    text_ptr: *const StringHeader,
    packed_keys: *const u8,
    packed_keys_len: u32,
    field_count: u32,
) -> JSValue {
    if text_ptr.is_null() {
        // Fall through to generic (which will throw the standard error).
        return js_json_parse(text_ptr);
    }
    let len = (*text_ptr).byte_len as usize;
    if len == 0 {
        return js_json_parse(text_ptr);
    }
    let data_ptr = (text_ptr as *const u8).add(std::mem::size_of::<StringHeader>());
    let bytes = std::slice::from_raw_parts(data_ptr, len);

    // Build the shape hint once. The keys_array + pre-interned key
    // pointers are owned by longlived arena + shape-cache structures,
    // so they outlive the parse and survive any intervening GC.
    let shape = match build_shape_hint(packed_keys, packed_keys_len, field_count) {
        Some(s) => s,
        None => return js_json_parse(text_ptr),
    };

    // Same pre-parse cleanup + GC suppression as `js_json_parse` —
    // keeps the typed path on the same GC-safety contract.
    crate::gc::gc_collect_pending_suppressed_parse();
    crate::gc::gc_check_trigger();
    crate::gc::gc_suppress();
    let text_root = parse_root_push(JSValue::string_ptr(text_ptr as *mut StringHeader));

    let mut parser = DirectParser::with_shape(bytes, shape);
    let result = parser.parse_array_typed();
    parse_root_push(result);

    crate::gc::gc_unsuppress();
    crate::gc::gc_bump_malloc_trigger();
    parse_root_restore(text_root);

    PARSE_KEY_CACHE.with(|c| {
        let cache = c.borrow();
        if cache.len() > 4096 {
            drop(cache);
            c.borrow_mut().clear();
            clear_parse_key_ring();
        }
    });

    if result.is_null() {
        let is_literal_null = len >= 4 && bytes.starts_with(b"null");
        if !is_literal_null {
            let preview_len = len.min(50);
            let preview = std::str::from_utf8(&bytes[..preview_len]).unwrap_or("???");
            let msg = format!("JSON parse error: Unexpected token: {}", preview);
            // Throw a real `SyntaxError` (not a bare string) to match Node's
            // error identity for invalid JSON.
            crate::exception::js_throw(syntax_error_value(&msg));
        }
    }

    result
}

/// Build the one-per-call shape hint: intern key strings into
/// `PARSE_KEY_CACHE` (longlived arena) and build a shared
/// `keys_array` via the existing `js_build_class_keys_array` path so
/// `scan_shape_cache_roots` keeps it marked. Returns `None` if
/// `packed_keys` is malformed (no separators, unexpected count).
pub(crate) unsafe fn build_shape_hint(
    packed_keys: *const u8,
    packed_keys_len: u32,
    field_count: u32,
) -> Option<ObjectShapeHint> {
    if packed_keys.is_null() || field_count == 0 {
        return None;
    }
    let packed = std::slice::from_raw_parts(packed_keys, packed_keys_len as usize);
    // Same parsing as `js_build_class_keys_array`: split on `\0`,
    // drop empties.
    let keys: Vec<&[u8]> = packed
        .split(|&b| b == 0)
        .filter(|s| !s.is_empty())
        .collect();
    if keys.len() != field_count as usize {
        return None;
    }
    // Intern each key via PARSE_KEY_CACHE so the pointers are shared
    // with the generic-parse path — critical for the transition cache
    // to treat them as identical during slow-path field sets.
    let mut expected_keys: Vec<*const StringHeader> = Vec::with_capacity(keys.len());
    for key_bytes in &keys {
        let cached = PARSE_KEY_CACHE.with(|c| c.borrow().get(*key_bytes).copied());
        let ptr = if let Some(p) = cached {
            p
        } else {
            let p = crate::string::js_string_from_bytes_longlived(
                key_bytes.as_ptr(),
                key_bytes.len() as u32,
            );
            PARSE_KEY_CACHE.with(|c| {
                c.borrow_mut().insert(key_bytes.to_vec(), p);
            });
            p
        };
        expected_keys.push(ptr);
    }

    // Build the keys_array via the existing class-shape path. We
    // derive a class_id by hashing packed_keys so repeated typed-parse
    // calls with the same shape reuse the same keys_array (cache hit).
    let class_id = shape_hash(packed) as u32;
    let keys_array = crate::object::js_build_class_keys_array(
        class_id,
        field_count,
        packed_keys,
        packed_keys_len,
    );

    Some(ObjectShapeHint {
        expected_keys,
        keys_array,
        field_count,
    })
}

#[inline]
pub(crate) fn shape_hash(bytes: &[u8]) -> u64 {
    // FNV-1a, matching the style Perry uses elsewhere for shape
    // identity. A collision just means two distinct shapes share a
    // class_id in the shape cache — the cache is content-compared on
    // miss so no correctness issue, just a modest re-build cost.
    let mut h: u64 = 0xcbf29ce484222325;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    // Nonzero class_id (0 is reserved for plain objects).
    h | 0x8000_0000_0000_0000
}
