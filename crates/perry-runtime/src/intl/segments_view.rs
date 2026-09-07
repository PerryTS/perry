//! `Intl.Segmenter` **view mode** — the runtime half of
//! `INTERFACE_segments_view.md` §9, agreed with the keystroke lane.
//!
//! The compiler proves that a `for (let {segment: O} of X.segment(q))` loop
//! never lets the record or `O` escape, and then drives this cursor instead of
//! building either. Nothing here constructs a `Segments`: `open` takes the
//! segmenter and the input, which is why the view mode never depended on the
//! lazy-`Segments` work (measured and refuted separately).
//!
//! ## The rooting contract, which is the reason this file is small
//!
//! The cursor is an **ordinary GC object** whose slot 0 holds the input string
//! as a **traced value**, so the collector marks and rewrites it like any other
//! field — no registered root, no side table, no new scanner. Every entry point
//! re-derives its `&str` from that slot on entry and drops it before returning.
//! **No address derived from the input outlives a single entry point, and the
//! only thing that crosses the loop body is a GC pointer the collector
//! maintains.** `_next` and `_code_point_at` allocate nothing at all, so inside
//! them the question cannot even arise; `open` and `_segment` allocate and
//! carry the obligation explicitly.

use crate::object::ObjectHeader;
use crate::string::StringHeader;
use crate::value::JSValue;
use std::sync::atomic::{AtomicU64, Ordering};

/// Class id for the view cursor. It is the brand: a load, where a
/// `get_string_field(obj, "__brand")` check would allocate a key string on a
/// path that runs per loop entry.
pub const SEGMENTS_CURSOR_CLASS_ID: u32 = 0xFFFF_000E;

const F_INPUT: u32 = 0;
const F_BYTE_START: u32 = 1;
const F_UTF16_START: u32 = 2;
const F_BYTE_END: u32 = 3;
const F_UTF16_LEN: u32 = 4;
const CURSOR_FIELDS: u32 = 5;

// --- counters (PERRY_SEGVIEW_DIAG=1) ---------------------------------------

static OPENS: AtomicU64 = AtomicU64::new(0);
static DECLINE_NOT_SEGMENTER: AtomicU64 = AtomicU64::new(0);
static DECLINE_NOT_GRAPHEME: AtomicU64 = AtomicU64::new(0);
static DECLINE_SEGMENT_PATCHED: AtomicU64 = AtomicU64::new(0);
static DECLINE_NOT_STRING: AtomicU64 = AtomicU64::new(0);
static DECLINE_NOT_UTF8: AtomicU64 = AtomicU64::new(0);
static DECLINE_EMPTY: AtomicU64 = AtomicU64::new(0);
static NEXTS: AtomicU64 = AtomicU64::new(0);
static CODE_POINT_ATS: AtomicU64 = AtomicU64::new(0);
static MATERIALISE_SEGMENT: AtomicU64 = AtomicU64::new(0);
static REGEXP_TEST_ACCEPTED: AtomicU64 = AtomicU64::new(0);
static REGEXP_TEST_DECLINED: AtomicU64 = AtomicU64::new(0);

fn diag_on() -> bool {
    // Always on under test: a counter nothing increments cannot be asserted on,
    // and the decline counters exist so a decline names its cause.
    if cfg!(test) {
        return true;
    }
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var("PERRY_SEGVIEW_DIAG").is_ok())
}

#[inline(always)]
fn bump(c: &AtomicU64) {
    if diag_on() {
        c.fetch_add(1, Ordering::Relaxed);
    }
}

/// The counters' consumer today is the test suite, which is why there is no
/// `report_*` function: a printer with no caller is dead code, and the tally
/// the campaign reads is the COMPILER-side `PERRY_SEGVIEW_DIAG` one. Wire a
/// runtime-side printer when a rig run needs these numbers, not before.
#[cfg(test)]
fn counters() -> [u64; 4] {
    [
        OPENS.load(Ordering::Relaxed),
        DECLINE_NOT_STRING.load(Ordering::Relaxed),
        NEXTS.load(Ordering::Relaxed),
        crate::object::regex_proto_thunks::REGEXP_PROTOTYPE_TEST_WALKS.load(Ordering::Relaxed),
    ]
}

// --- cursor plumbing --------------------------------------------------------

#[inline(always)]
fn cursor_ptr(value: f64) -> Option<*mut ObjectHeader> {
    let jv = JSValue::from_bits(value.to_bits());
    if !jv.is_pointer() {
        return None;
    }
    let obj = jv.as_pointer::<ObjectHeader>();
    let addr = obj as usize;
    if !crate::value::addr_class::is_above_handle_band(addr)
        || crate::arena::classify_heap_generation(addr) == crate::arena::HeapGeneration::Unknown
    {
        return None;
    }
    let header =
        unsafe { (obj as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader };
    if unsafe {
        (*header).obj_type != crate::gc::GC_TYPE_OBJECT
            || (*obj).class_id != SEGMENTS_CURSOR_CLASS_ID
    } {
        return None;
    }
    Some(obj as *mut ObjectHeader)
}

#[inline(always)]
fn num_field(obj: *mut ObjectHeader, index: u32) -> usize {
    let bits = crate::object::js_object_get_field(obj, index);
    let n = JSValue::from_bits(bits.bits()).to_number();
    if n.is_finite() && n >= 0.0 {
        n as usize
    } else {
        0
    }
}

#[inline(always)]
fn set_num_field(obj: *mut ObjectHeader, index: u32, value: usize) {
    crate::object::js_object_set_field(obj, index, JSValue::number(value as f64));
}

/// Run `f` with the cursor's input as a `&str`. The borrow is derived here and
/// dropped at the end of the call; a short (SSO) string is decoded into the
/// caller's stack buffer, so neither case allocates and neither case leaks an
/// address.
///
/// **The UTF-8 validation is done ONCE, in `open`, and is not repeated here.**
/// Re-validating cost 6.2 % of the loop's thread as `core::str::from_utf8`
/// self, because every entry point re-derives the borrow per call (the §9a
/// rooting contract) and each derivation walked the whole input again.
///
/// The invariant that makes `from_utf8_unchecked` sound here, stated so it can
/// be checked rather than trusted:
///
/// 1. `F_INPUT` is written exactly once, by `js_segments_view_open`, and never
///    reassigned — no entry point below stores into it.
/// 2. `open` refuses any input that is not already a string primitive and runs
///    `std::str::from_utf8` on its bytes before allocating the cursor, so the
///    value in that slot has been validated.
/// 3. A collection may MOVE that string but never rewrites its bytes, and the
///    traced slot is updated to the new address, so the bytes reachable here
///    are the same bytes `open` validated.
/// 4. The SSO path decodes the same value into the stack buffer, so it carries
///    the same guarantee.
///
/// A `debug_assert` re-checks it in debug builds, which is where a future
/// fourth writer to the slot would be caught.
#[inline]
fn with_input<R>(cursor: *mut ObjectHeader, f: impl FnOnce(&str) -> R) -> Option<R> {
    let value = crate::object::js_object_get_field(cursor, F_INPUT);
    let mut sso = [0u8; crate::value::SHORT_STRING_MAX_LEN];
    let bytes =
        unsafe { crate::string::js_string_key_bytes(JSValue::from_bits(value.bits()), &mut sso) }?;
    debug_assert!(
        std::str::from_utf8(bytes).is_ok(),
        "the cursor's input slot is written once, by `open`, from a value it \
         validated — a failure here means a second writer appeared"
    );
    // SAFETY: invariants 1-4 above.
    let text = unsafe { std::str::from_utf8_unchecked(bytes) };
    Some(f(text))
}

#[cfg(feature = "intl-segmenter")]
fn next_boundary(text: &str, from: usize) -> Option<usize> {
    if from >= text.len() {
        return None;
    }
    let mut c = unicode_segmentation::GraphemeCursor::new(from, text.len(), true);
    match c.next_boundary(text, 0) {
        Ok(Some(next)) if next > from => Some(next),
        _ => None,
    }
}

#[cfg(not(feature = "intl-segmenter"))]
fn next_boundary(text: &str, from: usize) -> Option<usize> {
    text[from..].chars().next().map(|c| from + c.len_utf8())
}

// --- entry points -----------------------------------------------------------

/// `open(segmenter, input)` — a cursor positioned BEFORE the first segment, or
/// `0.0` to mean "take the spec path you already emit".
///
/// **The decline path has no observable effect of any kind.** The compiler
/// emits `open(X, q)` first and, on a decline, evaluates `X.segment(q)` exactly
/// once in its original position — so a decline must not coerce, allocate,
/// advance or throw. In particular `input` must ALREADY be a string primitive:
/// `build_segments` coerces with `js_jsvalue_to_string`, which runs user
/// `toString`/`valueOf` and **throws on a Symbol**, and doing that here would
/// either run user code twice or move the TypeError out of the spec path.
/// Nothing before the final step allocates.
#[no_mangle]
pub extern "C" fn js_segments_view_open(segmenter: f64, input: f64) -> f64 {
    // 1. a pristine Intl.Segmenter whose `segment` is still the builtin.
    let Some(obj) = (unsafe { crate::object::object_ptr_from_value(segmenter) }) else {
        bump(&DECLINE_NOT_SEGMENTER);
        return 0.0;
    };
    let obj = obj as *mut ObjectHeader;
    if !intl_kind_is_segmenter(obj) {
        bump(&DECLINE_NOT_SEGMENTER);
        return 0.0;
    }
    if !segment_method_is_canonical(obj) {
        bump(&DECLINE_SEGMENT_PATCHED);
        return 0.0;
    }
    // 2. grapheme only (§4): a resumable word cursor is not equivalent to
    //    segmenting the whole string, and nothing measured needs one.
    if !granularity_is_grapheme(obj) {
        bump(&DECLINE_NOT_GRAPHEME);
        return 0.0;
    }
    // 3. an ALREADY-string input, checked before any coercion could happen.
    let jv = JSValue::from_bits(input.to_bits());
    if !jv.is_string() {
        bump(&DECLINE_NOT_STRING);
        return 0.0;
    }
    // 4. valid UTF-8 and non-empty. A WTF-8 lone surrogate is repaired by
    //    `segmenter_input_text` on the spec path by COPYING, which a borrowing
    //    cursor cannot do.
    let mut sso = [0u8; crate::value::SHORT_STRING_MAX_LEN];
    let Some(bytes) = (unsafe { crate::string::js_string_key_bytes(jv, &mut sso) }) else {
        bump(&DECLINE_NOT_STRING);
        return 0.0;
    };
    if bytes.is_empty() {
        bump(&DECLINE_EMPTY);
        return 0.0;
    }
    if std::str::from_utf8(bytes).is_err() {
        bump(&DECLINE_NOT_UTF8);
        return 0.0;
    }
    // 5. only now allocate. The input is held in a rooted handle ACROSS the
    //    cursor allocation and re-read from it afterwards: this is the
    //    #9539/#9445 shape in its simplest form — allocate, then store a value
    //    that predates the allocation.
    let scope = crate::gc::RuntimeHandleScope::new();
    let input_h = scope.root_nanbox_f64(input);
    let cursor = crate::object::js_object_alloc(SEGMENTS_CURSOR_CLASS_ID, CURSOR_FIELDS);
    if cursor.is_null() {
        return 0.0;
    }
    crate::object::js_object_set_field(
        cursor,
        F_INPUT,
        JSValue::from_bits(input_h.get_nanbox_f64().to_bits()),
    );
    set_num_field(cursor, F_BYTE_START, 0);
    set_num_field(cursor, F_UTF16_START, 0);
    set_num_field(cursor, F_BYTE_END, 0);
    set_num_field(cursor, F_UTF16_LEN, 0);
    bump(&OPENS);
    crate::value::js_nanbox_pointer(cursor as i64)
}

/// Advance to the next grapheme boundary. `1.0` if a segment is now current,
/// `0.0` at the end. **Allocation-free by contract**: three integer field
/// writes and a UAX #29 boundary scan, no arena allocation, no owned `String`,
/// no descriptor insert — which is also why it cannot collect.
#[no_mangle]
pub extern "C" fn js_segments_view_next(cursor: f64) -> f64 {
    let Some(c) = cursor_ptr(cursor) else {
        return 0.0;
    };
    let from = num_field(c, F_BYTE_END);
    let utf16_start = num_field(c, F_UTF16_START) + num_field(c, F_UTF16_LEN);
    let step = with_input(c, |text| {
        next_boundary(text, from).map(|next| (next, super::segmenter::utf16_len(&text[from..next])))
    })
    .flatten();
    let Some((next, seg_u16)) = step else {
        return 0.0;
    };
    set_num_field(c, F_BYTE_START, from);
    set_num_field(c, F_UTF16_START, utf16_start);
    set_num_field(c, F_BYTE_END, next);
    set_num_field(c, F_UTF16_LEN, seg_u16 as usize);
    bump(&NEXTS);
    1.0
}

/// `segment.codePointAt(k)` for the CURRENT segment, without materialising it.
///
/// `k` is a UTF-16 offset **relative to the segment start** and is bounded by
/// the **segment**, not the input: `k` at or past the segment's UTF-16 length
/// is `undefined` even though the input has more code units there. Decoding
/// starts from the cursor's BYTE offset, so `k = 0` is O(1) — calling
/// `js_string_code_point_at` on the input instead would walk from index 0 on
/// any non-ASCII string and make the loop quadratic.
#[no_mangle]
pub extern "C" fn js_segments_view_code_point_at(cursor: f64, k: f64) -> f64 {
    let undef = f64::from_bits(crate::value::TAG_UNDEFINED);
    let Some(c) = cursor_ptr(cursor) else {
        return undef;
    };
    if !k.is_finite() || k < 0.0 || k.fract() != 0.0 {
        return undef;
    }
    let k = k as usize;
    if k >= num_field(c, F_UTF16_LEN) {
        return undef;
    }
    let start = num_field(c, F_BYTE_START);
    let end = num_field(c, F_BYTE_END);
    bump(&CODE_POINT_ATS);
    with_input(c, |text| {
        let seg = &text[start..end];
        let mut utf16_pos = 0usize;
        for ch in seg.chars() {
            let units = ch.len_utf16();
            if utf16_pos + units > k {
                if units == 1 || utf16_pos == k {
                    // A BMP code point, or the START of a surrogate pair,
                    // which per spec is the whole code point.
                    return u32::from(ch) as f64;
                }
                // `k` lands on the low surrogate half: return the bare unit,
                // exactly as `js_string_code_point_at` does.
                let v = u32::from(ch) - 0x10000;
                return (0xDC00 + (v & 0x3FF)) as f64;
            }
            utf16_pos += units;
        }
        undef
    })
    .unwrap_or(undef)
}

/// Materialise the current segment. The compiler emits this for a use it
/// cannot answer from the view — the per-use materialise-on-miss.
#[no_mangle]
pub extern "C" fn js_segments_view_segment(cursor: f64) -> f64 {
    let undef = f64::from_bits(crate::value::TAG_UNDEFINED);
    let Some(c) = cursor_ptr(cursor) else {
        return undef;
    };
    let start = num_field(c, F_BYTE_START);
    let end = num_field(c, F_BYTE_END);
    bump(&MATERIALISE_SEGMENT);
    // The allocation happens INSIDE the borrow, so the borrow must not outlive
    // it: take the bytes out first, then allocate from a copy on the stack path
    // `js_string_from_bytes` performs. Nothing derived from the input survives
    // this call.
    let made = with_input(c, |text| {
        let seg = &text[start..end];
        crate::string::js_string_from_bytes(seg.as_ptr(), seg.len() as u32)
    });
    match made {
        Some(ptr) if !ptr.is_null() => {
            f64::from_bits(JSValue::string_ptr(ptr as *mut StringHeader).bits())
        }
        _ => undef,
    }
}

/// `regex.test(segment)` without materialising the segment. **Three-valued**:
/// `true` / `false` / **`undefined` = "I decline"**, on which the compiler
/// materialises and calls the ordinary path.
///
/// It declines for a global or sticky regex, because `test` is then stateful
/// (`lastIndex` must be consulted and advanced) and that bookkeeping is written
/// against a `StringHeader`. It declines for a patched `RegExp.prototype.test`
/// or an own `test`, because `is RegExp` at the call site does not rule those
/// out and a view-mode test would silently bypass user code.
///
/// When it accepts, the haystack is a **slice whose bounds are the string's
/// ends**, so `^`, `$` and lookbehind are segment-local — the same answer the
/// materialised call would give, not "a match starting at an offset".
#[no_mangle]
pub extern "C" fn js_segments_view_regexp_test(cursor: f64, regex: f64) -> f64 {
    let undef = f64::from_bits(crate::value::TAG_UNDEFINED);
    let Some(c) = cursor_ptr(cursor) else {
        return undef;
    };
    let jv = JSValue::from_bits(regex.to_bits());
    if !jv.is_pointer() {
        bump(&REGEXP_TEST_DECLINED);
        return undef;
    }
    let re = jv.as_pointer::<crate::regex::RegExpHeader>();
    if !crate::regex::is_valid_regex_ptr(re) {
        bump(&REGEXP_TEST_DECLINED);
        return undef;
    }
    // `is RegExp` at the call site does not rule out a patched
    // `RegExp.prototype.test`, so the runtime re-checks and declines.
    //
    // Both helpers below are `#[cfg(feature = "regex-engine")]`. This entry
    // point is NOT gated with them: it is `#[no_mangle]`, so the symbol has to
    // exist in every configuration or a binary that emits a call to it fails to
    // link. Without the engine the fast path simply declines, which is the same
    // contract every other decline here has — the caller materialises and calls
    // `RegExp.prototype.test` itself.
    #[cfg(not(feature = "regex-engine"))]
    {
        let _ = (c, re);
        bump(&REGEXP_TEST_DECLINED);
        return undef;
    }
    #[cfg(feature = "regex-engine")]
    {
        if !crate::object::regex_proto_thunks::regexp_prototype_test_is_canonical(regex) {
            bump(&REGEXP_TEST_DECLINED);
            return undef;
        }
        let start = num_field(c, F_BYTE_START);
        let end = num_field(c, F_BYTE_END);
        let verdict = with_input(c, |text| {
            crate::regex::regexp_test_str_bounded(re, &text[start..end])
        })
        .flatten();
        match verdict {
            Some(v) => {
                bump(&REGEXP_TEST_ACCEPTED);
                f64::from_bits(JSValue::bool(v).bits())
            }
            None => {
                bump(&REGEXP_TEST_DECLINED);
                undef
            }
        }
    }
}

// Keepalive anchors. The compiler emits calls to these only when the view tier
// fires, so without a reference the bundle link's stub localization can drop
// them before the lowering that needs them is ever compiled — the same reason
// `js_for_of_next` carries `KEEP_JS_FOR_OF_NEXT`.
#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_JS_SEGMENTS_VIEW_OPEN: extern "C" fn(f64, f64) -> f64 = js_segments_view_open;
#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_JS_SEGMENTS_VIEW_NEXT: extern "C" fn(f64) -> f64 = js_segments_view_next;
#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_JS_SEGMENTS_VIEW_CODE_POINT_AT: extern "C" fn(f64, f64) -> f64 =
    js_segments_view_code_point_at;
#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_JS_SEGMENTS_VIEW_SEGMENT: extern "C" fn(f64) -> f64 = js_segments_view_segment;
#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_JS_SEGMENTS_VIEW_REGEXP_TEST: extern "C" fn(f64, f64) -> f64 =
    js_segments_view_regexp_test;

// --- the `open` predicates, all non-allocating after the first intern -------

fn interned(name: &[u8]) -> *const StringHeader {
    crate::string::intern_ascii_literal(name)
}

fn string_field_is(obj: *mut ObjectHeader, key: &[u8], expected: &[u8]) -> bool {
    let k = interned(key);
    if k.is_null() {
        return false;
    }
    let value = crate::object::js_object_get_field_by_name(obj, k);
    let mut sso = [0u8; crate::value::SHORT_STRING_MAX_LEN];
    match unsafe { crate::string::js_string_key_bytes(value, &mut sso) } {
        Some(bytes) => bytes == expected,
        None => false,
    }
}

fn intl_kind_is_segmenter(obj: *mut ObjectHeader) -> bool {
    string_field_is(obj, b"__intlKind", b"Segmenter")
}

fn granularity_is_grapheme(obj: *mut ObjectHeader) -> bool {
    string_field_is(obj, b"__intlGranularity", b"grapheme")
}

/// Is `obj.segment` still the builtin? One inherited lookup catches BOTH an own
/// shadow on the instance and a replaced `Intl.Segmenter.prototype.segment`,
/// because the lookup resolves whatever the call would have resolved.
fn segment_method_is_canonical(obj: *mut ObjectHeader) -> bool {
    let k = interned(b"segment");
    if k.is_null() {
        return false;
    }
    let value = crate::object::js_object_get_field_by_name(obj, k);
    let jv = JSValue::from_bits(value.bits());
    if !jv.is_pointer() {
        return false;
    }
    let closure = jv.as_pointer::<crate::closure::ClosureHeader>();
    if closure.is_null() {
        return false;
    }
    let entry = crate::closure::get_valid_func_ptr(closure);
    entry == super::segmenter::segmenter_segment_thunk as *const u8
        || entry == super::segmenter::segmenter_bound_segment_thunk as *const u8
}

#[cfg(test)]
mod view_mode_tests {
    use super::*;

    fn js_string(s: &str) -> f64 {
        let ptr = crate::string::js_string_from_bytes(s.as_ptr(), s.len() as u32);
        f64::from_bits(JSValue::string_ptr(ptr as *mut StringHeader).bits())
    }

    /// A real `Intl.Segmenter` instance, built by the runtime's own
    /// constructor path so the test cannot pass against a hand-made object the
    /// production code would reject.
    fn grapheme_segmenter() -> f64 {
        let options = crate::object::js_object_alloc(0, 1);
        let key = crate::string::js_string_from_bytes(b"granularity".as_ptr(), 11);
        crate::object::js_object_set_field_by_name(options, key, js_string("grapheme"));
        // The runtime's OWN constructor path, so the instance carries the same
        // internal fields and the same own bound `segment` a real
        // `new Intl.Segmenter(...)` produces. A hand-made object would test the
        // predicates against something production never sees.
        super::super::make_instance(
            std::ptr::null(),
            super::super::KIND_SEGMENTER,
            js_string("en"),
            crate::value::js_nanbox_pointer(options as i64),
        )
    }

    fn is_undefined(v: f64) -> bool {
        JSValue::from_bits(v.to_bits()).is_undefined()
    }

    /// Walk the cursor and collect (index, code point at 0, segment string).
    fn walk(cursor: f64) -> Vec<(usize, u32, String)> {
        let mut out = Vec::new();
        while js_segments_view_next(cursor) == 1.0 {
            let c = cursor_ptr(cursor).expect("cursor");
            let cp = js_segments_view_code_point_at(cursor, 0.0);
            let seg = js_segments_view_segment(cursor);
            let mut sso = [0u8; crate::value::SHORT_STRING_MAX_LEN];
            let bytes = unsafe {
                crate::string::js_string_key_bytes(JSValue::from_bits(seg.to_bits()), &mut sso)
            }
            .expect("segment string");
            out.push((
                num_field(c, F_UTF16_START),
                cp as u32,
                String::from_utf8_lossy(bytes).into_owned(),
            ));
        }
        out
    }

    /// The view must agree with `graphemes(true)` — the same segmentation the
    /// spec path uses — on the shapes cc actually renders.
    #[test]
    fn view_walk_matches_the_spec_segmentation() {
        let input = "a\u{301}b\u{1f469}\u{200d}\u{1f4bb}\u{1f1fa}\u{1f1f8}c";
        let cursor = js_segments_view_open(grapheme_segmenter(), js_string(input));
        assert!(
            cursor != 0.0,
            "open must accept a pristine grapheme segmenter"
        );
        let got = walk(cursor);

        #[cfg(feature = "intl-segmenter")]
        {
            use unicode_segmentation::UnicodeSegmentation;
            let mut want = Vec::new();
            let mut idx = 0usize;
            for g in input.graphemes(true) {
                want.push((
                    idx,
                    g.chars().next().map(u32::from).unwrap_or(0),
                    g.to_string(),
                ));
                idx += super::super::segmenter::utf16_len(g) as usize;
            }
            assert_eq!(got, want, "view segmentation must equal graphemes(true)");
        }
        assert!(!got.is_empty());
    }

    /// THE FALSIFIER. The two in-loop entry points must move the arena by
    /// ZERO, with the minor count pinned so a collection cannot manufacture the
    /// zero. This is the whole point of the view mode.
    #[test]
    fn next_and_code_point_at_allocate_nothing() {
        let input = "a\u{301}b\u{1f469}\u{200d}\u{1f4bb}c d e f g h i j k l m n o p";
        let scope = crate::gc::RuntimeHandleScope::new();
        let cursor_h = scope.root_nanbox_f64(js_segments_view_open(
            grapheme_segmenter(),
            js_string(input),
        ));
        assert!(cursor_h.get_nanbox_f64() != 0.0);
        // Warm: the first call may lazily build anything it builds.
        js_segments_view_next(cursor_h.get_nanbox_f64());
        js_segments_view_code_point_at(cursor_h.get_nanbox_f64(), 0.0);

        let minors_before = crate::gc::instruments::copying_minor_cycles();
        let bytes_before = crate::arena::arena_in_use_bytes();
        let mut steps = 0usize;
        for _ in 0..200 {
            if js_segments_view_next(cursor_h.get_nanbox_f64()) != 1.0 {
                // Re-open rather than stop: a short input would otherwise make
                // this test pass by doing nothing.
                break;
            }
            let cp = js_segments_view_code_point_at(cursor_h.get_nanbox_f64(), 0.0);
            assert!(!is_undefined(cp), "every segment has a code point at 0");
            steps += 1;
        }
        let bytes_after = crate::arena::arena_in_use_bytes();
        assert!(
            steps > 5,
            "the walk must actually have stepped (got {steps})"
        );
        assert_eq!(
            crate::gc::instruments::copying_minor_cycles(),
            minors_before,
            "a collection inside the window would make a zero delta prove nothing"
        );
        assert_eq!(
            bytes_after.saturating_sub(bytes_before),
            0,
            "next + code_point_at allocated {} bytes over {steps} steps",
            bytes_after.saturating_sub(bytes_before)
        );
    }

    /// The rooting obligation of §9e, exercised rather than asserted: `open`
    /// allocates the cursor while holding the input, so a collection landing in
    /// that window must not leave a dead value in the traced slot. Force a
    /// collection immediately before each `open` and then READ the input back
    /// through the cursor: with the handle removed (`PERRY_SABOTAGE_SEGVIEW=
    /// norooting`) this is the test that fails.
    #[test]
    fn open_survives_a_collection_between_its_two_allocations() {
        for round in 0..40 {
            let input = format!("a\u{301}b{round}\u{1f600}c");
            let s = js_string(&input);
            // Churn, so the cursor allocation below is likely to be the one
            // that trips the collector, and collect explicitly as well.
            let scope = crate::gc::RuntimeHandleScope::new();
            let s_h = scope.root_nanbox_f64(s);
            for _ in 0..64 {
                let _ = crate::object::js_object_alloc(0, 4);
            }
            crate::gc::js_gc_collect();
            let cursor = js_segments_view_open(grapheme_segmenter(), s_h.get_nanbox_f64());
            assert!(cursor != 0.0, "open must accept round {round}");
            let mut seen = String::new();
            while js_segments_view_next(cursor) == 1.0 {
                let seg = js_segments_view_segment(cursor);
                let mut sso = [0u8; crate::value::SHORT_STRING_MAX_LEN];
                let bytes = unsafe {
                    crate::string::js_string_key_bytes(JSValue::from_bits(seg.to_bits()), &mut sso)
                }
                .expect("segment string");
                seen.push_str(&String::from_utf8_lossy(bytes));
            }
            assert_eq!(
                seen, input,
                "the cursor's input slot must survive a collection inside open (round {round})"
            );
        }
    }

    /// `k` is bounded by the SEGMENT, not the input: reading past the end of a
    /// one-unit segment must be `undefined` even though the input continues.
    /// A view that clamped to the input would answer the NEXT grapheme.
    #[test]
    fn code_point_at_is_bounded_by_the_segment() {
        let cursor = js_segments_view_open(grapheme_segmenter(), js_string("ab"));
        assert!(cursor != 0.0);
        assert_eq!(js_segments_view_next(cursor), 1.0);
        assert_eq!(
            js_segments_view_code_point_at(cursor, 0.0),
            'a' as u32 as f64
        );
        assert!(
            is_undefined(js_segments_view_code_point_at(cursor, 1.0)),
            "k past the segment end must be undefined, not the next grapheme"
        );
        assert!(is_undefined(js_segments_view_code_point_at(cursor, -1.0)));
        assert!(is_undefined(js_segments_view_code_point_at(cursor, 0.5)));
    }

    /// A surrogate pair is ONE grapheme and `codePointAt(0)` is the whole code
    /// point; `k = 1` is the bare low surrogate, exactly as
    /// `js_string_code_point_at` answers on the materialised substring.
    #[test]
    fn code_point_at_matches_the_materialised_answer_on_a_surrogate_pair() {
        let cursor = js_segments_view_open(grapheme_segmenter(), js_string("\u{1f600}x"));
        assert_eq!(js_segments_view_next(cursor), 1.0);
        assert_eq!(js_segments_view_code_point_at(cursor, 0.0), 0x1f600 as f64);
        assert_eq!(js_segments_view_code_point_at(cursor, 1.0), 0xDE00 as f64);
        let seg = js_segments_view_segment(cursor);
        let ptr = JSValue::from_bits(seg.to_bits()).as_string_ptr();
        assert_eq!(
            crate::string::js_string_code_point_at(ptr, 0),
            js_segments_view_code_point_at(cursor, 0.0),
            "the view must answer exactly what the materialised segment does"
        );
        assert_eq!(
            crate::string::js_string_code_point_at(ptr, 1),
            js_segments_view_code_point_at(cursor, 1.0)
        );
    }

    /// Every decline in §9f, and the one that matters most: a non-string input
    /// must be refused BEFORE any coercion, and `open` must never throw — the
    /// compiler evaluates `X.segment(q)` itself on a decline.
    #[test]
    fn open_declines_without_side_effects() {
        let seg = grapheme_segmenter();
        assert_eq!(js_segments_view_open(seg, js_string("")), 0.0, "empty");
        assert_eq!(
            js_segments_view_open(seg, f64::from_bits(crate::value::TAG_UNDEFINED)),
            0.0,
            "undefined input must decline, not coerce to \"undefined\""
        );
        assert_eq!(js_segments_view_open(seg, 42.0), 0.0, "number input");
        let obj = crate::object::js_object_alloc(0, 0);
        assert_eq!(
            js_segments_view_open(seg, crate::value::js_nanbox_pointer(obj as i64)),
            0.0,
            "an object input must decline before running toString"
        );
        assert_eq!(
            js_segments_view_open(crate::value::js_nanbox_pointer(obj as i64), js_string("a")),
            0.0,
            "a non-Segmenter receiver must decline"
        );
        // A lone surrogate is WTF-8: the spec path repairs it by copying, a
        // borrowing cursor cannot, so it declines.
        let wtf8 = crate::string::js_string_from_bytes(b"a\xED\xA0\x80b".as_ptr(), 5);
        assert_eq!(
            js_segments_view_open(
                seg,
                f64::from_bits(JSValue::string_ptr(wtf8 as *mut StringHeader).bits())
            ),
            0.0,
            "invalid UTF-8 must decline"
        );
    }

    /// The canonicality proof must be a LOAD, not a walk: the only by-name
    /// lookup is the one-time recording at install. If this ever climbs with
    /// the number of calls, the fast path is not the path being taken — which
    /// is exactly the failure the counter exists to catch.
    #[cfg(feature = "regex-engine")]
    #[test]
    fn canonicality_proof_walks_once_per_realm_not_once_per_call() {
        let cursor = js_segments_view_open(grapheme_segmenter(), js_string("abcdef"));
        assert!(cursor != 0.0);
        let re = crate::regex::js_regexp_construct(js_string("[a-z]"), js_string(""));
        let re_v = f64::from_bits(JSValue::pointer(re as *const u8).bits());
        assert_eq!(js_segments_view_next(cursor), 1.0);
        let before = counters()[3];
        for _ in 0..50 {
            let v = js_segments_view_regexp_test(cursor, re_v);
            assert!(!is_undefined(v), "a plain regex must keep being accepted");
        }
        assert_eq!(
            counters()[3],
            before,
            "50 accepted calls must add ZERO by-name walks"
        );
        // A second cursor and a second regex must not add one either: the
        // recorded site belongs to the realm, not to the call or the receiver.
        let cursor2 = js_segments_view_open(grapheme_segmenter(), js_string("xy"));
        assert_eq!(js_segments_view_next(cursor2), 1.0);
        let re2 = crate::regex::js_regexp_construct(js_string("[x-z]"), js_string(""));
        let re2_v = f64::from_bits(JSValue::pointer(re2 as *const u8).bits());
        assert!(!is_undefined(js_segments_view_regexp_test(cursor2, re2_v)));
        assert_eq!(
            counters()[3],
            before,
            "a second cursor and regex must add no walks either"
        );
        // NOT asserted: an absolute bound like `walks <= 1`. The unit harness
        // resets arenas between tests and builds the prototype tower more than
        // once in a process, so the per-process total counts REALMS, not calls.
        // The property that matters — and the one that fails if the fast path
        // stops being taken — is the delta above.
    }

    /// Sabotage case three: the fixed cursor class id is the entire per-call
    /// brand check. With both byte-storage registries armed, a valid cursor
    /// advances without entering either. Changing only its class id must make
    /// the next call decline, proving the compare is load-bearing.
    #[test]
    fn view_cursor_brand_is_a_class_load_with_zero_registry_probes() {
        let _buffer = crate::buffer::js_buffer_alloc(1, 0);
        let _typed =
            crate::typedarray::js_typed_array_new_empty(crate::typedarray::KIND_INT16 as i32, 1);
        let cursor = js_segments_view_open(grapheme_segmenter(), js_string("ab"));
        let c = cursor_ptr(cursor).expect("test premise: open returned a cursor");
        let buffer_before = crate::buffer::test_buffer_registry_probe_count();
        let typed_before = crate::typedarray::test_typed_array_registry_probe_count();

        assert_eq!(js_segments_view_next(cursor), 1.0);
        assert_eq!(
            crate::buffer::test_buffer_registry_probe_count(),
            buffer_before,
            "view cursor validation must not enter the Buffer registry"
        );
        assert_eq!(
            crate::typedarray::test_typed_array_registry_probe_count(),
            typed_before,
            "view cursor validation must not enter the typed-array registry"
        );

        let saved = unsafe { (*c).class_id };
        unsafe { (*c).class_id = saved ^ 1 };
        assert_eq!(
            js_segments_view_next(cursor),
            0.0,
            "sabotage: a wrong class id must be rejected by the next entry"
        );
        unsafe { (*c).class_id = saved };
        assert_eq!(js_segments_view_next(cursor), 1.0);
    }

    /// The view entry validates its RegExp argument once. Restoring the inner
    /// helper's duplicate validation makes this delta two; deleting the entry
    /// validation makes it zero.
    #[cfg(feature = "regex-engine")]
    #[test]
    fn view_regexp_pointer_is_validated_exactly_once_per_call() {
        let cursor = js_segments_view_open(grapheme_segmenter(), js_string("a"));
        assert_eq!(js_segments_view_next(cursor), 1.0);
        let re = crate::regex::js_regexp_construct(js_string("a"), js_string(""));
        let regex = f64::from_bits(JSValue::pointer(re as *const u8).bits());
        let before = crate::regex::test_regex_ptr_validation_calls();
        assert!(!is_undefined(js_segments_view_regexp_test(cursor, regex)));
        assert_eq!(
            crate::regex::test_regex_ptr_validation_calls() - before,
            1,
            "one exported call must perform exactly one RegExp brand validation"
        );
    }

    /// SABOTAGE-SHAPED, kept as a test: patching `RegExp.prototype.test` AFTER
    /// the site is recorded must make the very next call decline.
    #[cfg(feature = "regex-engine")]
    #[test]
    fn a_patched_prototype_test_declines_on_the_next_call() {
        let cursor = js_segments_view_open(grapheme_segmenter(), js_string("ab"));
        assert_eq!(js_segments_view_next(cursor), 1.0);
        let re = crate::regex::js_regexp_construct(js_string("[a-z]"), js_string(""));
        let re_v = f64::from_bits(JSValue::pointer(re as *const u8).bits());
        assert!(
            !is_undefined(js_segments_view_regexp_test(cursor, re_v)),
            "accepted before the patch"
        );

        // Replace `RegExp.prototype.test` the way a program would.
        let proto_ptr =
            crate::object::regex_proto_thunks::REGEXP_PROTOTYPE_PTR.load(Ordering::Acquire);
        assert!(proto_ptr != 0, "the site must have been recorded");
        let proto = proto_ptr as *mut ObjectHeader;
        let key = crate::string::js_string_from_bytes(b"test".as_ptr(), 4);
        let replacement = crate::closure::js_closure_alloc(patched_test_thunk as *const u8, 0);
        crate::closure::js_register_closure_arity(patched_test_thunk as *const u8, 0);
        crate::object::js_object_set_field_by_name(
            proto,
            key,
            crate::value::js_nanbox_pointer(replacement as i64),
        );
        assert!(
            is_undefined(js_segments_view_regexp_test(cursor, re_v)),
            "a replaced `RegExp.prototype.test` must make the view DECLINE, so \
             the caller materialises and runs the user's function"
        );
    }

    extern "C" fn patched_test_thunk(_c: *const crate::closure::ClosureHeader) -> f64 {
        f64::from_bits(JSValue::bool(true).bits())
    }

    /// `_regexp_test` answers the same as the materialised call for a plain
    /// regex, and DECLINES (three-valued `undefined`) for a global one, whose
    /// `test` is stateful in `lastIndex`.
    #[cfg(feature = "regex-engine")]
    #[test]
    fn regexp_test_matches_the_materialised_call_and_declines_when_stateful() {
        let cursor = js_segments_view_open(grapheme_segmenter(), js_string("a1"));
        assert_eq!(js_segments_view_next(cursor), 1.0);

        let plain = crate::regex::js_regexp_construct(js_string("^[a-z]$"), js_string(""));
        let plain_v = f64::from_bits(JSValue::pointer(plain as *const u8).bits());
        let seg = js_segments_view_segment(cursor);
        let seg_ptr = JSValue::from_bits(seg.to_bits()).as_string_ptr();
        let materialised = crate::regex::js_regexp_test(plain, seg_ptr) != 0;
        let viewed = js_segments_view_regexp_test(cursor, plain_v);
        assert!(!is_undefined(viewed), "a plain regex must be accepted");
        assert_eq!(
            crate::value::js_is_truthy(viewed) != 0,
            materialised,
            "the view answer must equal the materialised answer"
        );

        let global = crate::regex::js_regexp_construct(js_string("[a-z]"), js_string("g"));
        let global_v = f64::from_bits(JSValue::pointer(global as *const u8).bits());
        assert!(
            is_undefined(js_segments_view_regexp_test(cursor, global_v)),
            "a global regex is stateful in lastIndex and must DECLINE"
        );
    }

    /// The anchors are segment-local: `^`/`$` must bind to the segment's ends,
    /// not the input's. A start-offset match instead of a bounded haystack
    /// would make this pass for the first segment and fail for the second.
    #[cfg(feature = "regex-engine")]
    #[test]
    fn regexp_test_anchors_are_segment_local() {
        let cursor = js_segments_view_open(grapheme_segmenter(), js_string("ab"));
        let anchored = crate::regex::js_regexp_construct(js_string("^b$"), js_string(""));
        let v = f64::from_bits(JSValue::pointer(anchored as *const u8).bits());
        assert_eq!(js_segments_view_next(cursor), 1.0); // "a"
        assert_eq!(
            crate::value::js_is_truthy(js_segments_view_regexp_test(cursor, v)),
            0,
            "^b$ must not match the segment \"a\""
        );
        assert_eq!(js_segments_view_next(cursor), 1.0); // "b"
        assert_ne!(
            crate::value::js_is_truthy(js_segments_view_regexp_test(cursor, v)),
            0,
            "^b$ MUST match the segment \"b\" — the haystack's bounds are the \
             segment's ends, so the anchors are segment-local"
        );
    }
}
