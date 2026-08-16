//! `%TypedArray%.prototype` method thunks with `this` brand checks.
//!
//! Every `%TypedArray%.prototype` method begins (spec step 1) with
//! `ValidateTypedArray(this)` — it must throw a `TypeError` when invoked on a
//! receiver that is not a TypedArray. The receiver-typed fast path
//! (`new Int8Array([…]).map(…)`) is lowered straight to the element-typed
//! `js_typed_array_*` helpers by codegen and never touches these thunks. They
//! fire only on the *value* path:
//!
//!   const m = Int8Array.prototype.map;
//!   m.call(plainArray, fn);   // must throw — plainArray is NOT a TypedArray
//!
//! Pre-fix the per-kind prototypes installed the shared
//! `global_this_builtin_noop_thunk` for these methods. A `.call`/`.apply` on
//! that no-op routed through `try_dispatch_value_called_proto_method`, which
//! re-dispatched by *method name* against the new receiver — landing on the
//! regular Array helper, which (after the array-like-receiver change) silently
//! accepted a plain array and returned a wrong-but-non-throwing result.
//!
//! These thunks instead read the `IMPLICIT_THIS` receiver (set by the
//! `.call`/`.apply` dispatch), brand-check it via `lookup_typed_array_kind`,
//! throw a `TypeError` on a non-TypedArray receiver, and otherwise delegate to
//! the existing `dispatch_typed_array_method` tower (the same code the fast
//! path's runtime sibling uses) — so reflective TypedArray-prototype calls now
//! also *work* on a real TypedArray receiver.
//!
//! Installed onto each per-kind typed-array `.prototype` by
//! `global_this::populate_builtin_prototype_methods`.

use super::*;

enum TypedArrayProtoReceiver {
    TypedArray(*mut crate::typedarray::TypedArrayHeader),
    Uint8Buffer(usize),
}

/// The TypedArray prototype methods whose receiver must be brand-checked. The
/// `u32` is the spec `.length` (own-property arity), matching what Node reports
/// for `Int8Array.prototype.<m>.length`. Iterator/data methods that don't take
/// a callback are included too — they all share the same brand requirement.
///
/// Mutators (`set`/`fill`/`copyWithin`/`sort`) are intentionally included: the
/// brand check is the only behavioral change for them, and on a real TypedArray
/// receiver they still reach the existing mutator impls via the dispatch tower.
pub(super) const TYPED_ARRAY_PROTO_METHODS: &[(&str, u32)] = &[
    ("at", 1),
    ("copyWithin", 2),
    ("entries", 0),
    ("every", 1),
    ("fill", 1),
    ("filter", 1),
    ("find", 1),
    ("findIndex", 1),
    ("findLast", 1),
    ("findLastIndex", 1),
    ("forEach", 1),
    ("includes", 1),
    ("indexOf", 1),
    ("join", 1),
    ("keys", 0),
    ("lastIndexOf", 1),
    ("map", 1),
    ("reduce", 1),
    ("reduceRight", 1),
    ("reverse", 0),
    ("set", 1),
    ("slice", 2),
    ("some", 1),
    ("sort", 1),
    ("subarray", 2),
    ("toLocaleString", 0),
    ("toReversed", 0),
    ("toSorted", 1),
    ("values", 0),
    ("with", 2),
];

/// Install the brand-checking `%TypedArray%.prototype` methods onto a per-kind
/// typed-array prototype object. Each method gets a DISTINCT thunk func_ptr so
/// the per-func-ptr arity registry (and the no-op-thunk filter in
/// `try_dispatch_value_called_proto_method`) can tell them apart — and, because
/// these are not the shared no-op thunk, a `.call`/`.apply` on the value flows
/// through the normal closure-dispatch path straight into the thunk, where the
/// brand check runs.
pub(super) fn install_typed_array_proto_methods(proto_obj: *mut ObjectHeader) {
    use super::global_this::install_proto_method as ipm;
    for &(name, arity) in TYPED_ARRAY_PROTO_METHODS {
        let func_ptr = thunk_for(name);
        ipm(proto_obj, name, func_ptr, arity);
        // `install_proto_method` uses the visible spec `.length` as the call
        // arity. Most of these thunks have a uniform 3-argument native
        // signature; register that ABI width so omitted trailing arguments are
        // padded with `undefined` instead of reading unset register slots.
        //
        // `lastIndexOf` and the reducers need to distinguish omitted optional
        // arguments from an explicitly supplied `undefined`, so they use the
        // rest-dispatch path with one fixed argument.
        if matches!(name, "lastIndexOf" | "reduce" | "reduceRight") {
            crate::closure::js_register_closure_rest(func_ptr, 1);
        } else {
            crate::closure::js_register_closure_arity(func_ptr, 3);
        }
    }
}

/// Map a method name to its dedicated brand-checking thunk. Unknown names fall
/// back to a generic dispatcher keyed off the closure's recorded `.name` — but
/// every entry in `TYPED_ARRAY_PROTO_METHODS` has a concrete thunk so the lookup
/// is exhaustive in practice.
fn thunk_for(name: &str) -> *const u8 {
    match name {
        "at" => ta_at_thunk as *const u8,
        "copyWithin" => ta_copy_within_thunk as *const u8,
        "entries" => ta_entries_thunk as *const u8,
        "every" => ta_every_thunk as *const u8,
        "fill" => ta_fill_thunk as *const u8,
        "filter" => ta_filter_thunk as *const u8,
        "find" => ta_find_thunk as *const u8,
        "findIndex" => ta_find_index_thunk as *const u8,
        "findLast" => ta_find_last_thunk as *const u8,
        "findLastIndex" => ta_find_last_index_thunk as *const u8,
        "forEach" => ta_for_each_thunk as *const u8,
        "includes" => ta_includes_thunk as *const u8,
        "indexOf" => ta_index_of_thunk as *const u8,
        "join" => ta_join_thunk as *const u8,
        "keys" => ta_keys_thunk as *const u8,
        "lastIndexOf" => ta_last_index_of_thunk as *const u8,
        "map" => ta_map_thunk as *const u8,
        "reduce" => ta_reduce_thunk as *const u8,
        "reduceRight" => ta_reduce_right_thunk as *const u8,
        "reverse" => ta_reverse_thunk as *const u8,
        "set" => ta_set_thunk as *const u8,
        "slice" => ta_slice_thunk as *const u8,
        "some" => ta_some_thunk as *const u8,
        "sort" => ta_sort_thunk as *const u8,
        "subarray" => ta_subarray_thunk as *const u8,
        "toLocaleString" => ta_to_locale_string_thunk as *const u8,
        "toReversed" => ta_to_reversed_thunk as *const u8,
        "toSorted" => ta_to_sorted_thunk as *const u8,
        "values" => ta_values_thunk as *const u8,
        "with" => ta_with_thunk as *const u8,
        _ => ta_generic_thunk as *const u8,
    }
}

/// Throw `TypeError: <fn> called on incompatible receiver` (Test262's brand
/// checks assert only the error *type*; the wording is informational). Never
/// returns.
fn throw_not_typed_array(method: &str) -> ! {
    let msg = format!("Method %TypedArray%.prototype.{method} called on incompatible receiver");
    let s = crate::string::js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
    let err = crate::error::js_typeerror_new(s);
    crate::exception::js_throw(f64::from_bits(
        crate::value::JSValue::pointer(err as *const u8).bits(),
    ))
}

/// Read the `IMPLICIT_THIS` receiver and brand-check it as a real TypedArray.
/// Returns the cleaned receiver, or throws a `TypeError`.
#[inline]
unsafe fn ta_receiver_or_throw(method: &str) -> TypedArrayProtoReceiver {
    let bits = IMPLICIT_THIS.with(|c| c.get());
    // A TypedArray receiver reaches here in either of two boxings: a NaN-boxed
    // `POINTER_TAG` value (top16 >= 0x7FF8) or a *raw* heap pointer whose top16
    // is 0 (the receiver-typed fast path threads the bare pointer — see the
    // raw-pointer arm in `native_call_method`). Resolve both to a clean address
    // and brand-check it against the typed-array registry.
    let top16 = bits >> 48;
    let addr = if top16 >= 0x7FF8 {
        (bits & crate::value::POINTER_MASK) as usize
    } else if top16 == 0 && bits >= 0x10000 {
        bits as usize
    } else {
        throw_not_typed_array(method)
    };
    if crate::typedarray::lookup_typed_array_kind(addr).is_some() {
        return TypedArrayProtoReceiver::TypedArray(
            addr as *mut crate::typedarray::TypedArrayHeader,
        );
    }
    if is_typed_array_buffer(addr) {
        return TypedArrayProtoReceiver::Uint8Buffer(addr);
    }
    throw_not_typed_array(method)
}

#[cfg(test)]
thread_local! {
/// Every entry into [`is_typed_array_buffer`], i.e. every caller that could not
/// rule a Buffer receiver out more cheaply. Mirrors #8140's
/// `TEST_TA_REGISTRY_PROBES` and exists for the same reason: the
/// `arena_payload_has_gc_type` gate that lets an ordinary array skip this probe
/// is asserted against it, so deleting the gate fails a test even though the
/// ANSWER stays correct. A fast path nobody can prove ran is not a fast path.
///
/// Per THREAD, not per process: `cargo test` runs each case on its own thread.
    static TEST_BUFFER_GATE_PROBES: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
pub(crate) fn test_buffer_gate_probe_count() -> u64 {
    TEST_BUFFER_GATE_PROBES.with(|c| c.get())
}

pub(crate) fn is_typed_array_buffer(addr: usize) -> bool {
    #[cfg(test)]
    TEST_BUFFER_GATE_PROBES.with(|c| c.set(c.get().wrapping_add(1)));
    crate::buffer::is_registered_buffer(addr)
        && !crate::buffer::is_any_array_buffer(addr)
        && !crate::buffer::is_data_view(addr)
        && !crate::buffer::is_secret_key(addr)
        && crate::buffer::asymmetric_key_meta(addr).is_none()
        && crate::buffer::crypto_key_meta(addr).is_none()
}

#[inline]
fn undefined() -> f64 {
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

#[inline]
fn pointer_value(addr: usize) -> f64 {
    f64::from_bits(crate::value::JSValue::pointer(addr as *mut u8).bits())
}

#[inline]
fn arg_or_undefined(args: &[f64], index: usize) -> f64 {
    args.get(index).copied().unwrap_or_else(undefined)
}

#[inline]
fn is_undefined_arg(value: f64) -> bool {
    crate::value::JSValue::from_bits(value.to_bits()).is_undefined()
}

#[inline]
fn to_integer_or_infinity(value: f64) -> f64 {
    let number = crate::value::JSValue::from_bits(value.to_bits()).to_number();
    if number.is_nan() || number == 0.0 {
        0.0
    } else if !number.is_finite() {
        number
    } else {
        number.trunc()
    }
}

#[inline]
fn uint8_relative_index_arg(args: &[f64], index: usize, len: usize, default: usize) -> usize {
    let Some(value) = args.get(index).copied() else {
        return default;
    };
    if is_undefined_arg(value) {
        return default;
    }
    let n = to_integer_or_infinity(value);
    if n == f64::INFINITY {
        return len;
    }
    if n == f64::NEG_INFINITY {
        return 0;
    }
    let n = n as i64;
    if n < 0 {
        (len as i64 + n).max(0) as usize
    } else {
        (n as usize).min(len)
    }
}

#[inline]
fn to_uint8(value: f64) -> u8 {
    let number = crate::value::JSValue::from_bits(value.to_bits()).to_number();
    if !number.is_finite() || number == 0.0 {
        0
    } else {
        (number.trunc() as i64).rem_euclid(256) as u8
    }
}

#[inline]
unsafe fn uint8_len(addr: usize) -> usize {
    (*(addr as *const crate::buffer::BufferHeader)).length as usize
}

#[inline]
unsafe fn uint8_get(addr: usize, index: usize) -> u8 {
    crate::buffer::js_buffer_get(addr as *const crate::buffer::BufferHeader, index as i32) as u8
}

#[inline]
unsafe fn uint8_set(addr: usize, index: usize, value: u8) {
    crate::buffer::js_buffer_set(
        addr as *mut crate::buffer::BufferHeader,
        index as i32,
        value as i32,
    );
}

unsafe fn uint8_alloc_like(source_addr: usize, len: usize) -> *mut crate::buffer::BufferHeader {
    let out = crate::buffer::buffer_alloc(len as u32);
    if !out.is_null() {
        (*out).length = len as u32;
        if crate::buffer::is_uint8array_buffer(source_addr) {
            crate::buffer::mark_as_uint8array(out as usize);
        }
    }
    out
}

unsafe fn uint8_copy_to_new(source_addr: usize) -> *mut crate::buffer::BufferHeader {
    let len = uint8_len(source_addr);
    let out = uint8_alloc_like(source_addr, len);
    for i in 0..len {
        uint8_set(out as usize, i, uint8_get(source_addr, i));
    }
    out
}

#[inline]
fn bool_value(value: bool) -> f64 {
    f64::from_bits(crate::value::JSValue::bool(value).bits())
}

#[inline]
fn string_value(value: *mut crate::string::StringHeader) -> f64 {
    f64::from_bits(crate::value::JSValue::string_ptr(value).bits())
}

unsafe fn uint8_materialize_array(addr: usize) -> *mut crate::array::ArrayHeader {
    crate::buffer::buffer_to_array(addr as *const crate::buffer::BufferHeader)
}

unsafe fn uint8_iterator(addr: usize, method: &str) -> f64 {
    let arr = uint8_materialize_array(addr);
    let iter = match method {
        "keys" => crate::array::js_array_keys_iter_obj(arr as *const crate::array::ArrayHeader),
        "entries" => {
            crate::array::js_array_entries_iter_obj(arr as *const crate::array::ArrayHeader)
        }
        _ => crate::array::js_array_values_iter_obj(arr as *const crate::array::ArrayHeader),
    };
    pointer_value(iter as usize)
}

fn uint8_at_index(args: &[f64], len: usize) -> Option<usize> {
    let index = to_integer_or_infinity(arg_or_undefined(args, 0));
    if !index.is_finite() {
        return None;
    }
    let index = if index < 0.0 {
        len as i64 + index as i64
    } else {
        index as i64
    };
    (0..len as i64).contains(&index).then_some(index as usize)
}

fn uint8_search_needle(value: f64) -> Option<u8> {
    let js_value = crate::value::JSValue::from_bits(value.to_bits());
    if !js_value.is_number() && !js_value.is_int32() {
        return None;
    }
    let number = js_value.to_number();
    if number == 0.0 {
        return Some(0);
    }
    if !number.is_finite() || number.fract() != 0.0 || !(0.0..=255.0).contains(&number) {
        return None;
    }
    Some(number as u8)
}

fn uint8_from_index(args: &[f64], len: usize) -> usize {
    let Some(value) = args.get(1).copied() else {
        return 0;
    };
    if is_undefined_arg(value) {
        return 0;
    }
    let n = to_integer_or_infinity(value);
    if n == f64::INFINITY {
        return len;
    }
    if n == f64::NEG_INFINITY {
        return 0;
    }
    let n = n as i64;
    if n < 0 {
        (len as i64 + n).max(0) as usize
    } else {
        (n as usize).min(len)
    }
}

fn uint8_last_from_index(args: &[f64], len: usize) -> Option<usize> {
    if len == 0 {
        return None;
    }
    let Some(value) = args.get(1).copied() else {
        return Some(len - 1);
    };
    let js_value = crate::value::JSValue::from_bits(value.to_bits());
    if is_undefined_arg(value) || (js_value.is_number() && js_value.to_number().is_nan()) {
        return Some(len - 1);
    }
    let n = to_integer_or_infinity(value);
    if n == f64::NEG_INFINITY {
        return None;
    }
    if n == f64::INFINITY {
        return Some(len - 1);
    }
    let n = n as i64;
    let index = if n < 0 {
        len as i64 + n
    } else {
        n.min(len as i64 - 1)
    };
    (index >= 0).then_some(index as usize)
}

unsafe fn uint8_index_of(addr: usize, args: &[f64]) -> f64 {
    let len = uint8_len(addr);
    let needle = match args.first().copied().and_then(uint8_search_needle) {
        Some(needle) => needle,
        None => return -1.0,
    };
    for i in uint8_from_index(args, len)..len {
        if uint8_get(addr, i) == needle {
            return i as f64;
        }
    }
    -1.0
}

unsafe fn uint8_last_index_of(addr: usize, args: &[f64]) -> f64 {
    let needle = match args.first().copied().and_then(uint8_search_needle) {
        Some(needle) => needle,
        None => return -1.0,
    };
    let Some(start) = uint8_last_from_index(args, uint8_len(addr)) else {
        return -1.0;
    };
    for i in (0..=start).rev() {
        if uint8_get(addr, i) == needle {
            return i as f64;
        }
    }
    -1.0
}

unsafe fn uint8_includes(addr: usize, args: &[f64]) -> f64 {
    let len = uint8_len(addr);
    let needle = match args.first().copied().and_then(uint8_search_needle) {
        Some(needle) => needle,
        None => return bool_value(false),
    };
    for i in uint8_from_index(args, len)..len {
        if uint8_get(addr, i) == needle {
            return bool_value(true);
        }
    }
    bool_value(false)
}

unsafe fn uint8_join(addr: usize, separator_value: f64) -> f64 {
    let separator = if is_undefined_arg(separator_value) {
        ",".to_string()
    } else {
        let separator = crate::value::js_jsvalue_to_string(separator_value);
        crate::string::string_as_str(separator as *const crate::string::StringHeader).to_string()
    };
    let len = uint8_len(addr);
    let mut result = String::new();
    for i in 0..len {
        if i > 0 {
            result.push_str(&separator);
        }
        result.push_str(&uint8_get(addr, i).to_string());
    }
    let out = crate::string::js_string_from_bytes(result.as_ptr(), result.len() as u32);
    std::hint::black_box(&result);
    string_value(out)
}

unsafe fn uint8_with(addr: usize, args: &[f64]) -> f64 {
    let len = uint8_len(addr);
    let Some(index) = uint8_at_index(args, len) else {
        crate::typedarray::throw_range_error(b"Invalid typed array index");
    };
    let value = to_uint8(arg_or_undefined(args, 1));
    let out = crate::buffer::js_uint8array_alloc(len as i32);
    for i in 0..len {
        let stored = if i == index {
            value
        } else {
            uint8_get(addr, i)
        };
        uint8_set(out as usize, i, stored);
    }
    pointer_value(out as usize)
}

unsafe fn rest_values(rest_value: f64) -> Vec<f64> {
    let rest = crate::value::JSValue::from_bits(rest_value.to_bits());
    if !rest.is_pointer() {
        return Vec::new();
    }
    let arr = rest.as_pointer::<crate::array::ArrayHeader>();
    if arr.is_null() {
        return Vec::new();
    }
    let len = crate::array::js_array_length(arr) as usize;
    let mut values = Vec::with_capacity(len);
    for i in 0..len {
        values.push(crate::array::js_array_get_f64(arr, i as u32));
    }
    values
}

unsafe fn first_arg_with_rest(first: f64, rest: f64) -> Vec<f64> {
    let mut args = Vec::with_capacity(2);
    args.push(first);
    args.extend(rest_values(rest));
    args
}

fn validate_callback(args: &[f64]) -> *const crate::closure::ClosureHeader {
    crate::array::js_validate_array_callback(arg_or_undefined(args, 0))
        as *const crate::closure::ClosureHeader
}

fn validate_comparator(args: &[f64]) -> *const crate::closure::ClosureHeader {
    if args.is_empty() {
        std::ptr::null()
    } else {
        crate::array::js_validate_array_comparator(args[0]) as *const crate::closure::ClosureHeader
    }
}

/// A Buffer-backed `Uint8Array` receiver (or `map`/`toSorted` result) kept
/// LIVE for the duration of a user-callback loop.
///
/// `dispatch_uint8_buffer_method` runs user code with the receiver reachable
/// only through a raw `usize` parameter and the native stack, which an
/// evacuating minor does not scan. `array::buffer_receiver_dispatch` roots it
/// at the boundary; the `%TypedArray%.prototype` thunk and
/// `dispatch_buffer_method`'s catch-all — the other two entries — do not, so
/// the root belongs here, where all three get it.
///
/// # Why the ADDRESS is read once per dispatch, not once per element
///
/// A raw address held across a call is normally a stale copy
/// (`docs/src/internals/gc-rooting-invariant.md`). A Buffer is the documented
/// exception: `buffer_alloc` allocates it with `arena_alloc_gc_old` and sets
/// `GC_FLAG_TENURED` (`buffer/header.rs`) — the same old-arena space
/// `typed_array_alloc` describes as "non-movable space: raw data pointers are
/// handed out". Every `%TypedArray%` sibling in `typedarray/iterate.rs` and
/// `typedarray/transform.rs` relies on exactly that invariant and holds its
/// `ta` / `recv` in a plain local across callbacks; doing something different
/// here would be inconsistent without being safer.
///
/// The one collector arm that relocates an old-arena page is old-page defrag,
/// and it is opt-in (`PERRY_GC_OLD_DEFRAG=1`, `gc/oldgen_defrag.rs`,
/// deliberately default-off until a fragmentation workload exists that can
/// exercise it). Making that arm safe is a TREE-WIDE property of every holder
/// of an old-arena raw address, not something one dispatcher can establish —
/// so this type buys liveness plus a single post-validation read, and the
/// invariant is named rather than re-checked 2.6M times per `forEach`.
/// Re-reading it per element measured **+19 %** on a `Uint8Array`
/// forEach/map/reduce benchmark, for a knob that is off.
///
/// The callback closure below is the opposite case and IS re-read per element:
/// it genuinely moves.
struct RootedUint8Buffer<'s> {
    handle: crate::gc::RuntimeHandle<'s>,
}

impl<'s> RootedUint8Buffer<'s> {
    #[inline]
    fn new(scope: &'s crate::gc::RuntimeHandleScope, addr: usize) -> Self {
        Self {
            handle: scope.root_nanbox_f64(pointer_value(addr)),
        }
    }

    /// The address and the receiver value from ONE handle read.
    ///
    /// Taken after `validate_callback` (which allocates on its throw path) and
    /// before the loop, so it is a post-allocation read. On Darwin a
    /// `perry_thread_local!` access is a `tlv_get_addr` CALL, which is why
    /// this is one read and not two.
    #[inline]
    fn live(&self) -> (usize, f64) {
        let value = self.handle.get_nanbox_f64();
        (
            (value.to_bits() & crate::value::POINTER_MASK) as usize,
            value,
        )
    }
}

/// A callback closure rooted for the duration of a loop that invokes it once
/// per element, with its address RE-READ from the root before every call.
///
/// **This is the live half of #8179.** A closure is an ordinary nursery
/// allocation (`GC_TYPE_CLOSURE`, `GcAllocationPolicy::ArenaOrMalloc`, with a
/// `GcMoveHookKind::ClosureDynamicProps` move hook — it both moves and dies),
/// and a callback handed in by a frameless caller — the arrow in
/// `Uint8Array.prototype.map.call(u, v => v + 1)` — is reachable ONLY through
/// a raw parameter plus the native stack, which an evacuating minor does not
/// scan. `js_array_map` has carried the same root since gh #6206 / #6081; this
/// dispatcher had it on one of its three entries.
///
/// `test-files/test_gap_gc_uint8_buffer_callback_rooting.ts` reproduces it on
/// the SHIPPED DEFAULT (`TypeError: value is not a function`, exit 1) and,
/// under `PERRY_GC_SCHEDULE_RATE=1 PERRY_GC_SCHEDULE_SEED=7
/// PERRY_GC_PROTECT_FROMSPACE=1`, faults precisely — on the FIRST scheduled
/// collection:
///
/// ```text
/// [gc-fromspace-protect] FAULT: signal 10 at 0x2454161058c
///   last-known object: user_ptr=0x24541610580 obj_type=4 size=24
/// [gc-schedule] FAILURE (signal 10) under seed=7
/// [gc-schedule]   safepoints=1 scheduled_collections=1
/// ```
///
/// `obj_type=4` is `GC_TYPE_CLOSURE`, and the faulting address is
/// `user_ptr + 12` — `CLOSURE_TYPE_TAG_OFFSET`, i.e. `get_valid_func_ptr`'s
/// `CLOSURE_MAGIC` probe reading a retired from-space closure header. After
/// the fix the same seed runs to completion with the instrument's own liveness
/// verdict showing the subject was live: `safepoints=5306
/// scheduled_collections=5306 copying_minors=5306 moved_objects=25760`.

/// #8180: each type also carries the closure's dispatch strategy, resolved
/// ONCE for the whole loop (`closure/dispatch/direct.rs`) instead of
/// re-derived per element by `js_closure_callN`.
macro_rules! rooted_callback {
    ($name:ident, $site:ident, $($arg:ident),+) => {
        struct $name<'s> {
            handle: crate::gc::RuntimeHandle<'s>,
            site: crate::closure::$site,
        }

        impl<'s> $name<'s> {
            #[inline]
            fn new(
                scope: &'s crate::gc::RuntimeHandleScope,
                callback: *const crate::closure::ClosureHeader,
            ) -> Self {
                Self {
                    handle: scope.root_raw_const_ptr(callback),
                    site: crate::closure::$site::resolve(callback),
                }
            }

            /// `with_const_ptr`, not a bare `get_raw_const_ptr`: the pointer is
            /// an ARGUMENT to a C-ABI call, a position `across_*` cannot
            /// express (it hands the address back only AFTER the call returns).
            ///
            /// The pre-call address is never bound across a call either way —
            /// the handle is re-read on EVERY iteration, immediately before the
            /// pointer is consumed, and the root is a `RawTagged` slot that
            /// `scan_runtime_handle_roots_mut` both marks and REWRITES. So a
            /// collection during call `i` is reflected in the address call
            /// `i + 1` uses, which is precisely the ordering `across_*` exists
            /// to enforce. The resolved `site` needs no such treatment: it is a
            /// static CODE address, which relocation does not change.
            #[inline]
            fn call(&self, $($arg: f64),+) -> f64 {
                self.handle
                    .with_const_ptr::<crate::closure::ClosureHeader, _>(|cb| {
                        self.site.call(cb, $($arg),+)
                    })
            }
        }
    };
}

rooted_callback!(RootedCallback2, DirectCall2, arg0, arg1);
rooted_callback!(RootedCallback3, DirectCall3, arg0, arg1, arg2);
rooted_callback!(RootedCallback4, DirectCall4, arg0, arg1, arg2, arg3);

pub(crate) unsafe fn dispatch_uint8_buffer_method(
    addr: usize,
    method: &str,
    args: &[f64],
) -> Option<f64> {
    let len = uint8_len(addr);
    // Root the receiver for the whole dispatch. Its ADDRESS is then taken from
    // the root once per arm — after the callback validation, before the loop —
    // rather than being carried in from the parameter; see `RootedUint8Buffer`
    // for why once-per-arm and not once-per-element.
    let scope = crate::gc::RuntimeHandleScope::new();
    let recv = RootedUint8Buffer::new(&scope, addr);
    // Shadow the raw parameter: nothing below may reach the receiver except
    // through the rooted handle.
    #[allow(unused_variables, clippy::let_unit_value)]
    let addr = ();

    let result = match method {
        "set" => {
            let mut args_ptr = std::ptr::null();
            if !args.is_empty() {
                args_ptr = args.as_ptr();
            }
            let (addr, _) = recv.live();
            super::dispatch_buffer_method(addr, method, args_ptr, args.len())
        }
        "at" => match uint8_at_index(args, len) {
            Some(index) => uint8_get(recv.live().0, index) as f64,
            None => undefined(),
        },
        "entries" | "keys" | "values" => uint8_iterator(recv.live().0, method),
        "slice" | "subarray" => {
            let start = uint8_relative_index_arg(args, 0, len, 0);
            let end = uint8_relative_index_arg(args, 1, len, len);
            let (addr, _) = recv.live();
            let result = crate::buffer::js_buffer_slice(
                addr as *mut crate::buffer::BufferHeader,
                start as i32,
                end as i32,
            );
            if crate::buffer::is_uint8array_buffer(addr) {
                crate::buffer::mark_as_uint8array(result as usize);
            }
            pointer_value(result as usize)
        }
        "copyWithin" => {
            let to = uint8_relative_index_arg(args, 0, len, 0);
            let from = uint8_relative_index_arg(args, 1, len, 0);
            let final_ = uint8_relative_index_arg(args, 2, len, len);
            let count = final_.saturating_sub(from).min(len.saturating_sub(to));
            let (addr, receiver) = recv.live();
            if count > 0 {
                let block: Vec<u8> = (0..count).map(|i| uint8_get(addr, from + i)).collect();
                for (i, value) in block.into_iter().enumerate() {
                    uint8_set(addr, to + i, value);
                }
            }
            receiver
        }
        "fill" => {
            // `to_uint8` runs ToNumber, which fires a user `valueOf` on an
            // object argument — so it is a collection point, and it is
            // sequenced BEFORE the `recv.live()` read on purpose.
            let value = to_uint8(arg_or_undefined(args, 0));
            let start = uint8_relative_index_arg(args, 1, len, 0);
            let end = uint8_relative_index_arg(args, 2, len, len);
            let (addr, receiver) = recv.live();
            for i in start..end {
                uint8_set(addr, i, value);
            }
            receiver
        }
        "map" => {
            let cb = RootedCallback3::new(&scope, validate_callback(args));
            // `uint8_alloc_like` allocates, so the receiver read is
            // sequenced AFTER it.
            let out = uint8_alloc_like(recv.live().0, len) as usize;
            let (addr, receiver) = recv.live();
            for i in 0..len {
                let value = uint8_get(addr, i) as f64;
                let mapped = cb.call(value, i as f64, receiver);
                uint8_set(out, i, to_uint8(mapped));
            }
            pointer_value(out)
        }
        "filter" => {
            let cb = RootedCallback3::new(&scope, validate_callback(args));
            let (addr, receiver) = recv.live();
            let mut kept = Vec::new();
            for i in 0..len {
                let value = uint8_get(addr, i) as f64;
                let keep = cb.call(value, i as f64, receiver);
                if crate::value::js_is_truthy(keep) != 0 {
                    kept.push(value as u8);
                }
            }
            let out = uint8_alloc_like(recv.live().0, kept.len());
            for (i, value) in kept.into_iter().enumerate() {
                uint8_set(out as usize, i, value);
            }
            pointer_value(out as usize)
        }
        "every" => {
            let cb = RootedCallback3::new(&scope, validate_callback(args));
            let (addr, receiver) = recv.live();
            for i in 0..len {
                let value = uint8_get(addr, i) as f64;
                let keep = cb.call(value, i as f64, receiver);
                if crate::value::js_is_truthy(keep) == 0 {
                    return Some(bool_value(false));
                }
            }
            bool_value(true)
        }
        "some" => {
            let cb = RootedCallback3::new(&scope, validate_callback(args));
            let (addr, receiver) = recv.live();
            for i in 0..len {
                let value = uint8_get(addr, i) as f64;
                let keep = cb.call(value, i as f64, receiver);
                if crate::value::js_is_truthy(keep) != 0 {
                    return Some(bool_value(true));
                }
            }
            bool_value(false)
        }
        // #8137: `findLast` had NO arm, so it fell through to the catch-all
        // and threw `TypeError: (Buffer).findLast is not a function` — on the
        // STATIC receiver too (`new Uint8Array([3,1,2]).findLast(x => x < 3)`,
        // node: `2`). Its sibling `findLastIndex` was already served below,
        // which is why the hole survived: the two are always cited together.
        "find" | "findLast" => {
            let cb = RootedCallback3::new(&scope, validate_callback(args));
            let (addr, receiver) = recv.live();
            let indexes: Box<dyn Iterator<Item = usize>> = if method == "find" {
                Box::new(0..len)
            } else {
                Box::new((0..len).rev())
            };
            for i in indexes {
                let value = uint8_get(addr, i) as f64;
                let keep = cb.call(value, i as f64, receiver);
                if crate::value::js_is_truthy(keep) != 0 {
                    return Some(value);
                }
            }
            undefined()
        }
        "findIndex" | "findLastIndex" => {
            let cb = RootedCallback3::new(&scope, validate_callback(args));
            let (addr, receiver) = recv.live();
            let indexes: Box<dyn Iterator<Item = usize>> = if method == "findIndex" {
                Box::new(0..len)
            } else {
                Box::new((0..len).rev())
            };
            for i in indexes {
                let value = uint8_get(addr, i) as f64;
                let keep = cb.call(value, i as f64, receiver);
                if crate::value::js_is_truthy(keep) != 0 {
                    return Some(i as f64);
                }
            }
            -1.0
        }
        "forEach" => {
            let cb = RootedCallback3::new(&scope, validate_callback(args));
            let (addr, receiver) = recv.live();
            for i in 0..len {
                let value = uint8_get(addr, i) as f64;
                let _ = cb.call(value, i as f64, receiver);
            }
            undefined()
        }
        "includes" => uint8_includes(recv.live().0, args),
        "indexOf" => uint8_index_of(recv.live().0, args),
        "lastIndexOf" => uint8_last_index_of(recv.live().0, args),
        "join" => uint8_join(recv.live().0, arg_or_undefined(args, 0)),
        "toLocaleString" => uint8_join(recv.live().0, undefined()),
        "reduce" | "reduceRight" => {
            let cb = RootedCallback4::new(&scope, validate_callback(args));
            if len == 0 && args.len() < 2 {
                crate::array::throw_reduce_of_empty();
            }
            let reverse = method == "reduceRight";
            let (addr, receiver) = recv.live();
            let (accumulator, indexes): (f64, Box<dyn Iterator<Item = usize>>) = if args.len() >= 2
            {
                let iter: Box<dyn Iterator<Item = usize>> = if reverse {
                    Box::new((0..len).rev())
                } else {
                    Box::new(0..len)
                };
                (args[1], iter)
            } else if reverse {
                (
                    uint8_get(addr, len - 1) as f64,
                    Box::new((0..len - 1).rev()),
                )
            } else {
                (uint8_get(addr, 0) as f64, Box::new(1..len))
            };
            // The accumulator is the one value in this dispatcher that is
            // routinely a NURSERY object: a string/object/array seed, or a
            // fresh one returned by every callback. Held in a Rust local it is
            // a pre-move NaN-boxed address that an evacuating minor rewrites
            // nowhere. `js_array_reduce` (array/iter_methods.rs) has rooted its
            // accumulator since the 2026-07-02 audit; the Buffer-receiver
            // reduce and both TypedArray reduces never did.
            let acc = scope.root_nanbox_f64(accumulator);
            for i in indexes {
                let value = uint8_get(addr, i) as f64;
                let next = cb.call(acc.get_nanbox_f64(), value, i as f64, receiver);
                acc.set_nanbox_f64(next);
            }
            acc.get_nanbox_f64()
        }
        "reverse" => {
            let (addr, receiver) = recv.live();
            if len > 1 {
                let mut i = 0usize;
                let mut j = len - 1;
                while i < j {
                    let left = uint8_get(addr, i);
                    let right = uint8_get(addr, j);
                    uint8_set(addr, i, right);
                    uint8_set(addr, j, left);
                    i += 1;
                    j -= 1;
                }
            }
            receiver
        }
        "sort" | "toSorted" => {
            let cmp = validate_comparator(args);
            let (addr, receiver) = recv.live();
            let out_addr = if method == "sort" {
                addr
            } else {
                uint8_copy_to_new(addr) as usize
            };
            let mut values: Vec<u8> = (0..len).map(|i| uint8_get(out_addr, i)).collect();
            if cmp.is_null() {
                values.sort_unstable();
            } else {
                let cmp = RootedCallback2::new(&scope, cmp);
                values.sort_by(|a, b| {
                    let r = cmp.call(*a as f64, *b as f64);
                    if r < 0.0 {
                        std::cmp::Ordering::Less
                    } else if r > 0.0 {
                        std::cmp::Ordering::Greater
                    } else {
                        std::cmp::Ordering::Equal
                    }
                });
            }
            for (i, value) in values.into_iter().enumerate() {
                uint8_set(out_addr, i, value);
            }
            if method == "sort" {
                receiver
            } else {
                pointer_value(out_addr)
            }
        }
        "toReversed" => {
            let out = uint8_alloc_like(recv.live().0, len);
            let (addr, _) = recv.live();
            for i in 0..len {
                uint8_set(out as usize, i, uint8_get(addr, len - 1 - i));
            }
            pointer_value(out as usize)
        }
        "with" => uint8_with(recv.live().0, args),
        _ => return None,
    };
    Some(result)
}

/// Brand-check, then delegate to the shared dispatch tower with the supplied
/// argument slice. `dispatch_typed_array_method` handles every method name in
/// `TYPED_ARRAY_PROTO_METHODS`; the `unwrap_or(undefined)` guard never fires in
/// practice (the name set is kept in sync) but avoids a panic on drift.
#[inline]
unsafe fn brand_then_dispatch(method: &str, args: &[f64]) -> f64 {
    let receiver = ta_receiver_or_throw(method);
    let args_ptr = if args.is_empty() {
        std::ptr::null()
    } else {
        args.as_ptr()
    };
    match receiver {
        TypedArrayProtoReceiver::TypedArray(ta) => {
            match super::native_call_method::dispatch_typed_array_method(
                ta,
                method,
                args_ptr,
                args.len(),
            ) {
                // Brand check passed and the tower handled the method.
                Some(r) => r,
                // The only `TYPED_ARRAY_PROTO_METHODS` entry the tower doesn't yet
                // resolve is `toLocaleString` (a separate formatting gap, out of scope
                // for this brand-check fix). The brand check already ran, so a
                // non-TypedArray receiver has thrown; a real receiver simply gets
                // `undefined` here rather than a wrong value.
                None => undefined(),
            }
        }
        TypedArrayProtoReceiver::Uint8Buffer(addr) => {
            dispatch_uint8_buffer_method(addr, method, args).unwrap_or_else(undefined)
        }
    }
}

// Every thunk takes a uniform `(closure, f64, f64, f64)` signature — the
// closure-dispatch path (`js_native_call_value`) transmutes the func_ptr to a
// per-arity signature using `max(registered_arity, supplied_args)`; a 3-arg
// signature safely covers all real TypedArray-method call shapes (the widest
// real methods — `copyWithin`/`fill` — take 3). Extra supplied args beyond the
// 3 we declare are dropped, which is fine: the brand check (the point of these
// thunks) runs before any argument is read, and no spec TypedArray method
// consumes more than 3 positional arguments. Each method slices off only the
// args it needs before delegating.

macro_rules! ta_thunk {
    ($name:ident, $method:literal, $argc:literal) => {
        pub(super) extern "C" fn $name(
            _c: *const crate::closure::ClosureHeader,
            a: f64,
            b: f64,
            d: f64,
        ) -> f64 {
            let all = [a, b, d];
            unsafe { brand_then_dispatch($method, &all[..$argc]) }
        }
    };
}

ta_thunk!(ta_at_thunk, "at", 1);
ta_thunk!(ta_copy_within_thunk, "copyWithin", 3);
ta_thunk!(ta_entries_thunk, "entries", 0);
ta_thunk!(ta_every_thunk, "every", 1);
ta_thunk!(ta_fill_thunk, "fill", 3);
ta_thunk!(ta_filter_thunk, "filter", 1);
ta_thunk!(ta_find_thunk, "find", 1);
ta_thunk!(ta_find_index_thunk, "findIndex", 1);
ta_thunk!(ta_find_last_thunk, "findLast", 1);
ta_thunk!(ta_find_last_index_thunk, "findLastIndex", 1);
ta_thunk!(ta_for_each_thunk, "forEach", 1);
ta_thunk!(ta_includes_thunk, "includes", 2);
ta_thunk!(ta_index_of_thunk, "indexOf", 2);
ta_thunk!(ta_join_thunk, "join", 1);
ta_thunk!(ta_keys_thunk, "keys", 0);
ta_thunk!(ta_map_thunk, "map", 1);
ta_thunk!(ta_reverse_thunk, "reverse", 0);
ta_thunk!(ta_set_thunk, "set", 2);
ta_thunk!(ta_slice_thunk, "slice", 2);
ta_thunk!(ta_some_thunk, "some", 1);
ta_thunk!(ta_sort_thunk, "sort", 1);
ta_thunk!(ta_subarray_thunk, "subarray", 2);
ta_thunk!(ta_to_locale_string_thunk, "toLocaleString", 0);
ta_thunk!(ta_to_reversed_thunk, "toReversed", 0);
ta_thunk!(ta_to_sorted_thunk, "toSorted", 1);
ta_thunk!(ta_values_thunk, "values", 0);
ta_thunk!(ta_with_thunk, "with", 2);

pub(super) extern "C" fn ta_last_index_of_thunk(
    _c: *const crate::closure::ClosureHeader,
    search_element: f64,
    rest: f64,
) -> f64 {
    unsafe {
        let args = first_arg_with_rest(search_element, rest);
        brand_then_dispatch("lastIndexOf", &args)
    }
}

pub(super) extern "C" fn ta_reduce_thunk(
    _c: *const crate::closure::ClosureHeader,
    callback: f64,
    rest: f64,
) -> f64 {
    unsafe {
        let args = first_arg_with_rest(callback, rest);
        brand_then_dispatch("reduce", &args)
    }
}

pub(super) extern "C" fn ta_reduce_right_thunk(
    _c: *const crate::closure::ClosureHeader,
    callback: f64,
    rest: f64,
) -> f64 {
    unsafe {
        let args = first_arg_with_rest(callback, rest);
        brand_then_dispatch("reduceRight", &args)
    }
}

/// Fallback thunk for any method name not given a dedicated thunk above.
/// Recovers the method name from the closure's recorded `.name`, brand-checks,
/// then dispatches. Never reached for the names in `TYPED_ARRAY_PROTO_METHODS`,
/// but keeps the install path total.
pub(super) extern "C" fn ta_generic_thunk(
    c: *const crate::closure::ClosureHeader,
    a: f64,
    b: f64,
    d: f64,
) -> f64 {
    unsafe {
        let name_val = crate::closure::closure_get_dynamic_prop(c as usize, "name");
        let name_hdr = crate::builtins::js_string_coerce(name_val);
        let name =
            super::has_own_helpers::str_from_string_header(name_hdr).unwrap_or("TypedArray method");
        let all = [a, b, d];
        brand_then_dispatch(name, &all)
    }
}

// ---------------------------------------------------------------------------
// #6674: `Uint8Array.prototype` base64/hex conversion methods.
//
// `toBase64`/`toHex`/`setFromBase64`/`setFromHex` are Uint8Array-ONLY in the
// spec (they do not exist on `Int8Array.prototype` etc.), so they are installed
// as own methods on the Uint8Array per-kind prototype via
// `install_uint8array_base64_hex_proto_methods`, NOT on the shared
// `%TypedArray%.prototype` intrinsic (`TYPED_ARRAY_PROTO_METHODS`).
//
// Pre-fix these methods dispatched on CALL (`b.toBase64(…)` reaches
// `dispatch_buffer_method` because Uint8Array instances are Buffer-backed) but
// were not materialized as readable property VALUES on the prototype, so
// `typeof Uint8Array.prototype.toBase64` read `"undefined"` and feature
// detection (`Uint8Array.prototype.toBase64 ? native : fallback`, as jose /
// Auth.js do) took the fallback path. These thunks make the value read report a
// function and route a reflective `.call`/`.apply` through the same
// `dispatch_buffer_method` base64/hex arms the instance fast path already uses.
// ---------------------------------------------------------------------------

/// Throw `TypeError: Method Uint8Array.prototype.<m> called on incompatible
/// receiver` (Test262 brand checks assert only the error *type*; the wording is
/// informational). Never returns.
fn throw_not_uint8_array(method: &str) -> ! {
    let msg = format!("Method Uint8Array.prototype.{method} called on incompatible receiver");
    let s = crate::string::js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
    let err = crate::error::js_typeerror_new(s);
    crate::exception::js_throw(f64::from_bits(
        crate::value::JSValue::pointer(err as *const u8).bits(),
    ))
}

/// Resolve the `IMPLICIT_THIS` receiver to a Uint8Array-backed buffer address.
/// The base64/hex methods only exist on Uint8Array, so a multi-byte typed-array
/// receiver (`Uint8Array.prototype.toBase64.call(new Int8Array())`) is rejected
/// with a `TypeError` just like Node's `ValidateUint8Array`.
#[inline]
unsafe fn uint8_receiver_or_throw(method: &str) -> usize {
    match ta_receiver_or_throw(method) {
        TypedArrayProtoReceiver::Uint8Buffer(addr) => addr,
        TypedArrayProtoReceiver::TypedArray(_) => throw_not_uint8_array(method),
    }
}

pub(super) extern "C" fn u8_to_base64_thunk(
    _c: *const crate::closure::ClosureHeader,
    opts: f64,
    _b: f64,
    _d: f64,
) -> f64 {
    unsafe {
        let addr = uint8_receiver_or_throw("toBase64");
        let args = [opts];
        super::dispatch_buffer_method(addr, "toBase64", args.as_ptr(), args.len())
    }
}

pub(super) extern "C" fn u8_to_hex_thunk(
    _c: *const crate::closure::ClosureHeader,
    _a: f64,
    _b: f64,
    _d: f64,
) -> f64 {
    unsafe {
        let addr = uint8_receiver_or_throw("toHex");
        super::dispatch_buffer_method(addr, "toHex", std::ptr::null(), 0)
    }
}

pub(super) extern "C" fn u8_set_from_base64_thunk(
    _c: *const crate::closure::ClosureHeader,
    str_arg: f64,
    opts: f64,
    _d: f64,
) -> f64 {
    unsafe {
        let addr = uint8_receiver_or_throw("setFromBase64");
        let args = [str_arg, opts];
        super::dispatch_buffer_method(addr, "setFromBase64", args.as_ptr(), args.len())
    }
}

pub(super) extern "C" fn u8_set_from_hex_thunk(
    _c: *const crate::closure::ClosureHeader,
    str_arg: f64,
    _b: f64,
    _d: f64,
) -> f64 {
    unsafe {
        let addr = uint8_receiver_or_throw("setFromHex");
        let args = [str_arg];
        super::dispatch_buffer_method(addr, "setFromHex", args.as_ptr(), args.len())
    }
}

/// Install the Uint8Array-only base64/hex methods as readable own methods on the
/// `Uint8Array.prototype` object (#6674). Each `(name, thunk, spec_length)` pair
/// gets the spec `.length` recorded by `install_proto_method`, then the ABI call
/// arity is widened to 3 (the uniform native thunk signature) so omitted trailing
/// arguments are padded with `undefined` instead of reading unset register slots
/// — the same convention `install_typed_array_proto_methods` uses.
pub(super) fn install_uint8array_base64_hex_proto_methods(proto_obj: *mut ObjectHeader) {
    use super::global_this::install_proto_method as ipm;
    let methods: &[(&str, *const u8, u32)] = &[
        ("toBase64", u8_to_base64_thunk as *const u8, 0),
        ("toHex", u8_to_hex_thunk as *const u8, 0),
        ("setFromBase64", u8_set_from_base64_thunk as *const u8, 1),
        ("setFromHex", u8_set_from_hex_thunk as *const u8, 1),
    ];
    for &(name, func_ptr, spec_length) in methods {
        ipm(proto_obj, name, func_ptr, spec_length);
        crate::closure::js_register_closure_arity(func_ptr, 3);
    }
}
