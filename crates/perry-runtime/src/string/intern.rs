//! Property-name string interning: hash table, FFI entry point, GC root
//! scanners, and concat-time helpers used from `concat.rs`.

use super::*;

/// Intern table entry. Each slot holds one interned string.
#[derive(Clone, Copy)]
#[repr(C)]
pub(crate) struct InternEntry {
    pub(crate) hash: u64,         // FNV-1a content hash
    pub(crate) string_ptr: usize, // pointer to StringHeader (0 = empty slot)
}

pub(crate) const INTERN_TABLE_SIZE: usize = 8192;
pub(crate) const INTERN_TABLE_MASK: usize = INTERN_TABLE_SIZE - 1;

/// Maximum byte length for strings eligible for interning.
pub(crate) const INTERN_MAX_BYTE_LEN: u32 = 64;

#[no_mangle]
pub(crate) static mut INTERN_TABLE: [InternEntry; INTERN_TABLE_SIZE] = [InternEntry {
    hash: 0,
    string_ptr: 0,
}; INTERN_TABLE_SIZE];

/// Intern a property-name string. Returns the canonical pointer for
/// the given content. `hash` is the pre-computed FNV-1a hash.
#[no_mangle]
pub extern "C" fn js_string_intern(key: *const StringHeader, hash: u64) -> *const StringHeader {
    if key.is_null() || !is_valid_string_ptr(key) {
        return key;
    }
    unsafe {
        let byte_len = (*key).byte_len;
        if byte_len > INTERN_MAX_BYTE_LEN {
            return key;
        }

        let slot = (hash as usize) & INTERN_TABLE_MASK;
        let entry = &mut INTERN_TABLE[slot];

        if entry.string_ptr != 0 && entry.hash == hash {
            let existing = entry.string_ptr as *const StringHeader;
            if is_valid_string_ptr(existing)
                && (*existing).byte_len == byte_len
                && intern_content_equals(key, existing, byte_len)
            {
                return existing;
            }
        }

        // Miss or collision — insert (evict on collision)
        entry.hash = hash;
        entry.string_ptr = key as usize;

        // Mark as interned in GcHeader
        let gc_header =
            (key as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *mut crate::gc::GcHeader;
        (*gc_header).gc_flags |= crate::gc::GC_FLAG_INTERNED;

        // Force shared — never mutate interned strings in-place
        (*(key as *mut StringHeader)).refcount = 0;

        key
    }
}

/// Byte-level content comparison for intern table lookups.
#[inline(always)]
unsafe fn intern_content_equals(
    a: *const StringHeader,
    b: *const StringHeader,
    byte_len: u32,
) -> bool {
    let data_a = (a as *const u8).add(std::mem::size_of::<StringHeader>());
    let data_b = (b as *const u8).add(std::mem::size_of::<StringHeader>());
    std::slice::from_raw_parts(data_a, byte_len as usize)
        == std::slice::from_raw_parts(data_b, byte_len as usize)
}

/// Compute FNV-1a hash incrementally over concatenated content a||b
/// without allocating the result. Caller guarantees both pointers are
/// valid when their respective lengths are >0.
#[inline(always)]
pub(crate) unsafe fn fnv1a_concat(
    a: *const StringHeader,
    a_len: u32,
    b: *const StringHeader,
    b_len: u32,
) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    if a_len > 0 {
        let data = (a as *const u8).add(std::mem::size_of::<StringHeader>());
        for i in 0..a_len as usize {
            h ^= *data.add(i) as u64;
            h = h.wrapping_mul(0x100000001b3);
        }
    }
    if b_len > 0 {
        let data = (b as *const u8).add(std::mem::size_of::<StringHeader>());
        for i in 0..b_len as usize {
            h ^= *data.add(i) as u64;
            h = h.wrapping_mul(0x100000001b3);
        }
    }
    h
}

/// Check if concat(a, b) matches the content of an existing interned string.
/// Caller guarantees pointers are valid when their respective lengths are >0.
#[inline(always)]
pub(crate) unsafe fn concat_content_matches(
    a: *const StringHeader,
    a_len: u32,
    b: *const StringHeader,
    b_len: u32,
    existing: *const StringHeader,
) -> bool {
    let ex_data = (existing as *const u8).add(std::mem::size_of::<StringHeader>());
    if a_len > 0 {
        let a_data = (a as *const u8).add(std::mem::size_of::<StringHeader>());
        if std::slice::from_raw_parts(a_data, a_len as usize)
            != std::slice::from_raw_parts(ex_data, a_len as usize)
        {
            return false;
        }
    }
    if b_len > 0 {
        let b_data = (b as *const u8).add(std::mem::size_of::<StringHeader>());
        if std::slice::from_raw_parts(b_data, b_len as usize)
            != std::slice::from_raw_parts(ex_data.add(a_len as usize), b_len as usize)
        {
            return false;
        }
    }
    true
}

/// GC root scanner for the intern table.
///
/// #855: walk via `&raw const` + raw pointer indexing to avoid the
/// `static_mut_refs` lint (hard error in Rust 2024). The intern table
/// is thread-local-by-discipline (perry user code is single-threaded),
/// so the unsafe deref is sound.
pub fn scan_intern_table_roots(mark: &mut dyn FnMut(f64)) {
    let mut visitor = crate::gc::RuntimeRootVisitor::for_copy(mark);
    scan_intern_table_roots_mut(&mut visitor);
}

pub fn scan_intern_table_roots_mut(visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    let base: *mut InternEntry = (&raw mut INTERN_TABLE).cast();
    unsafe {
        for i in 0..INTERN_TABLE_SIZE {
            let entry = &mut *base.add(i);
            visitor.visit_tagged_usize_slot(&mut entry.string_ptr, crate::value::STRING_TAG);
        }
    }
}

#[cfg(test)]
pub(crate) fn test_seed_intern_table_root(string_ptr: usize) {
    unsafe {
        INTERN_TABLE[0] = InternEntry {
            hash: 0xC0DEC0DE,
            string_ptr,
        };
    }
}

#[cfg(test)]
pub(crate) fn test_intern_table_root() -> usize {
    unsafe { INTERN_TABLE[0].string_ptr }
}

#[cfg(test)]
pub(crate) fn test_clear_intern_table_root() {
    unsafe {
        INTERN_TABLE[0] = InternEntry {
            hash: 0,
            string_ptr: 0,
        };
    }
}
