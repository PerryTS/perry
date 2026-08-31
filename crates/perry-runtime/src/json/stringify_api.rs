//! Public FFI entry points for `JSON.stringify` (2-arg form), the
//! specialized scalar stringify helpers, JSON validation, and the
//! `js_json_get_*` accessors used by legacy callers.

use super::*;
use crate::{js_string_from_bytes, JSValue, StringHeader};

/// Generic JSON.stringify that handles any JSValue
/// Takes a f64 (NaN-boxed JSValue) and a type_hint (0=unknown, 1=object, 2=array)
/// Returns a string pointer
#[no_mangle]
/// Issue #179 Step 2 Phase 3: if `value` is a lazy array that's
/// already been materialized (indexed access forced
/// `force_materialize_lazy`), return a JSValue pointing at the
/// materialized `ArrayHeader` tree instead of the `LazyArrayHeader`.
/// The generic tree-walk stringifier would otherwise read lazy-
/// header fields (magic, root_idx, blob_str, ...) as if they were
/// element f64s and crash on the first bogus pointer deref. No-op
/// for non-lazy values and for lazy values whose `materialized` is
/// still null (the lazy-stringify fast path handles those).
pub(crate) unsafe fn redirect_lazy_to_materialized(value: f64) -> f64 {
    let bits = value.to_bits();
    let top16 = bits >> 48;
    let ptr = if top16 == 0x7FFD {
        (bits & 0x0000_FFFF_FFFF_FFFF) as *const u8
    } else {
        return value;
    };
    // A synthetic handle-band id (fetch/Blob/socket/… < HANDLE_BAND_MAX) is not
    // a real heap object; `gc_header = ptr - 8` would deref unmapped memory →
    // SIGSEGV on e.g. `JSON.stringify(new Blob())` (#6240/#6241). Nor is an
    // above-the-band payload automatically real — require a GC-tracked
    // allocation (dereference-free page-map/registry lookups) before the
    // header read, the same rule `is_object_pointer` uses.
    if ptr.is_null() || !stringify::ptr_is_tracked_heap_object(ptr) {
        return value;
    }
    let gc_header = ptr.sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
    if (*gc_header).obj_type != crate::gc::GC_TYPE_LAZY_ARRAY {
        return value;
    }
    let lazy = ptr as *const crate::json_tape::LazyArrayHeader;
    if (*lazy).magic != crate::json_tape::LAZY_ARRAY_MAGIC {
        return value;
    }
    if (*lazy).materialized.is_null() {
        return value;
    }
    f64::from_bits(JSValue::object_ptr((*lazy).materialized as *mut u8).bits())
}

/// Return one validated JSON number token and its exclusive end offset.
/// The tape builder already accepted this input; the checks here keep the
/// lazy stringify path fail-closed if a retained blob or tape is corrupted.
fn json_number_token(bytes: &[u8], start: usize) -> Option<(usize, &[u8])> {
    let mut end = start;
    if bytes.get(end) == Some(&b'-') {
        end += 1;
    }
    match bytes.get(end) {
        Some(b'0') => end += 1,
        Some(b'1'..=b'9') => {
            end += 1;
            while bytes.get(end).is_some_and(u8::is_ascii_digit) {
                end += 1;
            }
        }
        _ => return None,
    }
    if bytes.get(end) == Some(&b'.') {
        end += 1;
        let fraction_start = end;
        while bytes.get(end).is_some_and(u8::is_ascii_digit) {
            end += 1;
        }
        if end == fraction_start {
            return None;
        }
    }
    if matches!(bytes.get(end), Some(b'e' | b'E')) {
        end += 1;
        if matches!(bytes.get(end), Some(b'+' | b'-')) {
            end += 1;
        }
        let exponent_start = end;
        while bytes.get(end).is_some_and(u8::is_ascii_digit) {
            end += 1;
        }
        if end == exponent_start {
            return None;
        }
    }
    Some((end, bytes.get(start..end)?))
}

/// Small source-form integers are already their canonical NumberToString
/// spelling and are exactly representable as `f64`. This avoids reparsing and
/// formatting the overwhelmingly common ID/index case while inspecting a lazy
/// array for number spellings that need normalization.
fn is_trivially_canonical_json_integer(token: &[u8]) -> bool {
    let (negative, digits) = match token.strip_prefix(b"-") {
        Some(digits) => (true, digits),
        None => (false, token),
    };
    !digits.is_empty()
        && digits.len() <= 15
        && digits.iter().all(u8::is_ascii_digit)
        && !(negative && digits == b"0")
}

/// Normalize number spellings in an otherwise byte-copyable lazy JSON array.
/// `JSON.parse` converts every number token to an IEEE-754 double, so the
/// stringify shortcut must not preserve source spellings such as `1e-400`,
/// `1.0`, or `9007199254740993`. Allocate a rewritten buffer only when a token
/// differs from the canonical formatting of the `f64` it denotes.
fn normalize_lazy_json_numbers<'a>(
    tape: &[crate::json_tape::TapeEntry],
    blob: &'a [u8],
    root_start: usize,
    root_end: usize,
) -> Option<std::borrow::Cow<'a, [u8]>> {
    let original = blob.get(root_start..root_end)?;
    let mut rewritten: Option<Vec<u8>> = None;
    let mut copied_until = root_start;
    let mut canonical = String::new();

    for entry in tape {
        if entry.kind != crate::json_tape::KIND_NUMBER {
            continue;
        }
        let number_start = entry.offset as usize;
        if number_start < root_start || number_start >= root_end {
            continue;
        }
        let (number_end, token) = json_number_token(blob, number_start)?;
        if number_end > root_end {
            return None;
        }
        if is_trivially_canonical_json_integer(token) {
            continue;
        }

        let value = std::str::from_utf8(token).ok()?.parse::<f64>().ok()?;
        canonical.clear();
        unsafe { stringify::write_number(&mut canonical, value) };
        if canonical.as_bytes() == token {
            continue;
        }

        let output = rewritten.get_or_insert_with(|| Vec::with_capacity(original.len()));
        if number_start < copied_until {
            return None;
        }
        output.extend_from_slice(blob.get(copied_until..number_start)?);
        output.extend_from_slice(canonical.as_bytes());
        copied_until = number_end;
    }

    if let Some(mut output) = rewritten {
        output.extend_from_slice(blob.get(copied_until..root_end)?);
        Some(std::borrow::Cow::Owned(output))
    } else {
        Some(std::borrow::Cow::Borrowed(original))
    }
}

/// Issue #179 Phase 4: lazy-stringify fast path. If `value` is a
/// lazy-parse top-level array whose `materialized` is still null (no
/// indexed access or mutation has forced tree build), memcpy the
/// original blob bytes into a fresh string — no tree walk, no
/// escape handling. Returns `None` if `value` is not a
/// tape-backed-and-unmutated lazy array, in which case the caller
/// falls through to the generic stringify path.
///
/// Correctness invariant: if the lazy value is unmutated, the bytes
/// spanning `[root.offset .. root_end.offset+1]` in the original
/// blob are exactly what `JSON.stringify` would produce for that
/// value (modulo whitespace the user's original blob may contain —
/// `JSON.stringify` never emits whitespace for the 2-arg form, so
/// this is only correct when the blob came from `JSON.stringify` or
/// is otherwise whitespace-free in the array span). Number tokens are
/// normalized separately: their source spelling is preserved only when it
/// matches the canonical formatting of the `f64` produced by `JSON.parse`.
pub(crate) unsafe fn try_stringify_lazy_array(value: f64) -> Option<*mut StringHeader> {
    let bits = value.to_bits();
    let top16 = bits >> 48;
    let maybe_ptr = if top16 == 0x7FFD {
        // POINTER_TAG NaN-box: lower 48 bits are the user pointer.
        (bits & 0x0000_FFFF_FFFF_FFFF) as *const u8
    } else if is_raw_pointer(bits) {
        // Raw heap pointer (no NaN-box tag). `top16 == 0` alone is NOT enough:
        // it is a superset of the positive-subnormal doubles, so it classified
        // every denormal `Number` as a pointer and dereferenced it —
        // `JSON.stringify(1e-317)` (bits `0x1ee257`) SIGSEGV'd at `0x1ee24f`,
        // reachable from untrusted input via
        // `JSON.stringify(JSON.parse(text))`. It was already the second
        // narrowing of this test (`top16 < 0x7FF8` before it, which crashed
        // `JSON.stringify(42)`); a third narrowing would not have helped
        // either, because a raw pointer and a denormal share bit patterns by
        // construction. `is_raw_pointer` decides by GC allocation membership
        // instead — see its doc in `json/mod.rs`.
        bits as *const u8
    } else {
        return None;
    };
    // Handle-band synthetic ids are never lazy arrays and must not be deref'd
    // one word below the id (#6240/#6241). Above the band is not proof either:
    // require a GC-tracked allocation before the header read. (Redundant for
    // the `is_raw_pointer` arm, load-bearing for the `POINTER_TAG` one.)
    if maybe_ptr.is_null() || !stringify::ptr_is_tracked_heap_object(maybe_ptr) {
        return None;
    }
    let gc_header = maybe_ptr.sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
    if (*gc_header).obj_type != crate::gc::GC_TYPE_LAZY_ARRAY {
        return None;
    }
    let lazy = maybe_ptr as *const crate::json_tape::LazyArrayHeader;
    if (*lazy).magic != crate::json_tape::LAZY_ARRAY_MAGIC || !(*lazy).materialized.is_null() {
        return None;
    }
    // Phase 5: if the sparse per-element cache has ANY bit set,
    // stringify might miss mutations made through a cached element
    // (e.g. `parsed[0].name = "x"` modifies the materialized object
    // but leaves the blob bytes untouched). Force-materialize the
    // full tree (which consults the sparse cache and preserves
    // cached mutations), then bail out so `redirect_lazy_to_materialized`
    // forwards to the materialized ArrayHeader on the next stringify
    // dispatch. No bits set means we haven't handed any pointers to
    // user code yet, so the blob bytes are authoritative.
    if !(*lazy).materialized_bitmap.is_null() && (*lazy).cached_length > 0 {
        let bitmap = (*lazy).materialized_bitmap;
        let bitmap_words = ((*lazy).cached_length as usize).div_ceil(64);
        let mut has_bits = false;
        for w in 0..bitmap_words {
            if *bitmap.add(w) != 0 {
                has_bits = true;
                break;
            }
        }
        if has_bits {
            crate::json_tape::force_materialize_lazy(
                lazy as *mut crate::json_tape::LazyArrayHeader,
            );
            return None;
        }
    }
    let tape = crate::json_tape::LazyArrayHeader::tape_slice(lazy);
    let blob_bytes = crate::json_tape::LazyArrayHeader::blob_bytes(lazy);
    if tape.is_empty() {
        return None;
    }
    let root = (*lazy).root_idx as usize;
    let start = tape[root].offset as usize;
    let end_idx = tape[root].link as usize;
    let end = tape[end_idx].offset as usize + 1; // +1 includes `]`
    if end > blob_bytes.len() || start > end {
        return None;
    }
    let normalized = normalize_lazy_json_numbers(tape, blob_bytes, start, end)?;
    Some(json_string_from_output_bytes(normalized.as_ref()))
}

#[no_mangle]
pub unsafe extern "C" fn js_json_stringify(value: f64, type_hint: u32) -> *mut StringHeader {
    if let Some(ptr) = try_stringify_lazy_array(value) {
        return ptr;
    }
    // If the value is a lazy array that's already been materialized
    // (indexed access forced it into a real tree), stringify the
    // tree directly — the generic walker would otherwise read the
    // LazyArrayHeader's fields as if they were array elements and
    // crash on the first deref of a bogus pointer.
    let value = redirect_lazy_to_materialized(value);

    // Non-reentrant fast path (issue #67): skip the shape_cache save/restore
    // round-trip (two RefCell.borrow_mut's + a Vec mem::take/assign) for the
    // common outermost call. A simple Cell-based depth counter identifies
    // reentrant calls (toJSON callbacks); only those pay for the save.
    let prior_depth = STRINGIFY_DEPTH.with(|d| {
        let c = d.get();
        d.set(c + 1);
        c
    });
    // Defensive: a throw (circular-ref TypeError) during a prior stringify
    // could longjmp past the arm/disarm pair around a `toJSON`-result recursion
    // and leave `SUPPRESS_NEXT_TO_JSON` set. Clear it at the outermost entry so
    // it can't leak across top-level calls.
    if prior_depth == 0 {
        super::SUPPRESS_NEXT_TO_JSON.with(|c| c.set(false));
        // Arbitrary user code ran since the last stringify, so the cached
        // `Object.prototype`-has-`toJSON` verdict must be recomputed (#6009).
        super::invalidate_object_proto_tojson_state();
        // A circular-ref `TypeError` longjmps past the `STRINGIFY_STACK`
        // pops (js_throw doesn't unwind Rust), so a caught throw can leave
        // stale ancestor pointers behind. Clear at the outermost entry so they
        // can't trigger a spurious "circular structure" on the next top-level
        // call (or, worse, mask a real cycle by colliding with a reused addr).
        super::STRINGIFY_STACK.with(|s| s.borrow_mut().clear());
    }
    let saved_cache = if prior_depth > 0 {
        Some(take_shape_cache())
    } else {
        None
    };
    let mut buf = take_stringify_buf();
    // Scratch buffer is pre-sized to 4096 on first thread-local init and
    // retained across calls, so most small stringifies never hit a
    // String::reserve. `push_str` grows on overflow for the rare
    // single-call output that exceeds that, so skip the estimate call
    // (issue #67: it was ~10ns of wasted work per call for small values).
    //
    // The root value's `toJSON` key is the empty String (§25.5.2.2). Reset it
    // unconditionally so a nested `JSON.stringify` inside a `toJSON`/replacer
    // callback also sees `""` at its own root, and so a key left by a prior
    // top-level call can't leak in (#5909).
    super::reset_to_json_key();
    stringify_value(value, type_hint, &mut buf);
    let ptr = json_string_from_output_bytes(buf.as_bytes());
    restore_stringify_buf(buf);
    match saved_cache {
        Some(s) => restore_shape_cache(s),
        None => clear_shape_cache(),
    }
    STRINGIFY_DEPTH.with(|d| d.set(d.get() - 1));
    ptr
}

// ─── Specialized stringify functions ──────────────────────────────────────────

#[no_mangle]
pub unsafe extern "C" fn js_json_stringify_string(
    str_ptr: *const StringHeader,
) -> *mut StringHeader {
    let s = match str_from_header(str_ptr) {
        Some(s) => s,
        None => return std::ptr::null_mut(),
    };
    let mut buf = String::with_capacity(s.len() + 16);
    write_escaped_string(&mut buf, s);
    js_string_from_bytes(buf.as_ptr(), buf.len() as u32)
}

/// Stringify a number
#[no_mangle]
pub unsafe extern "C" fn js_json_stringify_number(value: f64) -> *mut StringHeader {
    if value.is_nan() || value.is_infinite() {
        return js_string_from_bytes(b"null".as_ptr(), 4);
    }
    if value.fract() == 0.0 && value.abs() < crate::builtins::INT_EXACT_FASTPATH_LIMIT {
        let mut itoa_buf = itoa::Buffer::new();
        let s = itoa_buf.format(value as i64);
        return js_string_from_bytes(s.as_ptr(), s.len() as u32);
    }
    // #6127: at/above 2^53 the exact integer can carry more digits than the
    // shortest round-trip (`2**58`), so defer to the shortest-round-trip formatter.
    let s = crate::string::js_format_f64(value);
    js_string_from_bytes(s.as_ptr(), s.len() as u32)
}

/// Stringify a boolean
#[no_mangle]
pub unsafe extern "C" fn js_json_stringify_bool(value: bool) -> *mut StringHeader {
    let s = if value { "true" } else { "false" };
    js_string_from_bytes(s.as_ptr(), s.len() as u32)
}

/// Stringify null
#[no_mangle]
pub unsafe extern "C" fn js_json_stringify_null() -> *mut StringHeader {
    js_string_from_bytes(b"null".as_ptr(), 4)
}

/// Check if a string is valid JSON
#[no_mangle]
pub unsafe extern "C" fn js_json_is_valid(text_ptr: *const StringHeader) -> f64 {
    const TAG_TRUE: u64 = 0x7FFC_0000_0000_0004;
    const TAG_FALSE: u64 = 0x7FFC_0000_0000_0003;
    if text_ptr.is_null() {
        return f64::from_bits(TAG_FALSE);
    }
    let len = (*text_ptr).byte_len as usize;
    let data_ptr = (text_ptr as *const u8).add(std::mem::size_of::<StringHeader>());
    let bytes = std::slice::from_raw_parts(data_ptr, len);
    if serde_json::from_slice::<serde_json::Value>(bytes).is_ok() {
        f64::from_bits(TAG_TRUE)
    } else {
        f64::from_bits(TAG_FALSE)
    }
}

// ─── Utility functions ────────────────────────────────────────────────────────

/// Legacy wrapper that allocates a String from a StringHeader
pub(crate) unsafe fn string_from_header(ptr: *const StringHeader) -> Option<String> {
    str_from_header(ptr).map(|s| s.to_string())
}

/// Get a value from parsed JSON by key (for object access)
#[no_mangle]
pub unsafe extern "C" fn js_json_get_string(
    json_ptr: *const StringHeader,
    key_ptr: *const StringHeader,
) -> *mut StringHeader {
    let json_str = match string_from_header(json_ptr) {
        Some(j) => j,
        None => return std::ptr::null_mut(),
    };
    let key = match string_from_header(key_ptr) {
        Some(k) => k,
        None => return std::ptr::null_mut(),
    };
    if let Ok(serde_json::Value::Object(obj)) = serde_json::from_str::<serde_json::Value>(&json_str)
    {
        if let Some(serde_json::Value::String(s)) = obj.get(&key) {
            return js_string_from_bytes(s.as_ptr(), s.len() as u32);
        }
    }
    std::ptr::null_mut()
}

/// Get a number from parsed JSON by key
#[no_mangle]
pub unsafe extern "C" fn js_json_get_number(
    json_ptr: *const StringHeader,
    key_ptr: *const StringHeader,
) -> f64 {
    let json_str = match string_from_header(json_ptr) {
        Some(j) => j,
        None => return f64::NAN,
    };
    let key = match string_from_header(key_ptr) {
        Some(k) => k,
        None => return f64::NAN,
    };
    if let Ok(serde_json::Value::Object(obj)) = serde_json::from_str::<serde_json::Value>(&json_str)
    {
        if let Some(serde_json::Value::Number(n)) = obj.get(&key) {
            return n.as_f64().unwrap_or(f64::NAN);
        }
    }
    f64::NAN
}

/// Get a boolean from parsed JSON by key
#[no_mangle]
pub unsafe extern "C" fn js_json_get_bool(
    json_ptr: *const StringHeader,
    key_ptr: *const StringHeader,
) -> bool {
    let json_str = match string_from_header(json_ptr) {
        Some(j) => j,
        None => return false,
    };
    let key = match string_from_header(key_ptr) {
        Some(k) => k,
        None => return false,
    };
    if let Ok(serde_json::Value::Object(obj)) = serde_json::from_str::<serde_json::Value>(&json_str)
    {
        if let Some(serde_json::Value::Bool(b)) = obj.get(&key) {
            return *b;
        }
    }
    false
}
