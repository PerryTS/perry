//! NaN-boxed operand entry points for the compiled `path.*` fast paths (#7621).
//!
//! # The bug these exist to close
//!
//! Every `path.*` codegen arm used to unbox its operand with `unbox_to_i64` —
//! `bitcast double → i64; and POINTER_MASK` — and hand the low 48 bits to a
//! runtime entry that dereferences them as a `*const StringHeader`. That is the
//! right answer for a HEAP string (STRING_TAG = 0x7FFF, payload = the header),
//! and it is the CHARACTERS for a small-string-optimized one (SHORT_STRING_TAG
//! = 0x7FF9, payload = length + up to `SHORT_STRING_MAX_LEN` = 5 inline bytes).
//! So `path.resolve("/root", "s1")` worked — a literal is interned onto the heap
//! — and `path.resolve("/root", seg(1))` threw `ERR_INVALID_ARG_TYPE`, because a
//! computed short string takes the inline form. The #214 class, bisected by
//! length: 5 bytes throws, 6 bytes works.
//!
//! # Why this is not `js_get_string_pointer_unified`
//!
//! [`js_path_arg_header`] is deliberately NOT that helper. `unified` coerces
//! non-strings — a number arrives back as `"5"` — which would silently turn
//! `path.isAbsolute(5)` from Node's `ERR_INVALID_ARG_TYPE` throw into `false`.
//! This one changes the SSO case and NOTHING else: a heap string and every
//! non-string reproduce the old mask bit for bit, so each entry point keeps its
//! own established non-string behaviour (`js_path_join` throws,
//! `js_path_matches_glob` defaults to `""`) unexamined and unchanged.
//!
//! # Why the two-operand entries exist
//!
//! Materialising an inline operand ALLOCATES, which is a collection point
//! between the two operand unboxes — the #7213 window. Codegen cannot close it:
//! `rooting::with_operands_rooted` hands the lowering registers, not slots, so
//! the second operand's register is stale the moment the first is materialised
//! and there is no re-read to reach for (`RootedSlot` deliberately has no
//! `read`). Doing both unboxes inside ONE runtime call moves the window
//! somewhere it can be closed properly — with a `RuntimeHandleScope` and
//! `RuntimeHandle::across_const`, which never binds the pre-collection address.

use super::StringHeader;
use crate::gc::RuntimeHandleScope;
use crate::string::js_string_materialize_to_heap;
use crate::value::{JSValue, POINTER_MASK};

/// Resolve a NaN-boxed `path.*` operand to the `*const StringHeader` the
/// pointer-ABI entry points expect.
///
/// SSO operands are materialised onto the heap; everything else — heap strings
/// AND non-strings — reproduces the pre-#7621 `unbox_to_i64` mask exactly. See
/// the module docs for why that asymmetry is the point.
#[no_mangle]
pub extern "C" fn js_path_arg_header(value: f64) -> i64 {
    path_arg_header(value) as i64
}

/// Keepalive anchor: emitted only from generated code, so the whole-program
/// auto-optimize bitcode pass would otherwise dead-strip it.
#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_PATH_ARG_HEADER: extern "C" fn(f64) -> i64 = js_path_arg_header;

pub(crate) fn path_arg_header(value: f64) -> *const StringHeader {
    let bits = value.to_bits();
    if JSValue::from_bits(bits).is_short_string() {
        return js_string_materialize_to_heap(value) as *const StringHeader;
    }
    (bits & POINTER_MASK) as *const StringHeader
}

/// Materialise both operands and run `f` over the results.
///
/// The first operand is rooted across the second's materialisation, and its
/// address is re-read through `RuntimeHandle::across_const` rather than bound
/// before the call — the ordering half of the rooting invariant that
/// `docs/src/internals/gc-rooting-invariant.md` says keeps getting dropped.
///
/// A non-string operand is a masked garbage address (see [`path_arg_header`]),
/// so it is deliberately NOT rooted: handing the collector something to mark
/// that is not an object is a worse bug than the one being fixed. Nothing can
/// move it either, so reading it unrooted is sound by construction.
///
/// `f` receives raw pointers, and every callee wired up below reads BOTH
/// headers into owned Rust `String`s before it allocates, so neither can be
/// relocated out from under the other inside `f`. The scope stays alive across
/// `f` regardless, so both operands remain reachable for its whole duration.
///
/// # Honest status of the rooting half
///
/// This ordering is DEFENSIVE, not sabotage-proven. Under today's collector the
/// window cannot actually fault: the second materialisation allocates, and a
/// collection reached from inside an allocation runs with `GC_FLAG_IN_ALLOC`
/// set, which makes the copying minor ineligible — so nothing MOVES at an
/// allocation point and the pre-bound address stays valid. Reverting this to
/// `let a_ptr = a_ptr0;` was measured against 400k SSO-pair joins under
/// `PERRY_GC_FORCE_EVACUATE=1` + from-space protection and against the #7621 gap
/// test under the rate-1 seeded schedule (402k copying minors): zero faults, byte-
/// identical output. It is written this way because it is the shape the
/// invariant asks for and it costs nothing, not because an instrument caught it.
fn with_two_headers<R>(
    a: f64,
    b: f64,
    f: impl FnOnce(*const StringHeader, *const StringHeader) -> R,
) -> R {
    let scope = RuntimeHandleScope::new();
    let a_ptr0 = path_arg_header(a);
    let a_handle = JSValue::from_bits(a.to_bits())
        .is_any_string()
        .then(|| scope.root_string_ptr(a_ptr0));
    let (b_ptr, a_ptr) = match &a_handle {
        Some(h) => h.across_const::<StringHeader, _>(|| path_arg_header(b)),
        None => (path_arg_header(b), a_ptr0),
    };
    f(a_ptr, b_ptr)
}

macro_rules! two_operand_value_entry {
    ($(#[$meta:meta])* $value_fn:ident => $ptr_fn:path, $ret:ty, $keep:ident) => {
        $(#[$meta])*
        #[no_mangle]
        pub extern "C" fn $value_fn(a: f64, b: f64) -> $ret {
            with_two_headers(a, b, |x, y| $ptr_fn(x, y))
        }

        /// Keepalive anchor — see [`KEEP_PATH_ARG_HEADER`].
        #[cfg(feature = "keepalive-anchors")]
        #[used]
        static $keep: extern "C" fn(f64, f64) -> $ret = $value_fn;
    };
}

two_operand_value_entry!(
    /// `path.join(a, b)` over NaN-boxed operands — see the module docs.
    js_path_join_value => super::js_path_join,
    *mut StringHeader,
    KEEP_PATH_JOIN_VALUE
);
two_operand_value_entry!(
    /// `path.win32.join(a, b)` over NaN-boxed operands.
    js_path_win32_join_value => super::js_path_win32_join,
    *mut StringHeader,
    KEEP_PATH_WIN32_JOIN_VALUE
);
two_operand_value_entry!(
    /// The `path.resolve` fold step over NaN-boxed operands — the issue's
    /// headline repro, `path.resolve(base, computedShortString)`.
    js_path_resolve_join_value => super::js_path_resolve_join,
    *mut StringHeader,
    KEEP_PATH_RESOLVE_JOIN_VALUE
);
two_operand_value_entry!(
    /// `path.win32.resolve(a, b)` fold step over NaN-boxed operands.
    js_path_win32_resolve_join_value => super::js_path_win32_resolve_join,
    *mut StringHeader,
    KEEP_PATH_WIN32_RESOLVE_JOIN_VALUE
);
two_operand_value_entry!(
    /// `path.basename(path, ext)` over NaN-boxed operands.
    js_path_basename_ext_value => super::js_path_basename_ext,
    *mut StringHeader,
    KEEP_PATH_BASENAME_EXT_VALUE
);
two_operand_value_entry!(
    /// `path.win32.basename(path, ext)` over NaN-boxed operands.
    js_path_win32_basename_ext_value => super::js_path_win32_basename_ext,
    *mut StringHeader,
    KEEP_PATH_WIN32_BASENAME_EXT_VALUE
);
two_operand_value_entry!(
    /// `path.matchesGlob(path, pattern)` over NaN-boxed operands.
    js_path_matches_glob_value => super::js_path_matches_glob,
    i32,
    KEEP_PATH_MATCHES_GLOB_VALUE
);
two_operand_value_entry!(
    /// `path.win32.matchesGlob(path, pattern)` over NaN-boxed operands.
    js_path_win32_matches_glob_value => super::js_path_win32_matches_glob,
    i32,
    KEEP_PATH_WIN32_MATCHES_GLOB_VALUE
);

#[cfg(test)]
mod tests {
    use super::*;

    fn heap_string(s: &str) -> f64 {
        let ptr = crate::string::js_string_from_bytes(s.as_ptr(), s.len() as u32);
        f64::from_bits(JSValue::string_ptr(ptr).bits())
    }

    fn sso_string(s: &str) -> f64 {
        let v = JSValue::try_short_string(s.as_bytes()).expect("fits SHORT_STRING_MAX_LEN");
        assert!(v.is_short_string(), "probe must take the inline form");
        f64::from_bits(v.bits())
    }

    fn read(ptr: *mut StringHeader) -> String {
        unsafe { super::super::string_from_header(ptr as *const StringHeader) }
            .expect("entry point must return a real StringHeader")
    }

    fn read_native_path(ptr: *mut StringHeader) -> String {
        read(ptr).replace('\\', "/")
    }

    /// The bug: an inline operand's CHARACTERS were dereferenced as a header.
    #[test]
    fn sso_operand_resolves_to_a_real_header() {
        let inline = sso_string("s1");
        let ptr = path_arg_header(inline);
        assert_ne!(
            ptr as usize,
            (inline.to_bits() & POINTER_MASK) as usize,
            "an SSO operand must NOT resolve to its own inline payload bits"
        );
        assert_eq!(read(ptr as *mut StringHeader), "s1");
    }

    /// ...while a heap operand keeps the old mask, bit for bit.
    #[test]
    fn heap_operand_keeps_the_old_mask() {
        let v = heap_string("segment-that-is-longer-than-sso");
        assert_eq!(
            path_arg_header(v) as usize,
            (v.to_bits() & POINTER_MASK) as usize
        );
    }

    /// ...and so does every non-string, so each entry point keeps its own throw
    /// / default behaviour unchanged.
    #[test]
    fn non_string_operands_keep_the_old_mask() {
        for v in [5.0f64, 0.0, f64::from_bits(crate::value::TAG_UNDEFINED)] {
            assert_eq!(
                path_arg_header(v) as usize,
                (v.to_bits() & POINTER_MASK) as usize,
                "non-string operand {v:?} must reproduce the pre-#7621 mask"
            );
        }
    }

    /// The two-operand window: materialising `a` allocates, and `b` must still
    /// be read correctly afterwards — in both SSO/heap orders.
    #[test]
    fn both_operands_survive_the_materialisation_window() {
        assert_eq!(
            read_native_path(js_path_join_value(sso_string("/r"), sso_string("s1"))),
            "/r/s1"
        );
        assert_eq!(
            read_native_path(js_path_join_value(
                heap_string("/root-that-is-long"),
                sso_string("s1")
            )),
            "/root-that-is-long/s1"
        );
        assert_eq!(
            read_native_path(js_path_join_value(
                sso_string("/r"),
                heap_string("segment-longer-than-sso")
            )),
            "/r/segment-longer-than-sso"
        );
        assert_eq!(
            read_native_path(js_path_resolve_join_value(
                heap_string("/root"),
                sso_string("s1")
            )),
            "/root/s1"
        );
        assert_eq!(
            read_native_path(js_path_resolve_join_value(
                sso_string("/a"),
                sso_string("/b")
            )),
            "/b",
            "an absolute later segment still resets the resolve"
        );
        assert_eq!(
            js_path_matches_glob_value(sso_string("a.ts"), heap_string("*.ts")),
            1
        );
        assert_eq!(
            js_path_matches_glob_value(sso_string("a.js"), heap_string("*.ts")),
            0
        );
        assert_eq!(
            read(js_path_basename_ext_value(
                sso_string("a.ts"),
                sso_string(".ts")
            )),
            "a"
        );
    }
}
