//! Mutating sort — default + comparator, plus the spec-ops path for exotic
//! receivers (index accessors, sparse storage, inherited prototype elements).
use super::*;
use crate::closure::{js_closure_call2, ClosureHeader};
use std::ptr;

// ---------------------------------------------------------------------------
// SortCompare helpers shared by the dense fast paths, the exotic spec path,
// and the generic array-like engine (`object_sort`).
// ---------------------------------------------------------------------------

/// Resolved comparator state: the closure + (when shapes allow) the direct
/// 2-arg call target hoisted out of the comparison loops.
#[derive(Clone, Copy)]
pub(crate) struct ComparatorCall {
    comparator: *const ClosureHeader,
    direct: Option<extern "C" fn(*const ClosureHeader, f64, f64) -> f64>,
}

impl ComparatorCall {
    pub(crate) fn new(comparator: *const ClosureHeader) -> Self {
        ComparatorCall {
            comparator,
            direct: crate::closure::resolve_call2_direct(comparator),
        }
    }

    /// Raw `Call(comparator, undefined, « a, b »)`.
    #[inline(always)]
    fn call_raw(&self, a: f64, b: f64) -> f64 {
        match self.direct {
            Some(f) => f(self.comparator, a, b),
            None => js_closure_call2(self.comparator, a, b),
        }
    }

    /// ECMA-262 `CompareArrayElements` numeric result: `ToNumber(Call(...))`
    /// with NaN → +0. A plain finite/±inf f64 result skips the coercion; any
    /// NaN-boxed value (boolean, string, object with `valueOf`, undefined)
    /// goes through real ToNumber (firing user `valueOf`, throwing on
    /// BigInt/Symbol per spec).
    #[inline(always)]
    pub(crate) fn compare(&self, a: f64, b: f64) -> f64 {
        let r = self.call_raw(a, b);
        if r == r {
            return r;
        }
        let n = crate::builtins::js_number_coerce(r);
        if n.is_nan() {
            0.0
        } else {
            n
        }
    }
}

/// ToString(value) as an owned Rust `String` for the default (no-comparator)
/// sort's lexicographic key. Fires user `toString`/`valueOf` via the runtime
/// ToString machinery.
fn sort_key_string(value: f64) -> String {
    use crate::string::StringHeader;
    use crate::value::js_jsvalue_to_string;
    let str_ptr = js_jsvalue_to_string(value);
    if str_ptr.is_null() {
        return String::new();
    }
    unsafe {
        let header = &*(str_ptr as *const StringHeader);
        let bytes_ptr = (str_ptr as *const u8).add(std::mem::size_of::<StringHeader>());
        let slice = std::slice::from_raw_parts(bytes_ptr, header.byte_len as usize);
        std::str::from_utf8(slice).unwrap_or("").to_string()
    }
}

#[inline(always)]
fn is_undefined_bits(bits: u64) -> bool {
    bits == crate::value::TAG_UNDEFINED
}

/// Stable bottom-up merge sort over a raw f64 buffer pair using `le(a, b)`
/// ("a sorts at-or-before b"). Tolerant of an inconsistent user comparator
/// (never panics, unlike `slice::sort_by` which detects total-order
/// violations). `src` and `dst` must each hold `n` elements; the sorted run
/// is guaranteed to end up back in `src`'s buffer.
///
/// SAFETY: caller keeps both buffers alive (and GC-visible when the values
/// are NaN-boxed pointers and `le` can allocate — see the rooted temp-array
/// usage in the spec path).
unsafe fn stable_merge_sort_raw(
    src0: *mut f64,
    dst0: *mut f64,
    n: usize,
    mut le: impl FnMut(f64, f64) -> bool,
) {
    if n <= 1 {
        return;
    }
    let mut src = src0;
    let mut dst = dst0;
    let mut width = 1usize;
    while width < n {
        let mut i = 0;
        while i < n {
            let left = i;
            let mid = (i + width).min(n);
            let right = (i + 2 * width).min(n);
            let (mut l, mut r, mut k) = (left, mid, left);
            // GC_STORE_AUDIT(STACK): merge writes target caller-rooted scratch buffers.
            while l < mid && r < right {
                if le(*src.add(l), *src.add(r)) {
                    *dst.add(k) = *src.add(l);
                    l += 1;
                } else {
                    *dst.add(k) = *src.add(r);
                    r += 1;
                }
                k += 1;
            }
            // GC_STORE_AUDIT(STACK): tail copies target caller-rooted scratch buffers.
            while l < mid {
                *dst.add(k) = *src.add(l);
                l += 1;
                k += 1;
            }
            // GC_STORE_AUDIT(STACK): tail copies target caller-rooted scratch buffers.
            while r < right {
                *dst.add(k) = *src.add(r);
                r += 1;
                k += 1;
            }
            i += 2 * width;
        }
        std::mem::swap(&mut src, &mut dst);
        width *= 2;
    }
    if src != src0 {
        // GC_STORE_AUDIT(STACK): final copy between the two caller-rooted buffers.
        ptr::copy_nonoverlapping(src, src0, n);
    }
}

/// Sort `defined` (no holes / no undefined) per `SortCompare`: with the user
/// comparator when present, else the default ToString lexicographic order.
/// The default path materializes each key once (eager), matching the previous
/// behavior; `String` keys make the Rust sort total (never panics).
fn sort_defined_values(defined: &mut [f64], scratch: *mut f64, cmp: Option<ComparatorCall>) {
    let n = defined.len();
    if n <= 1 {
        return;
    }
    match cmp {
        Some(c) => unsafe {
            stable_merge_sort_raw(defined.as_mut_ptr(), scratch, n, |a, b| {
                c.compare(a, b) <= 0.0
            });
        },
        None => {
            let mut pairs: Vec<(String, f64)> = Vec::with_capacity(n);
            for &v in defined.iter() {
                pairs.push((sort_key_string(v), v));
            }
            pairs.sort_by(|a, b| crate::string::utf16_cmp_bytes(a.0.as_bytes(), b.0.as_bytes()));
            for (i, (_, v)) in pairs.into_iter().enumerate() {
                defined[i] = v;
            }
        }
    }
}

/// Sort `count` values held in the element buffer of a GC-rooted array (the
/// caller keeps the array pointer on its stack). The scratch buffer is a
/// second rooted array so every value stays GC-visible across comparator
/// calls. Used by the exotic spec path and the generic array-like engine.
pub(crate) unsafe fn sort_rooted_values(
    elems: *mut f64,
    count: usize,
    cmp: Option<ComparatorCall>,
) {
    if count <= 1 {
        return;
    }
    let scratch = js_array_alloc_with_length(count as u32);
    let scratch_elems = (scratch as *mut u8).add(std::mem::size_of::<ArrayHeader>()) as *mut f64;
    match cmp {
        Some(c) => {
            stable_merge_sort_raw(elems, scratch_elems, count, |a, b| c.compare(a, b) <= 0.0);
        }
        None => {
            let mut defined: Vec<f64> = Vec::with_capacity(count);
            for i in 0..count {
                defined.push(*elems.add(i));
            }
            sort_defined_values(&mut defined, scratch_elems, None);
            for (i, v) in defined.into_iter().enumerate() {
                // GC_STORE_AUDIT(BARRIERED): caller rebuilds the rooted array's layout.
                ptr::write(elems.add(i), v);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Object.prototype indexed-property probe (sort-local).
//
// `array_spec_has_index` / `array_spec_get` consult own properties and
// `Array.prototype`, but an element can also be inherited from
// `Object.prototype` (`Object.prototype[2] = 4` — test262
// sort/precise-prototype-element). Probing per call keeps the hot iteration
// predicates in `indexing.rs` untouched; sort is the only caller that pays.
// ---------------------------------------------------------------------------

fn object_prototype_value() -> Option<f64> {
    let ctor = crate::object::js_get_global_this_builtin_value(b"Object".as_ptr(), 6);
    let ctor_v = crate::value::JSValue::from_bits(ctor.to_bits());
    if !ctor_v.is_pointer() {
        return None;
    }
    let proto =
        crate::closure::closure_get_dynamic_prop(ctor_v.as_pointer::<u8>() as usize, "prototype");
    let proto_v = crate::value::JSValue::from_bits(proto.to_bits());
    if proto_v.is_pointer() {
        Some(proto)
    } else {
        None
    }
}

/// Own array-index keys of `Object.prototype` (usually empty). Computed once
/// per sort call; the result gates the per-index inherited reads below.
fn object_prototype_numeric_keys() -> Vec<u32> {
    let Some(proto) = object_prototype_value() else {
        return Vec::new();
    };
    let names = crate::object::js_object_get_own_property_names(proto);
    let jv = crate::value::JSValue::from_bits(names.to_bits());
    if !jv.is_pointer() {
        return Vec::new();
    }
    let arr = jv.as_pointer::<ArrayHeader>();
    if arr.is_null() {
        return Vec::new();
    }
    let mut keys = Vec::new();
    unsafe {
        let len = (*arr).length as usize;
        let elems = (arr as *const u8).add(std::mem::size_of::<ArrayHeader>()) as *const f64;
        for i in 0..len {
            let s = sort_key_string(*elems.add(i));
            if !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()) {
                if let Ok(k) = s.parse::<u32>() {
                    keys.push(k);
                }
            }
        }
    }
    keys
}

pub(crate) fn object_prototype_index_get(index: u32) -> f64 {
    match object_prototype_value() {
        Some(proto) => {
            // Fire an accessor getter installed via
            // `Object.defineProperty(Object.prototype, '<i>', { get })` — the
            // polymorphic read misses the descriptor side table.
            let addr = (proto.to_bits() & crate::value::POINTER_MASK) as usize;
            if let Some(acc) = crate::object::get_accessor_descriptor(addr, &index.to_string()) {
                if acc.get != 0 {
                    return f64::from_bits(
                        unsafe { crate::object::invoke_accessor_getter(acc.get, proto) }.bits(),
                    );
                }
                return f64::from_bits(crate::value::TAG_UNDEFINED);
            }
            crate::object::js_object_get_index_polymorphic(proto.to_bits() as i64, index as f64)
        }
        None => f64::from_bits(crate::value::TAG_UNDEFINED),
    }
}

/// `true` when `Object.prototype` carries an own `<index>` property (data or
/// accessor). Used by the sort spec path and the `in` operator's array arm.
pub(crate) fn object_prototype_has_index_prop(index: u32) -> bool {
    let Some(proto) = object_prototype_value() else {
        return false;
    };
    let addr = (proto.to_bits() & crate::value::POINTER_MASK) as usize;
    if crate::object::get_accessor_descriptor(addr, &index.to_string()).is_some() {
        return true;
    }
    let s = index.to_string();
    let key = crate::string::js_string_from_bytes(s.as_ptr(), s.len() as u32);
    let key_v = f64::from_bits(crate::value::JSValue::string_ptr(key).bits());
    crate::object::js_object_has_own(proto, key_v).to_bits() == 0x7FFC_0000_0000_0004
}

/// Spec `Set(O, ToString(j), v, true)` for the sort write-back: when the
/// receiver has NO own property at `j` and `Object.prototype` carries an
/// accessor there, OrdinarySet walks the chain and invokes that SETTER with
/// the array as receiver (test262 sort/precise-prototype-accessors) — a plain
/// `js_array_set_f64_extend` would create an own data slot instead.
unsafe fn sort_spec_set(
    arr: *mut ArrayHeader,
    index: u32,
    value: f64,
    objproto_keys: &[u32],
) -> *mut ArrayHeader {
    if objproto_keys.contains(&index) && !crate::array::array_has_own_index(arr, index) {
        if let Some(proto) = object_prototype_value() {
            let addr = (proto.to_bits() & crate::value::POINTER_MASK) as usize;
            if let Some(acc) = crate::object::get_accessor_descriptor(addr, &index.to_string()) {
                if acc.set != 0 {
                    crate::object::invoke_accessor_setter(
                        acc.set,
                        crate::value::js_nanbox_pointer(arr as i64),
                        value,
                    );
                }
                return arr;
            }
        }
    }
    js_array_set_f64_extend(arr, index, value)
}

// ---------------------------------------------------------------------------
// Spec-ops sort path for a real-array receiver (ECMA-262 §23.1.3.30
// SortIndexedProperties with holes skipped): collect via [[HasProperty]] /
// [[Get]] (own accessors + Array.prototype + Object.prototype), sort the
// collected values (undefined trailing, never fed to the comparator), then
// write back via [[Set]] (firing setters) and [[Delete]] the trailing range.
// ---------------------------------------------------------------------------

unsafe fn array_sort_spec_path(
    arr: *mut ArrayHeader,
    cmp: Option<ComparatorCall>,
    objproto_keys: &[u32],
) -> *mut ArrayHeader {
    let len = (*arr).length;

    // Collect present elements into a GC-rooted temp array (stack-local
    // pointer keeps it alive under the conservative scan; its element buffer
    // keeps accessor-produced values alive across comparator calls — a plain
    // Rust Vec would not be traced).
    let temp = js_array_alloc_with_length(len);
    let temp_elems = (temp as *mut u8).add(std::mem::size_of::<ArrayHeader>()) as *mut f64;
    let mut count = 0usize;
    let mut undef_count = 0usize;
    for j in 0..len {
        let (present, value) = if crate::array::array_spec_has_index(arr, j) {
            (true, crate::array::array_spec_get(arr, j))
        } else if objproto_keys.contains(&j) {
            (true, object_prototype_index_get(j))
        } else {
            (false, 0.0)
        };
        if present {
            if is_undefined_bits(value.to_bits()) {
                undef_count += 1;
            } else {
                // GC_STORE_AUDIT(BARRIERED): temp collection array is rebuilt below.
                ptr::write(temp_elems.add(count), value);
                count += 1;
            }
        }
    }
    (*temp).length = count as u32;
    rebuild_array_layout(temp);
    let item_count = count + undef_count;

    // Sort the defined values; the scratch buffer is a second rooted array.
    sort_rooted_values(temp_elems, count, cmp);
    rebuild_array_layout(temp);

    // Write back via [[Set]] (fires index setters / honors attrs), then
    // [[Delete]] the trailing [itemCount, len) range — restoring sparseness.
    let mut cur = arr;
    for j in 0..count {
        cur = sort_spec_set(cur, j as u32, *temp_elems.add(j), objproto_keys);
    }
    for j in count..item_count {
        cur = sort_spec_set(
            cur,
            j as u32,
            f64::from_bits(crate::value::TAG_UNDEFINED),
            objproto_keys,
        );
    }
    for j in item_count..len as usize {
        crate::array::js_array_delete(cur, j as u32);
    }
    cur
}

/// Whether the dense raw-store sort would diverge from the spec protocol for
/// this receiver. Mirrors `array_iteration_is_exotic`, plus the
/// `Object.prototype` numeric-key pollution case that only matters when the
/// array actually has holes for an inherited element to show through.
unsafe fn sort_needs_spec_path(arr: *const ArrayHeader, objproto_keys: &[u32]) -> bool {
    if crate::array::array_iteration_is_exotic(arr) {
        return true;
    }
    if objproto_keys.is_empty() {
        return false;
    }
    let length = (*arr).length as usize;
    let elements = (arr as *const u8).add(std::mem::size_of::<ArrayHeader>()) as *const f64;
    (0..length).any(|i| (*elements.add(i)).to_bits() == crate::value::TAG_HOLE)
}

/// Array.prototype.sort() default sort with no comparator. Per JS
/// semantics, elements are converted to strings and compared
/// lexicographically; undefined elements trail every defined value and holes
/// trail those (neither is ever compared). Sorts in place and returns the
/// same array pointer.
#[no_mangle]
pub extern "C" fn js_array_sort_default(arr: *mut ArrayHeader) -> *mut ArrayHeader {
    unsafe {
        // Runtime plain-object receiver behind a statically-Array variable
        // (test262 sort/S15.4.4.11_A6_T2 #5) — run the generic engine.
        // Probe the RAW pointer BEFORE the array-plausibility clean (which
        // may NULL an object receiver out, silently no-op'ing the sort).
        if let Some(recv) = crate::array::non_array_object_receiver(arr) {
            crate::array::object_sort(recv, std::ptr::null());
            return arr;
        }
        let arr = clean_arr_ptr(arr as *const ArrayHeader) as *mut ArrayHeader;
        if arr.is_null() {
            return arr;
        }
        // Issue #654: route typed-array receivers (compiler statically
        // typed `arr` as `Float64Array | Int32Array | …` and emitted the
        // ArraySort lowering) through the typed-array sorter so element
        // bytes are read by the right per-kind accessor instead of as
        // raw f64. Without this, `Int8Array.sort()` produced 4 i8 cells
        // re-interpreted as 8-byte f64s — garbage values + occasional
        // OOB reads.
        if crate::typedarray::lookup_typed_array_kind(arr as usize).is_some() {
            return crate::typedarray::js_typed_array_sort_default(
                arr as *mut crate::typedarray::TypedArrayHeader,
            ) as *mut ArrayHeader;
        }
        sort_array_receiver(arr, None)
    }
}

/// Validate a `sort` / `toSorted` comparator argument (#2796).
///
/// Per ECMA-262, the comparator must be either `undefined` or a callable
/// function; any other value throws `TypeError` *before* sorting begins.
/// Takes the raw NaN-boxed comparator value (NOT a pre-unboxed pointer) so
/// it can distinguish `undefined`/`null`/numbers/etc.
///
/// Returns the resolved `ClosureHeader*` (as `i64`) for the comparator path,
/// or `0` when the argument is `undefined` (use the default sort path).
#[no_mangle]
pub extern "C" fn js_validate_array_comparator(cmp_boxed: f64) -> i64 {
    use crate::value::JSValue;
    let jv = JSValue::from_bits(cmp_boxed.to_bits());
    // undefined -> default sort path.
    if jv.is_undefined() {
        return 0;
    }
    // Callable function -> comparator path.
    if jv.is_pointer() {
        let ptr = jv.as_pointer::<ClosureHeader>();
        if !ptr.is_null() && unsafe { (*ptr).type_tag == crate::closure::CLOSURE_MAGIC } {
            return ptr as i64;
        }
    }
    // Anything else (null, number, string, object, boolean) is a TypeError.
    throw_invalid_comparator(cmp_boxed);
}

#[used]
static KEEP_VALIDATE_ARRAY_COMPARATOR: extern "C" fn(f64) -> i64 = js_validate_array_comparator;

#[cold]
fn throw_invalid_comparator(cmp_boxed: f64) -> ! {
    // Stringify the supplied value the way Node renders it in the message,
    // e.g. "null", "1". `js_jsvalue_to_string` yields the JS String form.
    let value_str = {
        let sp = crate::value::js_jsvalue_to_string(cmp_boxed);
        if sp.is_null() {
            String::new()
        } else {
            unsafe {
                let header = &*(sp as *const crate::string::StringHeader);
                let bytes_ptr =
                    (sp as *const u8).add(std::mem::size_of::<crate::string::StringHeader>());
                let slice = std::slice::from_raw_parts(bytes_ptr, header.byte_len as usize);
                std::str::from_utf8(slice).unwrap_or("").to_string()
            }
        }
    };
    let message = format!(
        "The comparison function must be either a function or undefined: {}",
        value_str
    );
    let msg = crate::string::js_string_from_bytes(message.as_ptr(), message.len() as u32);
    let err = crate::error::js_typeerror_new(msg);
    crate::exception::js_throw(crate::value::js_nanbox_pointer(err as i64));
}

/// sort - sort array in-place using a comparator closure
/// The comparator takes (a, b) and returns negative if a < b, positive if a > b, 0 if equal
/// Returns the same array pointer (sorts in-place)
#[no_mangle]
pub extern "C" fn js_array_sort_with_comparator(
    arr: *mut ArrayHeader,
    comparator: *const ClosureHeader,
) -> *mut ArrayHeader {
    // #2796: a null comparator (validated `undefined`, or absent) means
    // "use the default sort path".
    if comparator.is_null() {
        return js_array_sort_default(arr);
    }
    unsafe {
        // Runtime plain-object receiver behind a statically-Array variable —
        // probe the RAW pointer before the array-plausibility clean.
        if let Some(recv) = crate::array::non_array_object_receiver(arr) {
            crate::array::object_sort(recv, comparator);
            return arr;
        }
        let arr = clean_arr_ptr(arr as *const ArrayHeader) as *mut ArrayHeader;
        if arr.is_null() {
            return arr;
        }
        // Issue #654: same routing as `js_array_sort_default` — when
        // codegen statically typed the receiver as a typed array but
        // chose the generic ArraySort HIR lowering, dispatch through
        // the typed-array helper instead of treating the buffer as f64s.
        if crate::typedarray::lookup_typed_array_kind(arr as usize).is_some() {
            return crate::typedarray::js_typed_array_sort_with_comparator(
                arr as *mut crate::typedarray::TypedArrayHeader,
                comparator,
            ) as *mut ArrayHeader;
        }
        sort_array_receiver(arr, Some(ComparatorCall::new(comparator)))
    }
}

/// Shared real-array sort body: route exotic receivers to the spec-ops path,
/// dense receivers to the in-place fast paths (with the hole/undefined
/// partition applied for BOTH the default and comparator sorts).
unsafe fn sort_array_receiver(
    arr: *mut ArrayHeader,
    cmp: Option<ComparatorCall>,
) -> *mut ArrayHeader {
    let objproto_keys = object_prototype_numeric_keys();
    if sort_needs_spec_path(arr, &objproto_keys) {
        return array_sort_spec_path(arr, cmp, &objproto_keys);
    }

    let length = (*arr).length as usize;
    if length <= 1 {
        return arr;
    }
    let elements_ptr = (arr as *mut u8).add(std::mem::size_of::<ArrayHeader>()) as *mut f64;

    // ECMAScript SortIndexedProperties + CompareArrayElements: array holes
    // are excluded from the sort and trail every element, and `undefined`
    // elements sort to the very end (after all defined values) WITHOUT the
    // comparator / ToString ever running for them. Detect their presence up
    // front; a dense, all-defined array keeps the fast in-place path.
    let mut has_special = false;
    for i in 0..length {
        let bits = (*elements_ptr.add(i)).to_bits();
        if bits == crate::value::TAG_HOLE || is_undefined_bits(bits) {
            has_special = true;
            break;
        }
    }
    if has_special {
        // Gather defined (non-hole, non-undefined) elements; tally the
        // undefined and hole counts to re-emit as a trailing suffix.
        let mut defined: Vec<f64> = Vec::with_capacity(length);
        let mut undef_count = 0usize;
        let mut hole_count = 0usize;
        for i in 0..length {
            let v = *elements_ptr.add(i);
            let bits = v.to_bits();
            if bits == crate::value::TAG_HOLE {
                hole_count += 1;
            } else if is_undefined_bits(bits) {
                undef_count += 1;
            } else {
                defined.push(v);
            }
        }
        // All defined values remain reachable through the (unmodified) array
        // storage until the write-back below, so the Vec scratch is safe.
        let n = defined.len();
        if n > 1 {
            let mut buf: Vec<f64> = vec![0.0; n];
            sort_defined_values(&mut defined, buf.as_mut_ptr(), cmp);
        }
        // Write back: sorted defined values, then `undefined` ×N, then
        // holes ×N — restoring the array's exotic sparseness.
        mark_array_layout_unknown(arr);
        let mut idx = 0usize;
        // GC_STORE_AUDIT(BARRIERED): write-back is included in the rebuild below.
        for &v in &defined {
            *elements_ptr.add(idx) = v;
            idx += 1;
        }
        // GC_STORE_AUDIT(POINTER_FREE): undefined/hole suffix has no child pointer;
        // covered by the rebuild below anyway.
        for _ in 0..undef_count {
            *elements_ptr.add(idx) = f64::from_bits(crate::value::TAG_UNDEFINED);
            idx += 1;
        }
        // GC_STORE_AUDIT(POINTER_FREE): hole suffix has no child pointer.
        for _ in 0..hole_count {
            *elements_ptr.add(idx) = f64::from_bits(crate::value::TAG_HOLE);
            idx += 1;
        }
        rebuild_array_layout(arr);
        return arr;
    }

    let Some(c) = cmp else {
        // Dense all-defined default sort: materialize each element's string
        // key once, sort stably on the keys, write back.
        let mut pairs: Vec<(String, f64)> = Vec::with_capacity(length);
        for i in 0..length {
            let val = *elements_ptr.add(i);
            pairs.push((sort_key_string(val), val));
        }
        pairs.sort_by(|a, b| crate::string::utf16_cmp_bytes(a.0.as_bytes(), b.0.as_bytes()));
        mark_array_layout_unknown(arr);
        for (i, (_, val)) in pairs.into_iter().enumerate() {
            // GC_STORE_AUDIT(BARRIERED): default sort writes are followed by layout/barrier rebuild.
            *elements_ptr.add(i) = val;
        }
        rebuild_array_layout(arr);
        return arr;
    };

    mark_array_layout_unknown(arr);

    // TimSort-style hybrid: insertion sort for small runs, merge sort for large arrays.
    // Stable, O(n log n) worst case. Insertion sort is used for runs <= 32 elements
    // because it has lower overhead for small inputs.
    const INSERTION_THRESHOLD: usize = 32;

    if length <= INSERTION_THRESHOLD {
        // Insertion sort for small arrays
        for i in 1..length {
            let key = *elements_ptr.add(i);
            let mut j = i as isize - 1;
            while j >= 0 {
                if c.compare(*elements_ptr.add(j as usize), key) > 0.0 {
                    // GC_STORE_AUDIT(BARRIERED): insertion-sort shift is included in the rebuild below.
                    ptr::write(
                        elements_ptr.add((j + 1) as usize),
                        *elements_ptr.add(j as usize),
                    );
                    j -= 1;
                } else {
                    break;
                }
            }
            // GC_STORE_AUDIT(BARRIERED): insertion-sort key write is included in the rebuild below.
            ptr::write(elements_ptr.add((j + 1) as usize), key);
        }
    } else {
        // Bottom-up merge sort for large arrays — O(n log n) stable sort
        let mut buf: Vec<f64> = Vec::with_capacity(length);
        buf.set_len(length);

        // Phase 1: Sort small runs with insertion sort
        let mut run_start = 0;
        while run_start < length {
            let run_end = (run_start + INSERTION_THRESHOLD).min(length);
            for i in (run_start + 1)..run_end {
                let key = *elements_ptr.add(i);
                let mut j = i as isize - 1;
                while j >= run_start as isize {
                    if c.compare(*elements_ptr.add(j as usize), key) > 0.0 {
                        // GC_STORE_AUDIT(BARRIERED): large-sort insertion shift is included in the rebuild below.
                        ptr::write(
                            elements_ptr.add((j + 1) as usize),
                            *elements_ptr.add(j as usize),
                        );
                        j -= 1;
                    } else {
                        break;
                    }
                }
                // GC_STORE_AUDIT(BARRIERED): large-sort insertion key write is included in the rebuild below.
                ptr::write(elements_ptr.add((j + 1) as usize), key);
            }
            run_start = run_end;
        }

        // Phase 2: Merge runs, doubling width each pass. Values are always
        // present in either the array storage or the scratch buffer; the
        // array itself roots them for the conservative scan.
        let mut width = INSERTION_THRESHOLD;
        let mut src = elements_ptr;
        let mut dst = buf.as_mut_ptr();

        while width < length {
            let mut i = 0;
            while i < length {
                let left = i;
                let mid = (i + width).min(length);
                let right = (i + 2 * width).min(length);

                let (mut l, mut r, mut k) = (left, mid, left);
                // GC_STORE_AUDIT(STACK): merge destination is a function-local Vec buffer, not GC heap.
                while l < mid && r < right {
                    if c.compare(*src.add(l), *src.add(r)) <= 0.0 {
                        *dst.add(k) = *src.add(l);
                        l += 1;
                    } else {
                        *dst.add(k) = *src.add(r);
                        r += 1;
                    }
                    k += 1;
                }
                // GC_STORE_AUDIT(STACK): remaining left run copies into the temporary merge buffer.
                while l < mid {
                    *dst.add(k) = *src.add(l);
                    l += 1;
                    k += 1;
                }
                // GC_STORE_AUDIT(STACK): remaining right run copies into the temporary merge buffer.
                while r < right {
                    *dst.add(k) = *src.add(r);
                    r += 1;
                    k += 1;
                }

                i += 2 * width;
            }
            // Swap src and dst for next pass
            std::mem::swap(&mut src, &mut dst);
            width *= 2;
        }

        // If final result is in buf, copy back to elements
        if src != elements_ptr {
            // GC_STORE_AUDIT(BARRIERED): merge buffer copyback is followed by layout/barrier rebuild.
            ptr::copy_nonoverlapping(src, elements_ptr, length);
        }
    }
    rebuild_array_layout(arr);

    arr
}
