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

/// Per-thread intern table.
///
/// Each thread (main + every `perry/thread` worker) has its own arena, so
/// cached `StringHeader*` pointers MUST be per-thread — a string interned
/// from worker A's arena is a use-after-free / cross-arena pointer when
/// read from worker B. The previous design used a single process-wide
/// `static mut`, which both raced under concurrent allocation and risked
/// handing back foreign-arena pointers.
thread_local! {
    // arm64_32 fix: HEAP-allocate (Box) this ~128KB table instead of inline TLS.
    // Oversized `#[thread_local]` storage overflows the ILP32 TLS layout and its
    // writes corrupt adjacent thread-locals. Boxing keeps only a pointer in TLS.
    pub(crate) static INTERN_TABLE: std::cell::UnsafeCell<Box<[InternEntry]>> =
        std::cell::UnsafeCell::new(
            vec![InternEntry { hash: 0, string_ptr: 0 }; INTERN_TABLE_SIZE].into_boxed_slice(),
        );
}

#[inline]
pub(crate) fn with_intern_table<R>(
    f: impl FnOnce(*mut [InternEntry; INTERN_TABLE_SIZE]) -> R,
) -> R {
    INTERN_TABLE.with(|c| unsafe {
        let boxed = &mut *c.get();
        f(boxed.as_mut_ptr() as *mut [InternEntry; INTERN_TABLE_SIZE])
    })
}

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
        let hit = with_intern_table(|table| {
            let entry = &(*table)[slot];
            if entry.string_ptr != 0 && entry.hash == hash {
                let existing = entry.string_ptr as *const StringHeader;
                if is_valid_string_ptr(existing)
                    && (*existing).byte_len == byte_len
                    && intern_content_equals(key, existing, byte_len)
                {
                    return Some(existing);
                }
            }
            None
        });
        if let Some(existing) = hit {
            return existing;
        }

        // Miss or collision — insert (evict on collision)
        with_intern_table(|table| {
            (*table)[slot] = InternEntry {
                hash,
                string_ptr: key as usize,
            };
        });

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
/// The intern table is `thread_local!` (issue: runtime thread-safety
/// hardening), so this scans the *current* thread's table. Each thread's
/// GC pass calls this from its own scanner registration, which is the
/// correct partitioning — a thread's GC only walks its own arena, and
/// only its own intern entries point into that arena.
pub fn scan_intern_table_roots(mark: &mut dyn FnMut(f64)) {
    let mut visitor = crate::gc::RuntimeRootVisitor::for_copy(mark);
    scan_intern_table_roots_mut(&mut visitor);
}

pub fn scan_intern_table_roots_mut(visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    // Interned strings can be relocated by this collection; pointer-identity
    // verdicts in the store-plan cache would go stale — flush them.
    crate::object::prop_plan::prop_plan_epoch_bump();
    with_intern_table(|table| unsafe {
        for i in 0..INTERN_TABLE_SIZE {
            let entry = &mut (*table)[i];
            visitor.visit_tagged_usize_slot(&mut entry.string_ptr, crate::value::STRING_TAG);
        }
    });
}

#[cfg(test)]
pub(crate) fn test_seed_intern_table_root(string_ptr: usize) {
    with_intern_table(|table| unsafe {
        (*table)[0] = InternEntry {
            hash: 0xC0DEC0DE,
            string_ptr,
        };
    });
}

#[cfg(test)]
pub(crate) fn test_intern_table_root() -> usize {
    with_intern_table(|table| unsafe { (*table)[0].string_ptr })
}

#[cfg(test)]
pub(crate) fn test_clear_intern_table_root() {
    with_intern_table(|table| unsafe {
        (*table)[0] = InternEntry {
            hash: 0,
            string_ptr: 0,
        };
    });
}
