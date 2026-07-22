//! Mutating sort — default + comparator, plus the spec-ops path for exotic
//! receivers (index accessors, sparse storage, inherited prototype elements).
use super::*;
use crate::closure::{js_closure_call2, ClosureHeader};

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

    /// ECMA-262 `CompareArrayElements` numeric result: `ToNumber(Call(...))`
    /// with NaN → +0. A plain finite/±inf f64 result skips the coercion; any
    /// NaN-boxed value (boolean, string, object with `valueOf`, undefined)
    /// goes through real ToNumber (firing user `valueOf`, throwing on
    /// BigInt/Symbol per spec).
    ///
    /// Takes an explicitly re-derived closure header rather than using the
    /// one cached in `self`: the sorting engines root the comparator in a
    /// `RuntimeHandleScope` and pass the CURRENT address here after every
    /// user-code window — a comparator that allocates can trigger a moving
    /// minor GC that relocates its own closure header, and the raw pointer
    /// cached in `self.comparator` then points at from-space. `direct` stays
    /// valid: it is a static code address resolved from the closure's shape,
    /// which relocation does not change.
    #[inline(always)]
    pub(crate) fn compare_at(&self, comparator: *const ClosureHeader, a: f64, b: f64) -> f64 {
        let r = match self.direct {
            Some(f) => f(comparator, a, b),
            None => js_closure_call2(comparator, a, b),
        };
        if !r.is_nan() {
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

/// TimSort-style hybrid threshold shared by the sorting engines below:
/// insertion sort for runs of at most this many elements, bottom-up merges
/// above it.
const INSERTION_THRESHOLD: usize = 32;

/// GC-rooted view of an array's inline element storage. The header pointer
/// lives in a `RuntimeHandleScope` slot (marked AND rewritten by a moving
/// collection), and the element base is re-derived from the CURRENT header
/// address on every access. A comparator — or an accessor fired by the spec
/// ops — is user code: it can allocate → trigger a minor GC → the array is
/// swept (bare Rust locals are invisible without a conservative stack scan,
/// which production does not run) or moved (alloc-point minors can be moving
/// under the evacuation policy), and any hoisted `*mut f64` then reads or
/// writes from-space garbage. Mirrors the PR #5981 `RootedIterArray` pattern
/// in `iter_methods.rs`.
pub(crate) struct RootedArrayElems<'s> {
    handle: crate::gc::RuntimeHandle<'s>,
}

impl<'s> RootedArrayElems<'s> {
    pub(crate) fn new(scope: &'s crate::gc::RuntimeHandleScope, arr: *mut ArrayHeader) -> Self {
        Self {
            handle: scope.root_raw_mut_ptr(arr),
        }
    }

    /// The CURRENT (post-any-GC) header address.
    #[inline(always)]
    pub(crate) fn arr(&self) -> *mut ArrayHeader {
        self.handle.get_raw_mut_ptr::<ArrayHeader>()
    }

    #[inline(always)]
    pub(crate) unsafe fn get(&self, index: usize) -> f64 {
        let arr = self.arr();
        *((arr as *const u8).add(std::mem::size_of::<ArrayHeader>()) as *const f64).add(index)
    }

    /// Barriered store (`note_array_slot`): keeps the layout side-table and
    /// the remembered set coherent even when the array is tenured mid-sort.
    #[inline(always)]
    pub(crate) unsafe fn set(&self, index: usize, value: f64) {
        note_array_slot(self.arr(), index, value.to_bits());
    }
}

/// In-place insertion sort of `vals[start..end]` under `le(a, b)` ("a sorts
/// at-or-before b"). Swap-based: the moving key is never parked in a Rust
/// local across a comparator call, so the authoritative element bits always
/// live in the rooted array and get rewritten in place by a moving GC.
unsafe fn insertion_sort_rooted(
    vals: &RootedArrayElems<'_>,
    start: usize,
    end: usize,
    le: &mut impl FnMut(f64, f64) -> bool,
) {
    for i in (start + 1)..end {
        let mut j = i;
        while j > start {
            // `le` is user code — both operands are re-read AFTER it returns
            // (their slots were rewritten in place if the array moved).
            if le(vals.get(j - 1), vals.get(j)) {
                break;
            }
            let prev = vals.get(j - 1);
            let cur = vals.get(j);
            vals.set(j - 1, cur);
            vals.set(j, prev);
            j -= 1;
        }
    }
}

/// Stable bottom-up merge sort over TWO rooted element buffers: `data` holds
/// the values, `scratch` is the ping-pong buffer, and runs of `start_width`
/// are assumed already sorted. Tolerant of an inconsistent user comparator
/// (never panics, unlike `slice::sort_by`). Every element access re-derives
/// the buffer base from its rooted handle and re-reads the winning element
/// AFTER each comparator call — the authoritative bits always live in one of
/// the two GC-visible arrays, never in an unrooted Rust buffer. (The #6076
/// merge engine ping-ponged through a bare `Vec<f64>`: after the first src/dst
/// swap the authoritative bits lived in that Vec, and a moving minor during a
/// comparator call left pre-move addresses in the published result.)
unsafe fn stable_merge_sort_rooted(
    data: &RootedArrayElems<'_>,
    scratch: &RootedArrayElems<'_>,
    n: usize,
    start_width: usize,
    mut le: impl FnMut(f64, f64) -> bool,
) {
    if n <= 1 {
        return;
    }
    let mut in_data = true; // which buffer currently holds the runs
    let mut width = start_width.max(1);
    while width < n {
        let (src, dst) = if in_data {
            (data, scratch)
        } else {
            (scratch, data)
        };
        let mut i = 0usize;
        while i < n {
            let left = i;
            let mid = (i + width).min(n);
            let right = (i + 2 * width).min(n);
            let (mut l, mut r, mut k) = (left, mid, left);
            while l < mid && r < right {
                if le(src.get(l), src.get(r)) {
                    dst.set(k, src.get(l));
                    l += 1;
                } else {
                    dst.set(k, src.get(r));
                    r += 1;
                }
                k += 1;
            }
            while l < mid {
                dst.set(k, src.get(l));
                l += 1;
                k += 1;
            }
            while r < right {
                dst.set(k, src.get(r));
                r += 1;
                k += 1;
            }
            i += 2 * width;
        }
        in_data = !in_data;
        width *= 2;
    }
    if !in_data {
        // Final runs landed in scratch — copy back (no user code here).
        for i in 0..n {
            data.set(i, scratch.get(i));
        }
    }
}

/// Comparator sort of `data[0..n]` (hybrid insertion + merge) with the
/// comparator header re-derived from `cmp_handle` before every call.
unsafe fn sort_comparator_rooted(
    data: &RootedArrayElems<'_>,
    scratch: &RootedArrayElems<'_>,
    n: usize,
    c: &ComparatorCall,
    cmp_handle: &crate::gc::RuntimeHandle<'_>,
) {
    let mut le = |a: f64, b: f64| -> bool {
        c.compare_at(cmp_handle.get_raw_const_ptr::<ClosureHeader>(), a, b) <= 0.0
    };
    // Phase 1: insertion-sort each small run in place.
    let mut run_start = 0usize;
    while run_start < n {
        let run_end = (run_start + INSERTION_THRESHOLD).min(n);
        insertion_sort_rooted(data, run_start, run_end, &mut le);
        run_start = run_end;
    }
    // Phase 2: bottom-up merges, ping-ponging between the two rooted buffers.
    stable_merge_sort_rooted(data, scratch, n, INSERTION_THRESHOLD, le);
}

/// Default (no-comparator) SortCompare over a rooted buffer: ToString each
/// element (user `toString`/`valueOf` can run — and can sweep or move the
/// buffer), sort a rank permutation on the owned `String` keys (GC-inert,
/// total order — never panics), then apply the permutation. Element bits are
/// only ever read fresh from the rooted array after the last user code.
unsafe fn sort_default_rooted(vals: &RootedArrayElems<'_>, n: usize) {
    if n <= 1 {
        return;
    }
    let mut keys: Vec<String> = Vec::with_capacity(n);
    for i in 0..n {
        keys.push(sort_key_string(vals.get(i)));
    }
    let mut order: Vec<u32> = (0..n as u32).collect();
    order.sort_by(|&a, &b| {
        crate::string::utf16_cmp_bytes(keys[a as usize].as_bytes(), keys[b as usize].as_bytes())
    });
    // No user code below — a plain snapshot Vec is safe for the permutation.
    let snapshot: Vec<f64> = (0..n).map(|i| vals.get(i)).collect();
    for (i, &src) in order.iter().enumerate() {
        vals.set(i, snapshot[src as usize]);
    }
}

/// Sort the first `count` element slots of `temp` (a dense collection array
/// with no holes / no undefined) per `SortCompare`: with the user comparator
/// when present, else the default ToString lexicographic order. `temp` is
/// rooted here for the duration — the caller's raw pointer may be STALE on
/// return whenever a comparator / ToString moved the array, so the current
/// header is returned; callers must either switch to it or hold their own
/// rooted handle. Used by the exotic spec path and the generic array-like
/// engine.
pub(crate) unsafe fn sort_rooted_values(
    temp: *mut ArrayHeader,
    count: usize,
    cmp: Option<ComparatorCall>,
) -> *mut ArrayHeader {
    if count <= 1 {
        return temp;
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let data = RootedArrayElems::new(&scope, temp);
    match cmp {
        Some(c) => {
            let cmp_handle = scope.root_raw_const_ptr(c.comparator);
            let scratch = RootedArrayElems::new(&scope, js_array_alloc_with_length(count as u32));
            sort_comparator_rooted(&data, &scratch, count, &c, &cmp_handle);
        }
        None => sort_default_rooted(&data, count),
    }
    data.arr()
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
        // Root the freshly-built names array: `sort_key_string` can allocate
        // (ToString of a non-string key), and a GC at that allocation point
        // would sweep — or move — the otherwise-unreferenced array while the
        // loop still reads its elements.
        let scope = crate::gc::RuntimeHandleScope::new();
        let names = RootedArrayElems::new(&scope, arr as *mut ArrayHeader);
        let len = (*names.arr()).length as usize;
        for i in 0..len {
            let s = sort_key_string(names.get(i));
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
///
/// The receiver travels as a rooted handle (updated in place when
/// `js_array_set_f64_extend` reallocates it, or when a setter-triggered GC
/// moves it), and `value` is rooted for the duration: the prototype probe
/// above the write can allocate, and a moving minor at that point would
/// otherwise leave `value` holding a pre-move address.
unsafe fn sort_spec_set(
    arr_handle: &crate::gc::RuntimeHandle<'_>,
    index: u32,
    value: f64,
    objproto_keys: &[u32],
) {
    let scope = crate::gc::RuntimeHandleScope::new();
    let value_handle = scope.root_nanbox_f64(value);
    if objproto_keys.contains(&index)
        && !crate::array::array_has_own_index(arr_handle.get_raw_mut_ptr::<ArrayHeader>(), index)
    {
        if let Some(proto) = object_prototype_value() {
            let addr = (proto.to_bits() & crate::value::POINTER_MASK) as usize;
            if let Some(acc) = crate::object::get_accessor_descriptor(addr, &index.to_string()) {
                if acc.set != 0 {
                    crate::object::invoke_accessor_setter(
                        acc.set,
                        crate::value::js_nanbox_pointer(
                            arr_handle.get_raw_mut_ptr::<ArrayHeader>() as i64,
                        ),
                        value_handle.get_nanbox_f64(),
                    );
                }
                return;
            }
        }
    }
    let updated = js_array_set_f64_extend(
        arr_handle.get_raw_mut_ptr::<ArrayHeader>(),
        index,
        value_handle.get_nanbox_f64(),
    );
    arr_handle.set_raw_mut_ptr(updated);
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

    // Root the receiver BEFORE the temp allocation below (which can trigger
    // GC) and re-derive it after every spec op — [[HasProperty]]/[[Get]]/
    // [[Set]] fire user accessors that can allocate, sweeping or moving it.
    let scope = crate::gc::RuntimeHandleScope::new();
    let arr_handle = scope.root_raw_mut_ptr(arr);

    // Collect present elements into a GC-rooted temp array whose element
    // buffer keeps accessor-produced values alive — and CURRENT — across
    // comparator calls (a plain Rust Vec is invisible to the collector).
    let temp = RootedArrayElems::new(&scope, js_array_alloc_with_length(len));
    let mut count = 0usize;
    let mut undef_count = 0usize;
    for j in 0..len {
        let (present, value) =
            if crate::array::array_spec_has_index(arr_handle.get_raw_mut_ptr::<ArrayHeader>(), j) {
                (
                    true,
                    crate::array::array_spec_get(arr_handle.get_raw_mut_ptr::<ArrayHeader>(), j),
                )
            } else if objproto_keys.contains(&j) {
                (true, object_prototype_index_get(j))
            } else {
                (false, 0.0)
            };
        if present {
            if is_undefined_bits(value.to_bits()) {
                undef_count += 1;
            } else {
                temp.set(count, value);
                count += 1;
            }
        }
    }
    (*temp.arr()).length = count as u32;
    rebuild_array_layout(temp.arr());
    let item_count = count + undef_count;

    // Sort the defined values; the scratch buffer is a second rooted array.
    let _ = sort_rooted_values(temp.arr(), count, cmp);

    // Write back via [[Set]] (fires index setters / honors attrs), then
    // [[Delete]] the trailing [itemCount, len) range — restoring sparseness.
    // The receiver handle is updated in place by `sort_spec_set`, and the
    // sorted value is re-read from the rooted temp AFTER the previous
    // element's (possibly user-code-running) write.
    for j in 0..count {
        sort_spec_set(&arr_handle, j as u32, temp.get(j), objproto_keys);
    }
    for j in count..item_count {
        sort_spec_set(
            &arr_handle,
            j as u32,
            f64::from_bits(crate::value::TAG_UNDEFINED),
            objproto_keys,
        );
    }
    for j in item_count..len as usize {
        crate::array::js_array_delete(arr_handle.get_raw_mut_ptr::<ArrayHeader>(), j as u32);
    }
    arr_handle.get_raw_mut_ptr::<ArrayHeader>()
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
    // Root the receiver up front: every branch below can run user code
    // (comparator, ToString, prototype accessors) or allocate before touching
    // it again, and a bare Rust local is invisible to the collector.
    let scope = crate::gc::RuntimeHandleScope::new();
    let arr_handle = scope.root_raw_mut_ptr(arr);
    let objproto_keys = object_prototype_numeric_keys();
    let arr = arr_handle.get_raw_mut_ptr::<ArrayHeader>();
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
        // Gather defined (non-hole, non-undefined) elements into a rooted
        // collection array; tally the undefined and hole counts to re-emit as
        // a trailing suffix. (A Rust Vec held the defined values before — a
        // moving minor during a comparator / ToString call rewrote the array
        // storage but left pre-move addresses in the Vec's copies.)
        let temp = RootedArrayElems::new(&scope, js_array_alloc_with_length(length as u32));
        let mut count = 0usize;
        let mut undef_count = 0usize;
        let mut hole_count = 0usize;
        {
            // Re-derive after the temp allocation above (which can GC).
            let arr = arr_handle.get_raw_mut_ptr::<ArrayHeader>();
            let elements_ptr = (arr as *mut u8).add(std::mem::size_of::<ArrayHeader>()) as *mut f64;
            for i in 0..length {
                let v = *elements_ptr.add(i);
                let bits = v.to_bits();
                if bits == crate::value::TAG_HOLE {
                    hole_count += 1;
                } else if is_undefined_bits(bits) {
                    undef_count += 1;
                } else {
                    temp.set(count, v);
                    count += 1;
                }
            }
        }
        (*temp.arr()).length = count as u32;
        rebuild_array_layout(temp.arr());
        let _ = sort_rooted_values(temp.arr(), count, cmp);
        // Write back (no user code below): sorted defined values, then
        // `undefined` ×N, then holes ×N — restoring the exotic sparseness.
        let arr = arr_handle.get_raw_mut_ptr::<ArrayHeader>();
        let elements_ptr = (arr as *mut u8).add(std::mem::size_of::<ArrayHeader>()) as *mut f64;
        mark_array_layout_unknown(arr);
        let mut idx = 0usize;
        // GC_STORE_AUDIT(BARRIERED): write-back is included in the rebuild below.
        for i in 0..count {
            *elements_ptr.add(idx) = temp.get(i);
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
        // Dense all-defined default sort: rank-permutation on owned string
        // keys over the rooted receiver — a user `toString` can allocate and
        // sweep/move the array mid-key-materialization, so element bits are
        // only ever read fresh from the rooted handle. A throwing `toString`
        // unwinds during the key pass, leaving the elements untouched.
        let vals = RootedArrayElems::new(&scope, arr);
        sort_default_rooted(&vals, length);
        return vals.arr();
    };

    // #6076: sort a GC-rooted COPY of the elements and publish it back only
    // after the comparator sort SUCCEEDS. A throwing comparator therefore
    // leaves the receiver's elements intact (an in-place sort corrupts them).
    // Both the copy AND the merge scratch are rooted arrays: the merge engine
    // ping-pongs between two GC-visible buffers, so a moving minor during a
    // comparator call rewrites whichever buffer holds the authoritative bits
    // (the previous engine's bare `Vec<f64>` kept pre-move addresses and
    // published them back into the receiver).
    let cmp_handle = scope.root_raw_const_ptr(c.comparator);
    let temp = RootedArrayElems::new(&scope, js_array_alloc_with_length(length as u32));
    {
        // Re-derive after the temp allocation above (which can GC).
        let arr = arr_handle.get_raw_mut_ptr::<ArrayHeader>();
        let recv_elems = (arr as *const u8).add(std::mem::size_of::<ArrayHeader>()) as *const f64;
        for i in 0..length {
            temp.set(i, *recv_elems.add(i));
        }
    }
    rebuild_array_layout(temp.arr());
    let scratch = RootedArrayElems::new(&scope, js_array_alloc_with_length(length as u32));
    sort_comparator_rooted(&temp, &scratch, length, &c, &cmp_handle);

    // The comparator sort completed without a throw — publish the sorted temp
    // back into the receiver (no user code below; both sides re-derived).
    let arr = arr_handle.get_raw_mut_ptr::<ArrayHeader>();
    let recv_elems = (arr as *mut u8).add(std::mem::size_of::<ArrayHeader>()) as *mut f64;
    mark_array_layout_unknown(arr);
    for i in 0..length {
        // GC_STORE_AUDIT(BARRIERED): dense write-back is followed by the rebuild below.
        *recv_elems.add(i) = temp.get(i);
    }
    rebuild_array_layout(arr);

    arr
}
