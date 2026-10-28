//! Checked builtin entries (#11915 inline-size rule): the `Pristine` test
//! lives in the CALLEE, once per builtin entry, instead of at every site.
//!
//! A compiled `s.slice(a, b)` on a proven string receiver calls
//! `js_string_m_slice2(s, a, b)` with the operands it evaluated — one call,
//! smaller than the unbox / coerce / call / box sequence it replaces. The
//! entry's first act is the proof:
//!
//! * `Pristine(String)` (an Array entry: `Pristine(Array)` plus the receiver
//!   cell's own proof — no named property of any name, no descriptor, no own
//!   `[[Prototype]]`): the builtin body, the same runtime calls the site used
//!   to emit, in the same order;
//! * otherwise the ONE generic path: `Call(Get(recv, name), recv, args)`
//!   (`builtin_proto::redirect_or_refresh`); a builtin still selected there
//!   runs the body on the re-read operands.
//!
//! Each body is also exported unchecked (`js_string_b_…`, `js_array_b_…`)
//! for a site whose arguments could observe WHEN the method is read
//! (`perry-codegen` `args_may_observe_lookup`): that site reads the byte
//! before them, as ECMA-262 reads the method, evaluates them once, and then
//! calls the body or the generic path. Either way `Pristine` is decided where
//! the specification reads the method.
//!
//! The arity is part of the symbol (`…slice0`, `…slice1`, `…slice2`), so a
//! site passes exactly the arguments it evaluated and the generic path sees
//! the real `arguments.length`. A string body whose operand is an object
//! (its coercion runs user code, and the body holds raw handles) runs the
//! builtin's rooted by-name body instead.

use super::builtin_proto::{pristine, redirect_or_refresh, BuiltinProto};
use crate::string::StringHeader;
use crate::value::{JSValue, POINTER_MASK, POINTER_TAG, STRING_TAG, TAG_UNDEFINED};

use crate::regex::hooked::{js_string_match_all_js, js_string_match_js, js_string_search_js};
use crate::string::{
    js_string_at, js_string_char_at, js_string_char_code_at, js_string_code_point_at,
    js_string_concat, js_string_ends_with, js_string_ends_with_at, js_string_index_of,
    js_string_index_of_from, js_string_last_index_of, js_string_last_index_of_from,
    js_string_locale_compare, js_string_normalize, js_string_pad_end, js_string_pad_start,
    js_string_repeat, js_string_slice, js_string_starts_with, js_string_starts_with_at,
    js_string_substr, js_string_substring, js_string_to_lower_case, js_string_to_upper_case,
    js_string_trim, js_string_trim_end, js_string_trim_start,
};
use crate::value::js_get_string_pointer_unified;

// The regex-backed bodies take boxed operands and are defined per engine
// feature; link to whichever definition is built. `crate::regex` names the
// match/matchAll/search bodies in both configurations; reaching them by
// symbol name left them unresolved in the Windows release link.
extern "C" {
    fn js_string_position_to_index(pos_f64: f64) -> i32;
    fn js_string_split_js(receiver: f64, separator: f64, limit: f64) -> f64;
    fn js_string_replace_js(receiver: f64, search: f64, replacement: f64) -> f64;
    fn js_string_replace_all_js(receiver: f64, search: f64, replacement: f64) -> f64;
}

const UNDEFINED: f64 = f64::from_bits(TAG_UNDEFINED);

#[inline(always)]
fn is_string(v: f64) -> bool {
    JSValue::from_bits(v.to_bits()).is_any_string()
}

#[inline(always)]
fn boxed(p: *mut StringHeader) -> f64 {
    f64::from_bits(JSValue::string_ptr(p).bits())
}

#[inline(always)]
fn boolean(b: bool) -> f64 {
    f64::from_bits(JSValue::bool(b).bits())
}

/// A heap string's header (one mask, no allocation), or `None`.
#[inline(always)]
fn heap_string(v: f64) -> Option<*const StringHeader> {
    let bits = v.to_bits();
    (bits & !POINTER_MASK == STRING_TAG).then_some((bits & POINTER_MASK) as *const StringHeader)
}

/// The fast path's receiver header: a heap string's own, or a short string
/// materialized. That materialization is the ONLY allocation a fast path
/// makes, and it is made first, so no raw header the body reads can be moved
/// under it; every operand is then read without allocating (a heap string's
/// own header, a number).
#[inline(always)]
unsafe fn fast_recv(recv: f64) -> Option<*const StringHeader> {
    if let Some(h) = heap_string(recv) {
        return Some(h);
    }
    if JSValue::from_bits(recv.to_bits()).is_short_string() {
        return Some(js_get_string_pointer_unified(recv) as *const StringHeader);
    }
    None
}

/// `ToIntegerOrInfinity` clamped to `i32` for an int32 or a plain number
/// (exactly `js_string_index_to_i32` on those); `None` for any value whose
/// conversion may allocate, run user code or throw.
#[inline(always)]
fn number_index(v: f64) -> Option<i32> {
    let jsval = JSValue::from_bits(v.to_bits());
    if jsval.is_int32() {
        Some(jsval.as_int32())
    } else if jsval.is_number() {
        Some(v as i32)
    } else {
        None
    }
}

/// `ToNumber` of an int32 or a plain number; `None` otherwise.
#[inline(always)]
fn number_value(v: f64) -> Option<f64> {
    let jsval = JSValue::from_bits(v.to_bits());
    if jsval.is_int32() {
        Some(f64::from(jsval.as_int32()))
    } else if jsval.is_number() {
        Some(v)
    } else {
        None
    }
}

/// An optional index operand on the fast path: absent is `default`.
#[inline(always)]
fn opt_index(v: Option<f64>, default: i32) -> Option<i32> {
    match v {
        None => Some(default),
        Some(v) => number_index(v),
    }
}

/// A receiver or operand the inline body does not take (not a string, or an
/// object operand whose coercion runs user code): the builtin's native body
/// by name, which roots its operands; a non-string receiver takes the
/// ordinary method dispatch.
#[cold]
#[inline(never)]
#[optimize(size)]
unsafe fn string_by_name(recv: f64, name: &[u8], args: &[f64]) -> f64 {
    // SAFETY: every name below is an ASCII literal.
    let name_str = std::str::from_utf8_unchecked(name);
    if let Some(value) = super::string_native_method(recv, name_str, args) {
        return value;
    }
    super::js_native_call_method(
        recv,
        name.as_ptr().cast(),
        name.len(),
        args.as_ptr(),
        args.len(),
    )
}

/// The failed proof: `Call(Get(recv, name), recv, args)`. `false` when the
/// method actually there is still the builtin (operands re-read).
#[cold]
#[inline(never)]
#[optimize(size)]
unsafe fn string_redirect(recv: &mut f64, name: &[u8], args: &mut [f64], out: &mut f64) -> bool {
    match redirect_or_refresh(recv, name, args) {
        Some(value) => {
            *out = value;
            true
        }
        None => false,
    }
}

type Body0 = unsafe extern "C-unwind" fn(f64) -> f64;
type Body1 = unsafe extern "C-unwind" fn(f64, f64) -> f64;
type Body2 = unsafe extern "C-unwind" fn(f64, f64, f64) -> f64;

/// A checked entry's miss, shared by every entry of one arity so each entry
/// is the test and two tail calls: the exact proof and generic path
/// ([`array_slow`] for an Array entry, [`string_redirect`] for a string
/// one), then — when the builtin is still selected — the body on the
/// re-read operands.
#[cold]
#[inline(never)]
#[optimize(size)]
unsafe fn miss(array: bool, recv: &mut f64, name: &'static [u8], args: &mut [f64]) -> Option<f64> {
    let mut out = UNDEFINED;
    let redirected = if array {
        array_slow(recv, name, args, &mut out)
    } else {
        string_redirect(recv, name, args, &mut out)
    };
    redirected.then_some(out)
}

#[cold]
#[inline(never)]
#[optimize(size)]
unsafe fn cold0(array: bool, mut recv: f64, name: &'static [u8], body: Body0) -> f64 {
    match miss(array, &mut recv, name, &mut []) {
        Some(value) => value,
        None => body(recv),
    }
}

#[cold]
#[inline(never)]
#[optimize(size)]
unsafe fn cold1(array: bool, mut recv: f64, a: f64, name: &'static [u8], body: Body1) -> f64 {
    let mut args = [a];
    match miss(array, &mut recv, name, &mut args) {
        Some(value) => value,
        None => body(recv, args[0]),
    }
}

#[cold]
#[inline(never)]
#[optimize(size)]
unsafe fn cold2(
    array: bool,
    mut recv: f64,
    a: f64,
    b: f64,
    name: &'static [u8],
    body: Body2,
) -> f64 {
    let mut args = [a, b];
    match miss(array, &mut recv, name, &mut args) {
        Some(value) => value,
        None => body(recv, args[0], args[1]),
    }
}

macro_rules! cold {
    ($array:expr, $r:ident, [], $n:expr, $b:expr) => {
        cold0($array, $r, $n, $b)
    };
    ($array:expr, $r:ident, [$a:ident], $n:expr, $b:expr) => {
        cold1($array, $r, $a, $n, $b)
    };
    ($array:expr, $r:ident, [$a:ident, $c:ident], $n:expr, $b:expr) => {
        cold2($array, $r, $a, $c, $n, $b)
    };
}

/// Two symbols per (method, argument count):
///
/// * `js_string_b_<m><n>` — the builtin's body, NOT checked: for a site that
///   proved `Pristine(String)` itself, before arguments whose evaluation could
///   observe the method read.
/// * `js_string_m_<m><n>` — the checked entry: `Pristine(String)` first, then
///   the body (expanded in place: an exported function is never inlined); a
///   miss takes the ONE generic path.
///
/// A body's fast path takes a string receiver and operands that need no
/// allocation, no user code and no throw (heap strings, numbers, absent);
/// everything else goes to [`string_by_name`], which roots the receiver and
/// the operands before any coercion. `any` bodies hand boxed operands to a
/// runtime body that roots them itself.
macro_rules! string_entries {
    ($( $m:ident / $b:ident = $name:literal, $mode:ident, $recv:ident($($a:ident),*) => $body:expr; )*) => {$(
        #[no_mangle]
        pub unsafe extern "C-unwind" fn $b($recv: f64 $(, $a: f64)*) -> f64 {
            string_entries!(@body $mode, $name, $recv, [$($a),*], $body)
        }

        #[no_mangle]
        pub unsafe extern "C-unwind" fn $m($recv: f64 $(, $a: f64)*) -> f64 {
            if !pristine(BuiltinProto::String) {
                return cold!(false, $recv, [$($a),*], $name, $b);
            }
            string_entries!(@body $mode, $name, $recv, [$($a),*], $body)
        }
    )*};
    // The body returns `Some(value)` from its fast path, `None` for the rooted
    // by-name body.
    (@body own, $name:literal, $recv:ident, [$($a:ident),*], $body:expr) => {
        #[allow(clippy::redundant_closure_call)]
        match (|| -> Option<f64> { $body })() {
            Some(value) => value,
            None => string_by_name($recv, $name, &[$($a),*]),
        }
    };
    (@body any, $name:literal, $recv:ident, [$($a:ident),*], $body:expr) => {
        if is_string($recv) {
            $body
        } else {
            string_by_name($recv, $name, &[$($a),*])
        }
    };
}

#[inline(always)]
unsafe fn slice_fast(
    recv: f64,
    start: Option<f64>,
    end: Option<f64>,
    substring: bool,
) -> Option<f64> {
    let h = fast_recv(recv)?;
    let len = *(h as *const i32);
    let end_index = match end {
        Some(e) if e.to_bits() == TAG_UNDEFINED => Some(len),
        other => opt_index(other, len),
    }?;
    let start_index = opt_index(start, 0)?;
    Some(boxed(if substring {
        js_string_substring(h, start_index, end_index)
    } else {
        js_string_slice(h, start_index, end_index)
    }))
}

#[inline(always)]
unsafe fn index_of_fast(recv: f64, needle: f64, from: Option<f64>) -> Option<f64> {
    let h = fast_recv(recv)?;
    let n = heap_string(needle)?;
    Some(f64::from(match from {
        None => js_string_index_of(h, n),
        Some(f) => js_string_index_of_from(h, n, number_index(f)?),
    }))
}

#[inline(always)]
unsafe fn last_index_of_fast(recv: f64, needle: f64, pos: Option<f64>) -> Option<f64> {
    let h = fast_recv(recv)?;
    let n = heap_string(needle)?;
    Some(f64::from(match pos {
        None => js_string_last_index_of(h, n),
        Some(p) => js_string_last_index_of_from(h, n, number_value(p)?, 1),
    }))
}

#[inline(always)]
unsafe fn includes_fast(recv: f64, needle: f64, pos: Option<f64>) -> Option<f64> {
    let h = fast_recv(recv)?;
    let n = heap_string(needle)?;
    let from = match pos {
        None => 0,
        Some(p) => {
            number_value(p)?;
            js_string_position_to_index(p)
        }
    };
    Some(boolean(js_string_index_of_from(h, n, from) != -1))
}

#[inline(always)]
unsafe fn starts_ends_fast(recv: f64, other: f64, pos: Option<f64>, ends: bool) -> Option<f64> {
    let h = fast_recv(recv)?;
    let o = heap_string(other)?;
    let r = match (pos, ends) {
        (Some(p), false) => js_string_starts_with_at(h, o, number_index(p)?),
        (Some(p), true) => js_string_ends_with_at(h, o, number_index(p)?),
        (None, false) => js_string_starts_with(h, o),
        (None, true) => js_string_ends_with(h, o),
    };
    Some(boolean(r != 0))
}

/// The one-index readers.
#[derive(Clone, Copy)]
enum CharOp {
    CharAt,
    CharCodeAt,
    CodePointAt,
    At,
}

#[inline(always)]
unsafe fn char_fast(recv: f64, idx: Option<f64>, op: CharOp) -> Option<f64> {
    let h = fast_recv(recv)?;
    let i = opt_index(idx, 0)?;
    Some(match op {
        CharOp::CharAt => boxed(js_string_char_at(h, i)),
        CharOp::CharCodeAt => js_string_char_code_at(h, i),
        CharOp::CodePointAt => js_string_code_point_at(h, i),
        CharOp::At => js_string_at(h, i),
    })
}

#[inline(always)]
unsafe fn pad_fast(recv: f64, len: f64, fill: Option<f64>, end: bool) -> Option<f64> {
    let len = number_value(len)?;
    let h = fast_recv(recv)?;
    // Absent → null: the runtime pads with " ".
    let fill = match fill {
        None => std::ptr::null(),
        Some(f) if f.to_bits() == TAG_UNDEFINED => std::ptr::null(),
        Some(f) => heap_string(f)?,
    };
    Some(boxed(if end {
        js_string_pad_end(h, len, fill)
    } else {
        js_string_pad_start(h, len, fill)
    }))
}

/// `substr` and `repeat` coerce their number operands themselves.
#[inline(always)]
fn numbers_or_undefined(ops: &[f64]) -> Option<()> {
    for v in ops {
        if v.to_bits() != TAG_UNDEFINED {
            number_value(*v)?;
        }
    }
    Some(())
}

#[inline(always)]
unsafe fn unary_fast(
    recv: f64,
    op: unsafe extern "C" fn(*const StringHeader) -> *mut StringHeader,
) -> Option<f64> {
    Some(boxed(op(fast_recv(recv)?)))
}

string_entries! {
    js_string_m_slice0 / js_string_b_slice0 = b"slice", own, recv() => slice_fast(recv, None, None, false);
    js_string_m_slice1 / js_string_b_slice1 = b"slice", own, recv(a) => slice_fast(recv, Some(a), None, false);
    js_string_m_slice2 / js_string_b_slice2 = b"slice", own, recv(a, b) => slice_fast(recv, Some(a), Some(b), false);
    js_string_m_substring0 / js_string_b_substring0 = b"substring", own, recv() => slice_fast(recv, None, None, true);
    js_string_m_substring1 / js_string_b_substring1 = b"substring", own, recv(a) => slice_fast(recv, Some(a), None, true);
    js_string_m_substring2 / js_string_b_substring2 = b"substring", own, recv(a, b) => slice_fast(recv, Some(a), Some(b), true);
    js_string_m_substr1 / js_string_b_substr1 = b"substr", own, recv(a) => {
        numbers_or_undefined(&[a]).and_then(|()| Some(boxed(js_string_substr(fast_recv(recv)?, a, UNDEFINED))))
    };
    js_string_m_substr2 / js_string_b_substr2 = b"substr", own, recv(a, b) => {
        numbers_or_undefined(&[a, b]).and_then(|()| Some(boxed(js_string_substr(fast_recv(recv)?, a, b))))
    };
    js_string_m_indexOf0 / js_string_b_indexOf0 = b"indexOf", own, recv() => None;
    js_string_m_indexOf1 / js_string_b_indexOf1 = b"indexOf", own, recv(a) => index_of_fast(recv, a, None);
    js_string_m_indexOf2 / js_string_b_indexOf2 = b"indexOf", own, recv(a, b) => index_of_fast(recv, a, Some(b));
    js_string_m_lastIndexOf0 / js_string_b_lastIndexOf0 = b"lastIndexOf", own, recv() => None;
    js_string_m_lastIndexOf1 / js_string_b_lastIndexOf1 = b"lastIndexOf", own, recv(a) => last_index_of_fast(recv, a, None);
    js_string_m_lastIndexOf2 / js_string_b_lastIndexOf2 = b"lastIndexOf", own, recv(a, b) => last_index_of_fast(recv, a, Some(b));
    js_string_m_includes0 / js_string_b_includes0 = b"includes", own, recv() => None;
    js_string_m_includes1 / js_string_b_includes1 = b"includes", own, recv(a) => includes_fast(recv, a, None);
    js_string_m_includes2 / js_string_b_includes2 = b"includes", own, recv(a, b) => includes_fast(recv, a, Some(b));
    js_string_m_startsWith0 / js_string_b_startsWith0 = b"startsWith", own, recv() => None;
    js_string_m_startsWith1 / js_string_b_startsWith1 = b"startsWith", own, recv(a) => starts_ends_fast(recv, a, None, false);
    js_string_m_startsWith2 / js_string_b_startsWith2 = b"startsWith", own, recv(a, b) => starts_ends_fast(recv, a, Some(b), false);
    js_string_m_endsWith0 / js_string_b_endsWith0 = b"endsWith", own, recv() => None;
    js_string_m_endsWith1 / js_string_b_endsWith1 = b"endsWith", own, recv(a) => starts_ends_fast(recv, a, None, true);
    js_string_m_endsWith2 / js_string_b_endsWith2 = b"endsWith", own, recv(a, b) => starts_ends_fast(recv, a, Some(b), true);
    js_string_m_charAt0 / js_string_b_charAt0 = b"charAt", own, recv() => char_fast(recv, None, CharOp::CharAt);
    js_string_m_charAt1 / js_string_b_charAt1 = b"charAt", own, recv(a) => char_fast(recv, Some(a), CharOp::CharAt);
    js_string_m_charCodeAt0 / js_string_b_charCodeAt0 = b"charCodeAt", own, recv() => char_fast(recv, None, CharOp::CharCodeAt);
    js_string_m_charCodeAt1 / js_string_b_charCodeAt1 = b"charCodeAt", own, recv(a) => char_fast(recv, Some(a), CharOp::CharCodeAt);
    js_string_m_codePointAt0 / js_string_b_codePointAt0 = b"codePointAt", own, recv() => char_fast(recv, None, CharOp::CodePointAt);
    js_string_m_codePointAt1 / js_string_b_codePointAt1 = b"codePointAt", own, recv(a) => char_fast(recv, Some(a), CharOp::CodePointAt);
    js_string_m_at0 / js_string_b_at0 = b"at", own, recv() => char_fast(recv, None, CharOp::At);
    js_string_m_at1 / js_string_b_at1 = b"at", own, recv(a) => char_fast(recv, Some(a), CharOp::At);
    js_string_m_split0 / js_string_b_split0 = b"split", any, recv() => js_string_split_js(recv, UNDEFINED, UNDEFINED);
    js_string_m_split1 / js_string_b_split1 = b"split", any, recv(a) => js_string_split_js(recv, a, UNDEFINED);
    js_string_m_split2 / js_string_b_split2 = b"split", any, recv(a, b) => js_string_split_js(recv, a, b);
    js_string_m_replace0 / js_string_b_replace0 = b"replace", any, recv() => js_string_replace_js(recv, UNDEFINED, UNDEFINED);
    js_string_m_replace1 / js_string_b_replace1 = b"replace", any, recv(a) => js_string_replace_js(recv, a, UNDEFINED);
    js_string_m_replace2 / js_string_b_replace2 = b"replace", any, recv(a, b) => js_string_replace_js(recv, a, b);
    js_string_m_replaceAll0 / js_string_b_replaceAll0 = b"replaceAll", any, recv() => js_string_replace_all_js(recv, UNDEFINED, UNDEFINED);
    js_string_m_replaceAll1 / js_string_b_replaceAll1 = b"replaceAll", any, recv(a) => js_string_replace_all_js(recv, a, UNDEFINED);
    js_string_m_replaceAll2 / js_string_b_replaceAll2 = b"replaceAll", any, recv(a, b) => js_string_replace_all_js(recv, a, b);
    js_string_m_match0 / js_string_b_match0 = b"match", any, recv() => js_string_match_js(recv, UNDEFINED);
    js_string_m_match1 / js_string_b_match1 = b"match", any, recv(a) => js_string_match_js(recv, a);
    js_string_m_matchAll0 / js_string_b_matchAll0 = b"matchAll", any, recv() => js_string_match_all_js(recv, UNDEFINED);
    js_string_m_matchAll1 / js_string_b_matchAll1 = b"matchAll", any, recv(a) => js_string_match_all_js(recv, a);
    js_string_m_search0 / js_string_b_search0 = b"search", any, recv() => js_string_search_js(recv, UNDEFINED);
    js_string_m_search1 / js_string_b_search1 = b"search", any, recv(a) => js_string_search_js(recv, a);
    js_string_m_toLowerCase0 / js_string_b_toLowerCase0 = b"toLowerCase", own, recv() => unary_fast(recv, js_string_to_lower_case);
    js_string_m_toUpperCase0 / js_string_b_toUpperCase0 = b"toUpperCase", own, recv() => unary_fast(recv, js_string_to_upper_case);
    js_string_m_trim0 / js_string_b_trim0 = b"trim", own, recv() => unary_fast(recv, js_string_trim);
    js_string_m_trimStart0 / js_string_b_trimStart0 = b"trimStart", own, recv() => unary_fast(recv, js_string_trim_start);
    js_string_m_trimEnd0 / js_string_b_trimEnd0 = b"trimEnd", own, recv() => unary_fast(recv, js_string_trim_end);
    js_string_m_padStart1 / js_string_b_padStart1 = b"padStart", own, recv(a) => pad_fast(recv, a, None, false);
    js_string_m_padStart2 / js_string_b_padStart2 = b"padStart", own, recv(a, b) => pad_fast(recv, a, Some(b), false);
    js_string_m_padEnd1 / js_string_b_padEnd1 = b"padEnd", own, recv(a) => pad_fast(recv, a, None, true);
    js_string_m_padEnd2 / js_string_b_padEnd2 = b"padEnd", own, recv(a, b) => pad_fast(recv, a, Some(b), true);
    js_string_m_repeat1 / js_string_b_repeat1 = b"repeat", own, recv(a) => {
        numbers_or_undefined(&[a]).and_then(|()| Some(boxed(js_string_repeat(fast_recv(recv)?, a))))
    };
    js_string_m_localeCompare0 / js_string_b_localeCompare0 = b"localeCompare", own, recv() => None;
    js_string_m_localeCompare1 / js_string_b_localeCompare1 = b"localeCompare", own, recv(a) => {
        fast_recv(recv).and_then(|h| Some(js_string_locale_compare(h, heap_string(a)?)))
    };
    js_string_m_normalize0 / js_string_b_normalize0 = b"normalize", own, recv() => {
        Some(boxed(js_string_normalize(fast_recv(recv)?, UNDEFINED)))
    };
    js_string_m_normalize1 / js_string_b_normalize1 = b"normalize", own, recv(a) => {
        let h = fast_recv(recv)?;
        heap_string(a).map(|_| boxed(js_string_normalize(h, a)))
    };
    js_string_m_concat0 / js_string_b_concat0 = b"concat", own, recv() => is_string(recv).then_some(recv);
    js_string_m_concat1 / js_string_b_concat1 = b"concat", own, recv(a) => {
        let h = fast_recv(recv)?;
        heap_string(a).map(|n| boxed(js_string_concat(h, n)))
    };
    js_string_m_concat2 / js_string_b_concat2 = b"concat", own, recv(a, b) => None;
}

#[cfg(test)]
#[path = "builtin_proto_entries_tests.rs"]
mod tests;

// ---------------------------------------------------------------------------
// Array: the folded `arr.<method>(…)` nodes.
//
// `Pristine(Array)` alone is not the receiver's proof: an array may own a
// property of the method's name, or carry its own `[[Prototype]]`. Both are
// recorded on the cell (`GC_ARRAY_KEY_ATTRS`, the descriptor flag,
// `GC_ARRAY_CUSTOM_PROTO`) — the same bits the emitted own-override guard
// reads — and every other receiver kind answers from the own-override
// predicate's global gate. A miss takes the real `Get`; a builtin found
// there runs the body below on the re-read operands.

#[path = "builtin_array_abi.rs"]
mod array_abi;
use array_abi::*;

extern "C" {
    fn js_receiver_may_own_named_method(recv: f64, key: f64) -> i32;
    fn js_validate_array_callback(cb_boxed: f64) -> i64;
    fn js_validate_array_map_callback(arr: i64, cb_boxed: f64) -> i64;
    fn js_validate_array_comparator(cmp_boxed: f64) -> i64;

}

/// Cell bits that say an array may not read `name` from
/// `%Array.prototype%`: its own `[[Prototype]]`. A named property lives in the
/// array's named layout (`ArrayHeader.named`), tested beside these bits.
const ARRAY_NOT_DEFAULT: u16 = crate::gc::GC_ARRAY_KEY_ATTRS | crate::gc::GC_ARRAY_CUSTOM_PROTO;

/// Does the receiver own a property named `name`? The own-override
/// predicate takes the method's interned key, and interning a name the
/// thread has not used yet allocates — so the receiver and the operands are
/// rooted across it and re-read (into `recv`/`args`) after.
#[inline(never)]
unsafe fn may_own_named(recv: &mut f64, name: &[u8], args: &mut [f64]) -> bool {
    let scope = crate::gc::RuntimeHandleScope::new();
    let r = scope.root_nanbox_f64(*recv);
    let rooted: Vec<_> = args.iter().map(|a| scope.root_nanbox_f64(*a)).collect();
    let key = crate::string::canonical_key(name);
    *recv = r.get_nanbox_f64();
    for (a, h) in args.iter_mut().zip(&rooted) {
        *a = h.get_nanbox_f64();
    }
    key.is_null()
        || js_receiver_may_own_named_method(
            *recv,
            f64::from_bits(crate::JSValue::string_ptr(key).bits()),
        ) != 0
}

/// Does `recv.name` resolve to `%Array.prototype%`'s builtin? `Pristine(Array)`
/// plus the receiver's own proof; anything not proven answers no. May
/// allocate (see [`may_own_named`]); `recv` and `args` are refreshed.
#[inline]
unsafe fn array_admits(recv: &mut f64, name: &[u8], args: &mut [f64]) -> bool {
    if !pristine(BuiltinProto::Array) {
        return false;
    }
    let bits = recv.to_bits();
    if bits & !POINTER_MASK != POINTER_TAG {
        // Not an object (a stream handle id, …): owns nothing.
        return true;
    }
    let addr = (bits & POINTER_MASK) as usize;
    match crate::value::addr_class::try_read_gc_header(addr) {
        Some(h) if h.obj_type == crate::gc::GC_TYPE_PROXY => false,
        Some(h)
            if matches!(
                h.obj_type,
                crate::gc::GC_TYPE_ARRAY | crate::gc::GC_TYPE_LAZY_ARRAY
            ) =>
        {
            h._reserved & ARRAY_NOT_DEFAULT == 0
                || (h._reserved & crate::gc::GC_ARRAY_CUSTOM_PROTO == 0
                    && !may_own_named(recv, name, args))
        }
        _ => {
            super::own_override::PERRY_OWN_NAMED_PROP_INSTALLED
                .load(std::sync::atomic::Ordering::Relaxed)
                == 0
                || !may_own_named(recv, name, args)
        }
    }
}

#[inline(always)]
fn arr(recv: f64) -> i64 {
    (recv.to_bits() & POINTER_MASK) as i64
}

#[inline(always)]
fn ptr_boxed(p: i64) -> f64 {
    f64::from_bits(POINTER_TAG | p as u64)
}

/// The inline half of [`array_admits`]: `Pristine(Array)` and a plain array
/// cell whose header records no named property, descriptor or own
/// `[[Prototype]]` — one byte load and one header word. Anything else asks
/// [`array_slow`].
#[inline(always)]
unsafe fn array_fast_admits(recv: f64) -> bool {
    if !pristine(BuiltinProto::Array) {
        return false;
    }
    let bits = recv.to_bits();
    if bits & !POINTER_MASK != POINTER_TAG {
        return true;
    }
    let Some(header) = crate::value::addr_class::pointer_value_header(bits) else {
        return false;
    };
    header.obj_type == crate::gc::GC_TYPE_ARRAY && header._reserved & ARRAY_NOT_DEFAULT == 0
}

/// The out-of-line half: the exact admission, and on a miss the real `Get`
/// and call. `false`: run the body (on re-read operands).
#[cold]
#[inline(never)]
#[optimize(size)]
unsafe fn array_slow(recv: &mut f64, name: &[u8], args: &mut [f64], out: &mut f64) -> bool {
    if array_admits(recv, name, args) {
        return false;
    }
    match redirect_or_refresh(recv, name, args) {
        Some(value) => {
            *out = value;
            true
        }
        None => false,
    }
}

/// Two symbols per (method, argument count), as for strings:
/// `js_array_b_<m><n>` is the fold's body, NOT checked (a site that proved
/// the fact before arguments that could observe the method read);
/// `js_array_m_<m><n>` is the checked entry.
macro_rules! array_entries {
    ($( $m:ident / $b:ident = $name:literal, $recv:ident($($a:ident),*) => $body:expr; )*) => {$(
        #[no_mangle]
        pub unsafe extern "C-unwind" fn $b($recv: f64 $(, $a: f64)*) -> f64 {
            $body
        }

        // The body (a call or two) is expanded here too: one frame per call.
        #[no_mangle]
        pub unsafe extern "C-unwind" fn $m($recv: f64 $(, $a: f64)*) -> f64 {
            if !array_fast_admits($recv) {
                return cold!(true, $recv, [$($a),*], $name, $b);
            }
            $body
        }
    )*};
}

array_entries! {
    js_array_m_forEach1 / js_array_b_forEach1 = b"forEach", recv(a) => {
        let cb = js_validate_array_callback(a);
        js_array_forEach(arr(recv), cb);
        UNDEFINED
    };
    js_array_m_map1 / js_array_b_map1 = b"map", recv(a) => {
        let h = arr(recv);
        ptr_boxed(js_array_map(h, js_validate_array_map_callback(h, a)))
    };
    js_array_m_mapDiscard1 / js_array_b_mapDiscard1 = b"map", recv(a) => {
        let h = arr(recv);
        js_array_map_discard(h, js_validate_array_map_callback(h, a));
        UNDEFINED
    };
    js_array_m_filter1 / js_array_b_filter1 = b"filter", recv(a) => ptr_boxed(js_array_filter(arr(recv), js_validate_array_callback(a)));
    js_array_m_flatMap1 / js_array_b_flatMap1 = b"flatMap", recv(a) => ptr_boxed(js_array_flatMap(arr(recv), js_validate_array_callback(a)));
    js_array_m_find1 / js_array_b_find1 = b"find", recv(a) => js_array_find(arr(recv), js_validate_array_callback(a));
    js_array_m_findLast1 / js_array_b_findLast1 = b"findLast", recv(a) => js_array_find_last(arr(recv), js_validate_array_callback(a));
    js_array_m_findIndex1 / js_array_b_findIndex1 = b"findIndex", recv(a) => f64::from(js_array_findIndex(arr(recv), js_validate_array_callback(a)));
    js_array_m_findLastIndex1 / js_array_b_findLastIndex1 = b"findLastIndex", recv(a) => f64::from(js_array_find_last_index(arr(recv), js_validate_array_callback(a)));
    js_array_m_some1 / js_array_b_some1 = b"some", recv(a) => js_array_some(arr(recv), js_validate_array_callback(a));
    js_array_m_every1 / js_array_b_every1 = b"every", recv(a) => js_array_every(arr(recv), js_validate_array_callback(a));
    js_array_m_sort1 / js_array_b_sort1 = b"sort", recv(a) => ptr_boxed(js_array_sort_with_comparator(arr(recv), js_validate_array_comparator(a)));
    js_array_m_reduce1 / js_array_b_reduce1 = b"reduce", recv(a) => {
        let cb = js_validate_array_callback(a);
        js_array_reduce(arr(recv), cb, 0, 0.0)
    };
    js_array_m_reduce2 / js_array_b_reduce2 = b"reduce", recv(a, b) => {
        let cb = js_validate_array_callback(a);
        js_array_reduce(arr(recv), cb, 1, b)
    };
    js_array_m_reduceRight1 / js_array_b_reduceRight1 = b"reduceRight", recv(a) => {
        let cb = js_validate_array_callback(a);
        js_array_reduce_right(arr(recv), cb, 0, 0.0)
    };
    js_array_m_reduceRight2 / js_array_b_reduceRight2 = b"reduceRight", recv(a, b) => {
        let cb = js_validate_array_callback(a);
        js_array_reduce_right(arr(recv), cb, 1, b)
    };
    js_array_m_indexOf1 / js_array_b_indexOf1 = b"indexOf", recv(a) => js_array_indexOf_jsvalue(arr(recv), a, a, 0) as f64;
    js_array_m_indexOf2 / js_array_b_indexOf2 = b"indexOf", recv(a, b) => js_array_indexOf_jsvalue(arr(recv), a, b, 1) as f64;
    js_array_m_lastIndexOf1 / js_array_b_lastIndexOf1 = b"lastIndexOf", recv(a) => js_array_last_index_of_jsvalue(arr(recv), a, a, 0) as f64;
    js_array_m_lastIndexOf2 / js_array_b_lastIndexOf2 = b"lastIndexOf", recv(a, b) => js_array_last_index_of_jsvalue(arr(recv), a, b, 1) as f64;
    js_array_m_includes1 / js_array_b_includes1 = b"includes", recv(a) => boolean(js_array_includes_jsvalue(arr(recv), a, a, 0) != 0);
    js_array_m_includes2 / js_array_b_includes2 = b"includes", recv(a, b) => boolean(js_array_includes_jsvalue(arr(recv), a, b, 1) != 0);
    js_array_m_at1 / js_array_b_at1 = b"at", recv(a) => js_array_at(arr(recv), a);
    js_array_m_join0 / js_array_b_join0 = b"join", recv() => boxed(js_array_join_value(arr(recv), UNDEFINED));
    js_array_m_join1 / js_array_b_join1 = b"join", recv(a) => boxed(js_array_join_value(arr(recv), a));
    js_array_m_flat0 / js_array_b_flat0 = b"flat", recv() => ptr_boxed(js_array_flat(arr(recv)));
    js_array_m_toReversed0 / js_array_b_toReversed0 = b"toReversed", recv() => ptr_boxed(js_array_to_reversed(arr(recv)));
    js_array_m_toSorted0 / js_array_b_toSorted0 = b"toSorted", recv() => ptr_boxed(js_array_to_sorted_default(arr(recv)));
    js_array_m_toSorted1 / js_array_b_toSorted1 = b"toSorted", recv(a) => ptr_boxed(js_array_to_sorted_with_comparator(arr(recv), js_validate_array_comparator(a)));
    js_array_m_with2 / js_array_b_with2 = b"with", recv(a, b) => ptr_boxed(js_array_with(arr(recv), a, b));
    js_array_m_entries0 / js_array_b_entries0 = b"entries", recv() => ptr_boxed(js_array_entries_iter_obj(recv.to_bits() as i64));
    js_array_m_keys0 / js_array_b_keys0 = b"keys", recv() => ptr_boxed(js_array_keys_iter_obj(recv.to_bits() as i64));
    js_array_m_values0 / js_array_b_values0 = b"values", recv() => ptr_boxed(js_array_values_iter_obj(recv.to_bits() as i64));
    js_array_m_slice1 / js_array_b_slice1 = b"slice", recv(a) => ptr_boxed(js_array_slice_values(arr(recv), a, UNDEFINED));
    js_array_m_slice2 / js_array_b_slice2 = b"slice", recv(a, b) => ptr_boxed(js_array_slice_values(arr(recv), a, b));
    js_array_m_shift0 / js_array_b_shift0 = b"shift", recv() => js_array_shift_f64(arr(recv));
}
