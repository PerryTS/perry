//! Set representation for Perry
//!
//! Sets are heap-allocated with a stable header pointer.
//! The elements array is separately allocated and can be reallocated
//! without changing the SetHeader address.

use crate::string::StringHeader;
use std::alloc::{alloc, realloc, Layout};
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::ptr;

thread_local! {
    static SET_REGISTRY: RefCell<HashSet<usize>> = RefCell::new(HashSet::new());
}

/// A wrapper around f64 JSValues that implements Hash and Eq using
/// content-based comparison for strings (matching jsvalue_eq semantics).
#[derive(Clone)]
struct JSValueKey(f64);

impl Hash for JSValueKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        let bits = self.0.to_bits();
        let ptr = extract_string_ptr_from_value(bits);
        if !ptr.is_null() && (ptr as usize) >= 0x1000 {
            // String value: hash by content so that identical strings
            // with different pointers/tags produce the same hash.
            unsafe {
                let len = (*ptr).byte_len;
                // Use a distinct domain tag so string hashes don't collide
                // with non-string bit patterns.
                0xFFFF_FFFFu32.hash(state);
                len.hash(state);
                let data = (ptr as *const u8).add(std::mem::size_of::<StringHeader>());
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

/// Side-table mapping set_ptr -> (JSValueKey -> index_in_elements).
/// Provides O(1) lookup for `find_value_index` instead of O(n) linear scan.
thread_local! {
    static SET_INDEX: RefCell<HashMap<usize, HashMap<JSValueKey, u32>>> =
        RefCell::new(HashMap::new());
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

/// Check if a value looks like a heap pointer (raw pointer stored in f64)
fn looks_like_pointer(val: f64) -> bool {
    let bits = val.to_bits();
    let upper_16 = bits >> 48;
    let lower_48 = bits & 0x0000_FFFF_FFFF_FFFF;
    upper_16 == 0 && lower_48 > 0x10000
}

/// Extract pointer from raw f64
fn as_raw_pointer(val: f64) -> *const u8 {
    val.to_bits() as *const u8
}

/// Compare two strings by content
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

fn is_string_like(bits: u64) -> bool {
    !extract_string_ptr_from_value(bits).is_null()
}

/// Check if two JSValues are equal (for set element comparison)
/// Handles STRING_TAG (0x7FFF), POINTER_TAG (0x7FFD), raw pointers, and cross-tag combinations.
fn jsvalue_eq(a: f64, b: f64) -> bool {
    let a_bits = a.to_bits();
    let b_bits = b.to_bits();

    if a_bits == b_bits {
        return true;
    }

    if is_string_like(a_bits) && is_string_like(b_bits) {
        let ptr_a = extract_string_ptr_from_value(a_bits);
        let ptr_b = extract_string_ptr_from_value(b_bits);
        return unsafe { strings_equal(ptr_a, ptr_b) };
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
            idx.borrow_mut().insert(ptr as usize, HashMap::new());
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

/// Convert a Set to an Array (for Array.from(set))
/// Returns a new array containing all elements of the set
#[no_mangle]
pub extern "C" fn js_set_to_array(set: *const SetHeader) -> *mut crate::array::ArrayHeader {
    if set.is_null() {
        return crate::array::js_array_alloc(0);
    }
    unsafe {
        let size = (*set).size as usize;
        let result = crate::array::js_array_alloc(size as u32);
        if size > 0 {
            let elements = (*set).elements as *const f64;
            for i in 0..size {
                let element = ptr::read(elements.add(i));
                crate::array::js_array_push_f64(result, element);
            }
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
    unsafe {
        let len = crate::array::js_array_length(arr);
        for i in 0..len {
            let element = crate::array::js_array_get_f64(arr, i);
            js_set_add(set, element);
        }
    }
    set
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
