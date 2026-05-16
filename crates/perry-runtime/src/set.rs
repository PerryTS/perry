//! Set representation for Perry
//!
//! Sets are heap-allocated with a stable header pointer.
//! The elements array is separately allocated and can be reallocated
//! without changing the SetHeader address.

use crate::fast_hash::{new_ptr_hash_set, PtrHashSet};
use crate::string::StringHeader;
use std::alloc::{alloc, realloc, Layout};
use std::cell::RefCell;
use std::hash::{Hash, Hasher};
use std::ptr;

thread_local! {
    static SET_REGISTRY: RefCell<PtrHashSet<usize>> = RefCell::new(new_ptr_hash_set());
}

/// A wrapper around f64 JSValues that implements Hash and Eq using
/// content-based comparison for strings (matching jsvalue_eq semantics).
#[derive(Clone)]
struct JSValueKey(f64);

impl Hash for JSValueKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        let bits = self.0.to_bits();
        let mut scratch = [0u8; crate::value::SHORT_STRING_MAX_LEN];
        if let Some((data, len)) = string_view_from_bits(bits, &mut scratch) {
            // String value: hash by content so identical strings with
            // different representations (heap STRING_TAG / inline SSO /
            // POINTER_TAG / raw pointer) produce the same hash.
            unsafe {
                // Distinct domain tag so string hashes don't collide
                // with non-string bit patterns.
                0xFFFF_FFFFu32.hash(state);
                len.hash(state);
                let slice = std::slice::from_raw_parts(data, len as usize);
                slice.hash(state);
            }
        } else {
            bits.hash(state);
        }
    }
}

impl PartialEq for JSValueKey {
    fn eq(&self, other: &Self) -> bool {
        jsvalue_eq(self.0, other.0)
    }
}
impl Eq for JSValueKey {}

// Side-table mapping set_ptr -> (JSValueKey -> index_in_elements).
// Provides O(1) lookup for `find_value_index` instead of O(n) linear scan.
//
// Both nesting levels use `PtrHasher` (Fibonacci-multiplicative + xorshift
// avalanche). Outer key is set heap-pointer; inner is `JSValueKey` whose
// `Hash` impl writes either string-content bytes or f64 bits — the
// avalanche step handles both cleanly. Same rationale as MAP_INDEX
// (commit 39e253cd) — the perry-runtime registries don't need
// SipHash's DoS-resistance for keys that never come from external input.
thread_local! {
    static SET_INDEX: RefCell<
        crate::fast_hash::PtrHashMap<usize, crate::fast_hash::PtrHashMap<JSValueKey, u32>>,
    > = RefCell::new(crate::fast_hash::new_ptr_hash_map());
}

fn register_set(ptr: *mut SetHeader) {
    SET_REGISTRY.with(|r| r.borrow_mut().insert(ptr as usize));
}

pub fn is_registered_set(addr: usize) -> bool {
    SET_REGISTRY.with(|r| r.borrow().contains(&addr))
}

/// Set header - stable address, elements allocated separately
#[repr(C)]
pub struct SetHeader {
    /// Number of elements in the set
    pub size: u32,
    /// Capacity (allocated space for elements)
    pub capacity: u32,
    /// Pointer to elements array (separately allocated)
    pub elements: *mut f64,
}

/// Each set element is 8 bytes (f64/JSValue)
const ELEMENT_SIZE: usize = 8;

/// Calculate the layout for an elements array with N elements capacity
fn elements_layout(capacity: usize) -> Layout {
    let elements_size = capacity * ELEMENT_SIZE;
    Layout::from_size_align(elements_size.max(8), 8).unwrap()
}

/// Get pointer to elements array
unsafe fn elements_ptr(set: *const SetHeader) -> *const f64 {
    (*set).elements as *const f64
}

/// Get mutable pointer to elements array
unsafe fn elements_ptr_mut(set: *mut SetHeader) -> *mut f64 {
    (*set).elements
}

/// SameValueZero key normalization: -0 → +0.
/// ECMAScript Sets treat -0 and +0 as the same value (23.2.3.1). Without
/// this, `0` (bits 0x0) and `-0` (bits 0x8000_0000_0000_0000) hash/compare
/// as distinct. Non-number JSValues have NaN-box tags in the upper bits,
/// so `v == 0.0` stays false for them.
#[inline(always)]
fn normalize_zero(value: f64) -> f64 {
    if value == 0.0 {
        0.0
    } else {
        value
    }
}

/// Compare two strings by content
#[cfg(test)]
unsafe fn strings_equal(a: *const StringHeader, b: *const StringHeader) -> bool {
    if a.is_null() || b.is_null() || (a as usize) < 0x1000 || (b as usize) < 0x1000 {
        return a == b;
    }
    // Fast path: same pointer means same string
    if std::ptr::eq(a, b) {
        return true;
    }
    let len_a = (*a).byte_len;
    let len_b = (*b).byte_len;
    if len_a != len_b {
        return false;
    }
    let data_a = (a as *const u8).add(std::mem::size_of::<StringHeader>());
    let data_b = (b as *const u8).add(std::mem::size_of::<StringHeader>());
    // Use slice comparison which leverages SIMD-optimized memcmp
    let slice_a = std::slice::from_raw_parts(data_a, len_a as usize);
    let slice_b = std::slice::from_raw_parts(data_b, len_a as usize);
    slice_a == slice_b
}

/// Extract a string pointer from a value that might be NaN-boxed with various tags.
/// Does NOT handle SHORT_STRING_TAG (SSO) — those don't carry a heap pointer;
/// use `string_view_from_bits` for representation-agnostic content access.
fn extract_string_ptr_from_value(bits: u64) -> *const StringHeader {
    let upper = bits >> 48;
    match upper {
        0x7FFF => (bits & 0x0000_FFFF_FFFF_FFFF) as *const StringHeader, // STRING_TAG
        0x7FFD => (bits & 0x0000_FFFF_FFFF_FFFF) as *const StringHeader, // POINTER_TAG
        0x0000 => {
            let lower = bits & 0x0000_FFFF_FFFF_FFFF;
            if lower > 0x10000 {
                lower as *const StringHeader
            } else {
                std::ptr::null()
            }
        }
        _ => std::ptr::null(),
    }
}

/// Return a `(ptr, byte_len)` view for any string-like JSValue:
/// - `STRING_TAG` heap strings, `POINTER_TAG`, and raw pointers point at a
///   `StringHeader` and yield its inline data.
/// - `SHORT_STRING_TAG` (SSO) inline values decode their length+bytes into
///   `scratch` and return a pointer into it.
///
/// Returns `None` for non-string values. Callers must keep `scratch` alive
/// for the lifetime of the returned slice in the SSO case.
///
/// Issue #434: pre-fix, `JSValueKey::hash` and `jsvalue_eq` only recognized
/// heap-pointer string representations, so `Set.has(JSON.parse('"input"'))`
/// missed the `"input"` literal stored as STRING_TAG.
fn string_view_from_bits<'a>(
    bits: u64,
    scratch: &'a mut [u8; crate::value::SHORT_STRING_MAX_LEN],
) -> Option<(*const u8, u32)> {
    let upper = bits >> 48;
    if upper == (crate::value::SHORT_STRING_TAG >> 48) {
        let len = ((bits & crate::value::SHORT_STRING_LEN_MASK)
            >> crate::value::SHORT_STRING_LEN_SHIFT) as usize;
        let data = bits & crate::value::SHORT_STRING_DATA_MASK;
        for (i, slot) in scratch.iter_mut().enumerate().take(len) {
            *slot = ((data >> (i * 8)) & 0xFF) as u8;
        }
        return Some((scratch.as_ptr(), len as u32));
    }
    let ptr = extract_string_ptr_from_value(bits);
    if ptr.is_null() || (ptr as usize) < 0x1000 {
        return None;
    }
    unsafe {
        let len = (*ptr).byte_len;
        let data = (ptr as *const u8).add(std::mem::size_of::<StringHeader>());
        Some((data, len))
    }
}

fn is_string_like(bits: u64) -> bool {
    let upper = bits >> 48;
    if upper == (crate::value::SHORT_STRING_TAG >> 48) {
        return true;
    }
    // STRING_TAG always identifies a string pointee — accept without GC check.
    if upper == 0x7FFF {
        return !extract_string_ptr_from_value(bits).is_null();
    }
    // Issue #549: POINTER_TAG and raw pointers must be GC-validated as strings
    // before we treat them as string-like. Pre-fix, two distinct `{}` objects
    // both got POINTER_TAG, both passed `is_string_like`, and `jsvalue_eq`
    // content-compared them by reinterpreting `ObjectHeader` as `StringHeader`
    // (class_id became byte_len, etc.) — colliding empty objects in `Set.add`.
    let ptr = extract_string_ptr_from_value(bits);
    if ptr.is_null() || (ptr as usize) < 0x1000 {
        return false;
    }
    unsafe {
        let gc_hdr =
            (ptr as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
        (*gc_hdr).obj_type == crate::gc::GC_TYPE_STRING
    }
}

/// Check if two JSValues are equal (for set element comparison).
/// Handles STRING_TAG (0x7FFF), POINTER_TAG (0x7FFD), SHORT_STRING_TAG (0x7FF9 SSO),
/// raw pointers, and cross-tag combinations.
fn jsvalue_eq(a: f64, b: f64) -> bool {
    let a_bits = a.to_bits();
    let b_bits = b.to_bits();

    if a_bits == b_bits {
        return true;
    }

    if is_string_like(a_bits) && is_string_like(b_bits) {
        let mut a_scratch = [0u8; crate::value::SHORT_STRING_MAX_LEN];
        let mut b_scratch = [0u8; crate::value::SHORT_STRING_MAX_LEN];
        if let (Some((a_ptr, a_len)), Some((b_ptr, b_len))) = (
            string_view_from_bits(a_bits, &mut a_scratch),
            string_view_from_bits(b_bits, &mut b_scratch),
        ) {
            if a_len != b_len {
                return false;
            }
            if a_len == 0 {
                return true;
            }
            unsafe {
                let a_slice = std::slice::from_raw_parts(a_ptr, a_len as usize);
                let b_slice = std::slice::from_raw_parts(b_ptr, b_len as usize);
                return a_slice == b_slice;
            }
        }
    }

    false
}

/// Find the index of a value in the set, or -1 if not found.
/// Uses the O(1) hash index side-table.
unsafe fn find_value_index(set: *const SetHeader, value: f64) -> i32 {
    SET_INDEX.with(|idx| {
        let idx = idx.borrow();
        if let Some(map) = idx.get(&(set as usize)) {
            if let Some(&index) = map.get(&JSValueKey(value)) {
                if index < (*set).size {
                    return index as i32;
                }
            }
        }
        -1
    })
}

/// Grow the elements array if needed (header stays at same address)
unsafe fn ensure_capacity(set: *mut SetHeader) {
    let size = (*set).size;
    let capacity = (*set).capacity;

    if size < capacity {
        return;
    }

    // Double the capacity
    let new_capacity = capacity * 2;
    let old_layout = elements_layout(capacity as usize);
    let new_layout = elements_layout(new_capacity as usize);

    let new_elements =
        realloc((*set).elements as *mut u8, old_layout, new_layout.size()) as *mut f64;
    if new_elements.is_null() {
        panic!("Failed to grow set elements");
    }

    (*set).elements = new_elements;
    (*set).capacity = new_capacity;
}

/// Allocate a new empty set with the given initial capacity
#[no_mangle]
pub extern "C" fn js_set_alloc(capacity: u32) -> *mut SetHeader {
    let cap = if capacity == 0 { 4 } else { capacity };
    let header_layout = Layout::new::<SetHeader>();
    let elem_layout = elements_layout(cap as usize);
    unsafe {
        let ptr = alloc(header_layout) as *mut SetHeader;
        if ptr.is_null() {
            panic!("Failed to allocate set header");
        }
        let elements = alloc(elem_layout) as *mut f64;
        if elements.is_null() {
            panic!("Failed to allocate set elements");
        }

        // Initialize header
        (*ptr).size = 0;
        (*ptr).capacity = cap;
        (*ptr).elements = elements;

        // Register in set registry for runtime type detection
        register_set(ptr);

        // Initialize O(1) lookup index
        SET_INDEX.with(|idx| {
            idx.borrow_mut()
                .insert(ptr as usize, crate::fast_hash::new_ptr_hash_map());
        });

        ptr
    }
}

/// Clean a set pointer that might have NaN-box tag bits
#[inline(always)]
fn clean_set_ptr(set: *const SetHeader) -> *const SetHeader {
    let bits = set as u64;
    let top16 = bits >> 48;
    if top16 >= 0x7FF8 {
        if top16 == 0x7FFC || (bits & 0x0000_FFFF_FFFF_FFFF) == 0 {
            return std::ptr::null();
        }
        (bits & 0x0000_FFFF_FFFF_FFFF) as *const SetHeader
    } else {
        set
    }
}

/// Get the number of elements in the set
#[no_mangle]
pub extern "C" fn js_set_size(set: *const SetHeader) -> u32 {
    let set = clean_set_ptr(set);
    if set.is_null() {
        return 0;
    }
    unsafe { (*set).size }
}

/// Add a value to the set
/// Returns the set pointer (always the same, stable address)
#[no_mangle]
pub extern "C" fn js_set_add(set: *mut SetHeader, value: f64) -> *mut SetHeader {
    let value = normalize_zero(value);
    unsafe {
        // Check if value already exists
        let idx = find_value_index(set, value);

        if idx >= 0 {
            // Value already exists, nothing to do
            return set;
        }

        // Value doesn't exist, need to add it
        ensure_capacity(set);
        let size = (*set).size;
        let elements = elements_ptr_mut(set);

        // Write the value
        ptr::write(elements.add(size as usize), value);

        // Update the hash index
        SET_INDEX.with(|idx| {
            let mut idx = idx.borrow_mut();
            if let Some(map) = idx.get_mut(&(set as usize)) {
                map.insert(JSValueKey(value), size);
            }
        });

        (*set).size = size + 1;
        set
    }
}

/// Check if the set has a value
/// Returns 1 if found, 0 if not found
#[no_mangle]
pub extern "C" fn js_set_has(set: *const SetHeader, value: f64) -> i32 {
    let value = normalize_zero(value);
    unsafe {
        if find_value_index(set, value) >= 0 {
            1
        } else {
            0
        }
    }
}

/// Delete a value from the set
/// Returns 1 if deleted, 0 if value not found
#[no_mangle]
pub extern "C" fn js_set_delete(set: *mut SetHeader, value: f64) -> i32 {
    let value = normalize_zero(value);
    unsafe {
        let idx = find_value_index(set, value);

        if idx < 0 {
            return 0;
        }

        let size = (*set).size;
        let elements = elements_ptr_mut(set);

        // Update the hash index: remove the deleted value,
        // and if we swap-remove, update the swapped element's index.
        SET_INDEX.with(|sidx| {
            let mut sidx = sidx.borrow_mut();
            if let Some(map) = sidx.get_mut(&(set as usize)) {
                map.remove(&JSValueKey(value));
                if (idx as u32) < size - 1 {
                    let last_value = ptr::read(elements.add((size - 1) as usize));
                    // Update the last element's index to the position of the deleted element
                    if let Some(entry) = map.get_mut(&JSValueKey(last_value)) {
                        *entry = idx as u32;
                    }
                }
            }
        });

        // If not the last element, swap with the last element
        if (idx as u32) < size - 1 {
            let last_value = ptr::read(elements.add((size - 1) as usize));
            ptr::write(elements.add(idx as usize), last_value);
        }

        (*set).size = size - 1;
        1
    }
}

/// Clear all elements from the set
#[no_mangle]
pub extern "C" fn js_set_clear(set: *mut SetHeader) {
    unsafe {
        (*set).size = 0;
    }
    SET_INDEX.with(|idx| {
        let mut idx = idx.borrow_mut();
        if let Some(map) = idx.get_mut(&(set as usize)) {
            map.clear();
        }
    });
}

/// Direct read of the i-th element in insertion order — counterpart to
/// `js_map_entry_value_at`. Lets the codegen iterate `for (const x of set)`
/// without materializing the elements buffer into a transient Array via
/// `js_set_to_array`. Returns `undefined` (NaN-box) for out-of-range / null.
#[no_mangle]
pub extern "C" fn js_set_value_at(set: *const SetHeader, i: u32) -> f64 {
    const UNDEF: u64 = 0x7FFC_0000_0000_0001;
    let set = clean_set_ptr(set);
    if set.is_null() {
        return f64::from_bits(UNDEF);
    }
    unsafe {
        if i >= (*set).size {
            return f64::from_bits(UNDEF);
        }
        let elements = (*set).elements as *const f64;
        ptr::read(elements.add(i as usize))
    }
}

/// Convert a Set to an Array (for Array.from(set))
/// Returns a new array containing all elements of the set.
///
/// Bulk path: for non-empty sets we pre-allocate an array with capacity
/// = set size and `memcpy` the f64s directly, then set `length = size`
/// in one shot. The previous loop did N `js_array_push_f64` calls
/// (each chasing a forwarding pointer + checking capacity + bumping
/// length); on a 1.25M-element set the per-push overhead added ~70 ms
/// to `[...set]` (vs Bun's 2 ms). The bulk copy is correctness-safe
/// because we own the freshly-allocated destination — no aliasing,
/// no concurrent modification, capacity is exact.
#[no_mangle]
pub extern "C" fn js_set_to_array(set: *const SetHeader) -> *mut crate::array::ArrayHeader {
    if set.is_null() {
        return crate::array::js_array_alloc(0);
    }
    unsafe {
        let size = (*set).size as usize;
        let result = crate::array::js_array_alloc(size as u32);
        if size > 0 {
            let src = (*set).elements as *const f64;
            let dst = (result as *mut u8).add(std::mem::size_of::<crate::array::ArrayHeader>())
                as *mut f64;
            ptr::copy_nonoverlapping(src, dst, size);
            (*result).length = size as u32;
        }
        result
    }
}

/// Create a Set from an Array (for `new Set(array)`)
/// Takes an ArrayHeader pointer and adds all elements to a new Set
#[no_mangle]
pub extern "C" fn js_set_from_array(arr: *const crate::array::ArrayHeader) -> *mut SetHeader {
    let set = js_set_alloc(4);
    if arr.is_null() {
        return set;
    }
    let len = crate::array::js_array_length(arr);
    for i in 0..len {
        let element = crate::array::js_array_get_f64(arr, i);
        js_set_add(set, element);
    }
    set
}

/// Create a Set from any iterable JS value (for `new Set(iter)`).
///
/// Takes a NaN-boxed value and dispatches by tag:
/// - Array (POINTER_TAG over ArrayHeader) → iterate elements (delegates
///   to `js_set_from_array`)
/// - String (STRING_TAG or SHORT_STRING_TAG) → iterate codepoints,
///   adding each as a single-char string to the set (matches the JS
///   spec: `new Set("abc")` → `{"a", "b", "c"}`)
/// - undefined / null → empty set (matches `new Set()` and `new Set(null)`)
/// - anything else → empty set with no error (lenient — JS would throw
///   `TypeError`, but blocking compilation for non-iterable inputs is
///   worse than silently producing an empty set; followup as needed)
///
/// Used by codegen for `Expr::SetNewFromArray` so a single call site
/// handles any iterable input shape — closes #421-related bug where
/// `new Set("abc")` (hono's `regExpMetaChars` pattern) crashed because
/// `js_set_from_array` blindly cast the StringHeader to ArrayHeader.
#[no_mangle]
pub extern "C" fn js_set_from_iterable(value: f64) -> *mut SetHeader {
    let bits = value.to_bits();
    let top16 = (bits >> 48) as u16;
    // String literals (heap or SSO).
    if top16 == 0x7FFF || top16 == 0x7FF9 {
        let set = js_set_alloc(4);
        // Materialize SSO to a real heap StringHeader; cheap for heap strings.
        let str_ptr = {
            crate::value::js_get_string_pointer_unified(value) as *const crate::string::StringHeader
        };
        if str_ptr.is_null() {
            return set;
        }
        unsafe {
            let byte_len = (*str_ptr).byte_len as usize;
            let data =
                (str_ptr as *const u8).add(std::mem::size_of::<crate::string::StringHeader>());
            let bytes = std::slice::from_raw_parts(data, byte_len);
            // UTF-8 codepoint iteration. Each codepoint becomes a single-
            // codepoint string allocated via `js_string_from_bytes`, then
            // added to the set as the NaN-boxed string handle.
            let mut i = 0;
            while i < bytes.len() {
                let lead = bytes[i];
                let codepoint_len = if lead < 0x80 {
                    1
                } else if lead < 0xC0 {
                    // Continuation byte mid-sequence — shouldn't happen for
                    // well-formed UTF-8; skip one byte to avoid infinite loop.
                    1
                } else if lead < 0xE0 {
                    2
                } else if lead < 0xF0 {
                    3
                } else {
                    4
                };
                let end = (i + codepoint_len).min(bytes.len());
                let cp_bytes = &bytes[i..end];
                let cp_str =
                    crate::string::js_string_from_bytes(cp_bytes.as_ptr(), cp_bytes.len() as u32);
                let cp_value =
                    f64::from_bits(0x7FFF_0000_0000_0000 | (cp_str as u64 & 0x0000_FFFF_FFFF_FFFF));
                js_set_add(set, cp_value);
                i = end;
            }
        }
        return set;
    }
    // Pointer tag covers arrays + objects; we treat it as array (existing
    // path). Non-array objects produce an empty-ish set since
    // `js_array_length` reads bytes that happen to be at offset 0.
    if top16 == 0x7FFD || (bits & 0xFFFF_0000_0000_0000) == 0 {
        let arr_ptr = (bits & 0x0000_FFFF_FFFF_FFFF) as *const crate::array::ArrayHeader;
        return js_set_from_array(arr_ptr);
    }
    // undefined / null / number / etc. → empty set.
    js_set_alloc(4)
}

/// Iterate over set elements, calling a callback with (value, value, set) for each
/// Matches JS Set.forEach signature where key===value (so we pass value twice).
#[no_mangle]
pub extern "C" fn js_set_foreach(set: *const SetHeader, callback: f64) {
    let set = clean_set_ptr(set);
    if set.is_null() {
        return;
    }
    unsafe {
        let size = (*set).size as usize;
        if size == 0 {
            return;
        }
        let elements = elements_ptr(set);

        // Extract the closure pointer from the NaN-boxed callback.
        // Mask off the upper 16 bits (NaN-box tag) to get the real pointer.
        let closure_ptr =
            (callback.to_bits() & 0x0000_FFFF_FFFF_FFFF) as *const crate::closure::ClosureHeader;

        for i in 0..size {
            let value = ptr::read(elements.add(i));
            // Call closure with (value, value) - Set.forEach callback gets (value, value) in JS
            crate::closure::js_closure_call2(closure_ptr, value, value);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::string::js_string_from_bytes;

    #[test]
    fn test_set_add_and_has() {
        let set = js_set_alloc(4);
        js_set_add(set, 1.0);
        js_set_add(set, 2.0);
        js_set_add(set, 3.0);

        assert_eq!(js_set_has(set, 1.0), 1);
        assert_eq!(js_set_has(set, 2.0), 1);
        assert_eq!(js_set_has(set, 3.0), 1);
        assert_eq!(js_set_has(set, 4.0), 0);
        assert_eq!(js_set_has(set, 0.0), 0);
    }

    #[test]
    fn test_set_add_duplicate() {
        let set = js_set_alloc(4);
        js_set_add(set, 42.0);
        js_set_add(set, 42.0);
        js_set_add(set, 42.0);

        assert_eq!(js_set_size(set), 1);
    }

    #[test]
    fn test_set_delete() {
        let set = js_set_alloc(4);
        js_set_add(set, 1.0);
        js_set_add(set, 2.0);
        js_set_add(set, 3.0);

        // Delete existing value
        assert_eq!(js_set_delete(set, 2.0), 1);
        assert_eq!(js_set_size(set), 2);
        assert_eq!(js_set_has(set, 2.0), 0);

        // Other values still present
        assert_eq!(js_set_has(set, 1.0), 1);
        assert_eq!(js_set_has(set, 3.0), 1);

        // Delete non-existing value
        assert_eq!(js_set_delete(set, 99.0), 0);
        assert_eq!(js_set_size(set), 2);
    }

    #[test]
    fn test_set_clear() {
        let set = js_set_alloc(4);
        js_set_add(set, 1.0);
        js_set_add(set, 2.0);
        js_set_add(set, 3.0);

        js_set_clear(set);
        assert_eq!(js_set_size(set), 0);
        assert_eq!(js_set_has(set, 1.0), 0);
        assert_eq!(js_set_has(set, 2.0), 0);
        assert_eq!(js_set_has(set, 3.0), 0);
    }

    #[test]
    fn test_set_size_tracking() {
        let set = js_set_alloc(4);
        assert_eq!(js_set_size(set), 0);

        js_set_add(set, 1.0);
        assert_eq!(js_set_size(set), 1);

        js_set_add(set, 2.0);
        assert_eq!(js_set_size(set), 2);

        // Duplicate doesn't increase size
        js_set_add(set, 1.0);
        assert_eq!(js_set_size(set), 2);

        js_set_delete(set, 1.0);
        assert_eq!(js_set_size(set), 1);
    }

    #[test]
    fn test_set_grow_beyond_initial_capacity() {
        let set = js_set_alloc(2);
        // Add more elements than initial capacity
        for i in 0..20 {
            js_set_add(set, i as f64);
        }

        assert_eq!(js_set_size(set), 20);
        for i in 0..20 {
            assert_eq!(js_set_has(set, i as f64), 1, "should contain {}", i);
        }
        assert_eq!(js_set_has(set, 20.0), 0);
    }

    #[test]
    fn test_set_string_values() {
        // Create two string headers with identical content at different addresses
        let s1 = js_string_from_bytes(b"hello".as_ptr(), 5);
        let s2 = js_string_from_bytes(b"hello".as_ptr(), 5);

        // Verify they are at different addresses
        assert_ne!(s1 as usize, s2 as usize);

        // NaN-box with STRING_TAG (0x7FFF)
        let val1 = f64::from_bits(0x7FFF_0000_0000_0000 | (s1 as u64 & 0x0000_FFFF_FFFF_FFFF));
        let val2 = f64::from_bits(0x7FFF_0000_0000_0000 | (s2 as u64 & 0x0000_FFFF_FFFF_FFFF));

        let set = js_set_alloc(4);
        js_set_add(set, val1);

        // Adding string with same content (different pointer) should be duplicate
        js_set_add(set, val2);
        assert_eq!(
            js_set_size(set),
            1,
            "strings with same content should be deduplicated"
        );

        // has() should find by content
        assert_eq!(js_set_has(set, val2), 1);
    }

    #[test]
    fn test_set_mixed_number_values() {
        let set = js_set_alloc(4);

        // Various number values
        js_set_add(set, 0.0);
        js_set_add(set, -1.0);
        js_set_add(set, 3.14);
        js_set_add(set, f64::INFINITY);
        js_set_add(set, f64::NEG_INFINITY);

        assert_eq!(js_set_size(set), 5);
        assert_eq!(js_set_has(set, 0.0), 1);
        assert_eq!(js_set_has(set, -1.0), 1);
        assert_eq!(js_set_has(set, 3.14), 1);
        assert_eq!(js_set_has(set, f64::INFINITY), 1);
        assert_eq!(js_set_has(set, f64::NEG_INFINITY), 1);
    }

    #[test]
    fn test_set_large() {
        let set = js_set_alloc(4);
        let n = 1000;

        for i in 0..n {
            js_set_add(set, i as f64);
        }
        assert_eq!(js_set_size(set), n);

        // Verify all values present
        for i in 0..n {
            assert_eq!(js_set_has(set, i as f64), 1, "should contain {}", i);
        }

        // Values outside range not present
        assert_eq!(js_set_has(set, n as f64), 0);
        assert_eq!(js_set_has(set, -1.0), 0);
    }

    #[test]
    fn test_set_delete_and_re_add() {
        let set = js_set_alloc(4);
        js_set_add(set, 1.0);
        js_set_add(set, 2.0);
        js_set_add(set, 3.0);

        js_set_delete(set, 2.0);
        assert_eq!(js_set_has(set, 2.0), 0);

        // Re-add the deleted value
        js_set_add(set, 2.0);
        assert_eq!(js_set_has(set, 2.0), 1);
        assert_eq!(js_set_size(set), 3);
    }

    #[test]
    fn test_set_to_array_roundtrip() {
        let set = js_set_alloc(4);
        js_set_add(set, 10.0);
        js_set_add(set, 20.0);
        js_set_add(set, 30.0);

        let arr = js_set_to_array(set);
        assert_eq!(crate::array::js_array_length(arr), 3);

        // Verify all values are in the array
        let mut found = [false; 3];
        for i in 0..3 {
            let val = crate::array::js_array_get_f64(arr, i);
            if val == 10.0 {
                found[0] = true;
            }
            if val == 20.0 {
                found[1] = true;
            }
            if val == 30.0 {
                found[2] = true;
            }
        }
        assert!(found.iter().all(|&f| f), "all values should be in array");
    }

    // --- String comparison tests (Phase 0D) ---

    #[test]
    fn test_strings_equal_same_content() {
        let s1 = js_string_from_bytes(b"test".as_ptr(), 4);
        let s2 = js_string_from_bytes(b"test".as_ptr(), 4);
        assert_ne!(s1 as usize, s2 as usize);
        assert!(unsafe { strings_equal(s1, s2) });
    }

    #[test]
    fn test_strings_equal_different_length() {
        let s1 = js_string_from_bytes(b"hello".as_ptr(), 5);
        let s2 = js_string_from_bytes(b"hi".as_ptr(), 2);
        assert!(!unsafe { strings_equal(s1, s2) });
    }

    #[test]
    fn test_strings_equal_same_pointer() {
        let s1 = js_string_from_bytes(b"hello".as_ptr(), 5);
        // Same pointer should be equal
        assert!(unsafe { strings_equal(s1, s1) });
    }

    #[test]
    fn test_strings_equal_empty() {
        let s1 = js_string_from_bytes(std::ptr::null(), 0);
        let s2 = js_string_from_bytes(std::ptr::null(), 0);
        assert!(unsafe { strings_equal(s1, s2) });
    }

    #[test]
    fn test_strings_equal_different_content() {
        let s1 = js_string_from_bytes(b"abc".as_ptr(), 3);
        let s2 = js_string_from_bytes(b"abd".as_ptr(), 3);
        assert!(!unsafe { strings_equal(s1, s2) });
    }

    #[test]
    fn test_jsvalue_eq_numbers() {
        assert!(jsvalue_eq(1.0, 1.0));
        assert!(jsvalue_eq(0.0, 0.0));
        assert!(!jsvalue_eq(1.0, 2.0));
    }

    #[test]
    fn test_jsvalue_eq_cross_tag_strings() {
        let s1 = js_string_from_bytes(b"hello".as_ptr(), 5);
        let s2 = js_string_from_bytes(b"hello".as_ptr(), 5);

        // STRING_TAG
        let val1 = f64::from_bits(0x7FFF_0000_0000_0000 | (s1 as u64 & 0x0000_FFFF_FFFF_FFFF));
        // POINTER_TAG (different tag, same content)
        let val2 = f64::from_bits(0x7FFD_0000_0000_0000 | (s2 as u64 & 0x0000_FFFF_FFFF_FFFF));

        assert!(
            jsvalue_eq(val1, val2),
            "cross-tag strings with same content should be equal"
        );
    }
}
