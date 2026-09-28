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

// SAFETY: integer fields only; `string_ptr == 0` is the empty slot (#11507).
unsafe impl crate::zeroed_cache::ZeroEmpty for InternEntry {}

pub(crate) const INTERN_TABLE_SIZE: usize = 8192;
pub(crate) const INTERN_TABLE_MASK: usize = INTERN_TABLE_SIZE - 1;

/// Maximum byte length for strings eligible for interning.
pub(crate) const INTERN_MAX_BYTE_LEN: u32 = 64;

// Per-thread intern table.
//
// Each thread (main + every `perry/thread` worker) has its own arena, so
// cached `StringHeader*` pointers MUST be per-thread — a string interned
// from worker A's arena is a use-after-free / cross-arena pointer when
// read from worker B. The previous design used a single process-wide
// `static mut`, which both raced under concurrent allocation and risked
// handing back foreign-arena pointers.
crate::perry_thread_local! {
    // arm64_32 fix: HEAP-allocate this table instead of inline TLS.
    // Oversized `#[thread_local]` storage overflows the ILP32 TLS layout and its
    // writes corrupt adjacent thread-locals. Boxing keeps only a pointer in TLS.
    pub(crate) static INTERN_TABLE: std::cell::UnsafeCell<Box<[InternEntry]>> =
        std::cell::UnsafeCell::new(crate::zeroed_cache::new_zeroed_cache(INTERN_TABLE_SIZE));
}

crate::perry_thread_local! {
    /// Intern-table slots that may hold a string a minor can act on
    /// (`gc/young_log.rs`). A minor-scoped `scan_intern_table_roots_mut`
    /// visits only these instead of all [`INTERN_TABLE_SIZE`] slots — on an
    /// allocation-bound loop the full walk was ~200k instructions per pass,
    /// two passes per copying minor, over a table that was usually empty.
    static INTERN_YOUNG: std::cell::RefCell<crate::gc::young_log::YoungLog<u32>> =
        const { std::cell::RefCell::new(crate::gc::young_log::YoungLog::new()) };
}

const INTERN_YOUNG_LOG_NAME: &str = "string.intern_table";

/// Rule 1 of `gc/young_log.rs`: every writer of a slot calls this BEFORE the
/// slot names `string_ptr`. An old string notes nothing — a minor can neither
/// move nor free it, so visiting it would be a no-op.
#[inline]
fn arm_intern_young(slot: usize, string_ptr: usize) {
    if crate::gc::young_log::addr_is_minor_relevant(string_ptr) {
        INTERN_YOUNG.with(|log| log.borrow_mut().note(slot as u32));
    }
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
        arm_intern_young(slot, key as usize);
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

/// Materialize an immutable AOT dispatch descriptor into the current thread's
/// intern table. The precomputed content hash selects the existing direct-
/// mapped slot without re-hashing; content comparison preserves correctness
/// across hash collisions while keeping the table's per-thread RSS unchanged.
pub(crate) fn intern_dispatch_bytes(
    static_dispatch_id: usize,
    bytes: *const u8,
    byte_len: usize,
    precomputed_hash: u64,
    is_wtf8: bool,
) -> *const StringHeader {
    if (bytes.is_null() && byte_len != 0) || byte_len > u32::MAX as usize {
        return std::ptr::null();
    }
    let input = if byte_len == 0 {
        &[][..]
    } else {
        unsafe { std::slice::from_raw_parts(bytes, byte_len) }
    };
    let hash = if static_dispatch_id != 0 {
        precomputed_hash
    } else {
        let mut hash = 0xcbf2_9ce4_8422_2325u64;
        for &byte in input {
            hash ^= byte as u64;
            hash = hash.wrapping_mul(0x0100_0000_01b3);
        }
        hash
    };
    let slot = (hash as usize) & INTERN_TABLE_MASK;

    let hit = with_intern_table(|table| unsafe {
        let entry = &mut (*table)[slot];
        if entry.string_ptr == 0 {
            return None;
        }
        let existing = entry.string_ptr as *const StringHeader;
        if entry.hash == hash
            && is_valid_string_ptr(existing)
            && (*existing).byte_len as usize == byte_len
            && std::slice::from_raw_parts(
                (existing as *const u8).add(std::mem::size_of::<StringHeader>()),
                byte_len,
            ) == input
        {
            return Some(existing);
        }
        None
    });
    if let Some(existing) = hit {
        return existing;
    }

    let key = if is_wtf8 {
        js_string_from_wtf8_bytes(bytes, byte_len as u32)
    } else {
        js_string_from_bytes(bytes, byte_len as u32)
    };
    unsafe {
        let gc_header =
            (key as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *mut crate::gc::GcHeader;
        (*gc_header).gc_flags |= crate::gc::GC_FLAG_INTERNED;
        (*key).refcount = 0;
    }
    arm_intern_young(slot, key as usize);
    with_intern_table(|table| unsafe {
        (*table)[slot] = InternEntry {
            hash,
            string_ptr: key as usize,
        };
    });
    key
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
    // verdicts in the store-plan cache would go stale — flush them. This is a
    // relocation, NOT a property change, so it bumps only the pointer-identity
    // epoch: a cache that re-derives its addresses every lookup (`#7910`'s
    // `Object.prototype` `then` verdict) must not be flushed at loop-poll
    // cadence by the incremental collector.
    crate::object::prop_plan::prop_plan_gc_epoch_bump();
    // A minor-scoped pass can act only on a slot that names a young string,
    // and every such slot is in `INTERN_YOUNG` (rule 1), so it visits the log
    // instead of the table. A full pass walks the table and rebuilds the log.
    if visitor.young_scope() {
        #[cfg(any(debug_assertions, test))]
        debug_assert_intern_young_logged();
        let mut kept = INTERN_YOUNG.with(|log| log.borrow_mut().take_spare());
        let batch = INTERN_YOUNG.with(|log| log.borrow_mut().take_sorted());
        let logged = batch.len() as u64;
        with_intern_table(|table| unsafe {
            for &slot in &batch {
                let entry = &mut (*table)[slot as usize];
                visitor.visit_tagged_usize_slot(&mut entry.string_ptr, crate::value::STRING_TAG);
                if crate::gc::young_log::addr_is_minor_relevant(entry.string_ptr) {
                    kept.push(slot);
                }
            }
        });
        let kept_len = kept.len() as u64;
        INTERN_YOUNG.with(|log| log.borrow_mut().extend(kept));
        crate::gc::young_log::note_walk(
            INTERN_YOUNG_LOG_NAME,
            crate::gc::young_log::YoungLogWalk {
                partial: true,
                logged,
                visited: logged,
                kept: kept_len,
                table_len: INTERN_TABLE_SIZE as u64,
            },
        );
        return;
    }
    let _ = INTERN_YOUNG.with(|log| log.borrow_mut().take_sorted());
    let mut kept = Vec::new();
    with_intern_table(|table| unsafe {
        for i in 0..INTERN_TABLE_SIZE {
            let entry = &mut (*table)[i];
            visitor.visit_tagged_usize_slot(&mut entry.string_ptr, crate::value::STRING_TAG);
            if crate::gc::young_log::addr_is_minor_relevant(entry.string_ptr) {
                kept.push(i as u32);
            }
        }
    });
    let kept_len = kept.len() as u64;
    INTERN_YOUNG.with(|log| log.borrow_mut().extend(kept));
    crate::gc::young_log::note_walk(
        INTERN_YOUNG_LOG_NAME,
        crate::gc::young_log::YoungLogWalk {
            partial: false,
            logged: INTERN_TABLE_SIZE as u64,
            visited: INTERN_TABLE_SIZE as u64,
            kept: kept_len,
            table_len: INTERN_TABLE_SIZE as u64,
        },
    );
}

/// Rule 2 of `gc/young_log.rs`: re-derive the minor-relevant slots from the
/// table itself and require the log to name every one. A writer that
/// publishes without [`arm_intern_young`] fails here on the next minor instead
/// of leaving a from-space pointer in the table.
#[cfg(any(debug_assertions, test))]
fn debug_assert_intern_young_logged() {
    let relevant: Vec<u32> = with_intern_table(|table| unsafe {
        (0..INTERN_TABLE_SIZE)
            .filter(|&i| crate::gc::young_log::addr_is_minor_relevant((*table)[i].string_ptr))
            .map(|i| i as u32)
            .collect()
    });
    INTERN_YOUNG.with(|log| {
        log.borrow()
            .debug_assert_logged(INTERN_YOUNG_LOG_NAME, &relevant)
    });
}

/// #11507: the table is zero-allocated rather than filled, so a thread's first
/// view of it must be an empty slot everywhere.
#[cfg(test)]
#[test]
fn fresh_thread_intern_table_reads_empty_everywhere() {
    std::thread::spawn(|| {
        with_intern_table(|table| unsafe {
            for entry in (*table).iter() {
                assert_eq!((entry.hash, entry.string_ptr), (0, 0));
            }
        });
    })
    .join()
    .unwrap();
}

#[cfg(test)]
pub(crate) fn test_seed_intern_table_root(string_ptr: usize) {
    arm_intern_young(0, string_ptr);
    with_intern_table(|table| unsafe {
        (*table)[0] = InternEntry {
            hash: 0xC0DEC0DE,
            string_ptr,
        };
    });
}

/// Empty every slot and the young log: a test that asserts on the log's walk
/// must not inherit another test's entries on a reused thread.
#[cfg(test)]
pub(crate) fn test_clear_intern_table() {
    INTERN_YOUNG.with(|log| log.borrow_mut().clear());
    with_intern_table(|table| unsafe {
        for entry in (*table).iter_mut() {
            *entry = InternEntry {
                hash: 0,
                string_ptr: 0,
            };
        }
    });
}

/// The pointer the slot for `hash` currently names (0 when empty).
#[cfg(test)]
pub(crate) fn test_intern_slot_ptr(hash: u64) -> usize {
    with_intern_table(|table| unsafe { (*table)[(hash as usize) & INTERN_TABLE_MASK].string_ptr })
}

/// A writer that forgets rule 1: publish without noting the slot.
#[cfg(test)]
pub(crate) fn test_write_intern_slot_without_logging(slot: usize, string_ptr: usize) {
    with_intern_table(|table| unsafe {
        (*table)[slot & INTERN_TABLE_MASK] = InternEntry {
            hash: 0xC0DE,
            string_ptr,
        };
    });
}

/// Run the rule-2 check the minor-scoped walk runs first.
#[cfg(test)]
pub(crate) fn test_check_intern_young_logged() {
    debug_assert_intern_young_logged();
}

/// Initialise a raw string allocation (e.g. an old-generation one a test
/// allocated directly) with ASCII `bytes`.
///
/// # Safety
/// `ptr` must be a string allocation with room for `bytes`.
#[cfg(test)]
pub(crate) unsafe fn test_init_string_bytes(ptr: *mut StringHeader, bytes: &[u8]) {
    init_string_header(
        ptr,
        bytes.len() as u32,
        bytes.len() as u32,
        bytes.len() as u32,
        0,
        0,
    );
    std::ptr::copy_nonoverlapping(bytes.as_ptr(), string_data(ptr) as *mut u8, bytes.len());
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

/// Read-only intern-table probe: the canonical pointer for `bytes`, or `None`
/// when they are not currently tabled. Never inserts, never allocates, never
/// touches GC state — safe to call with unrooted raw pointers live.
pub(crate) fn intern_lookup_bytes(bytes: &[u8]) -> Option<*const StringHeader> {
    if bytes.is_empty() || bytes.len() > INTERN_MAX_BYTE_LEN as usize {
        return None;
    }
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for &b in bytes {
        hash ^= b as u64;
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    let slot = (hash as usize) & INTERN_TABLE_MASK;
    with_intern_table(|table| unsafe {
        let entry = &(*table)[slot];
        if entry.string_ptr != 0 && entry.hash == hash {
            let existing = entry.string_ptr as *const StringHeader;
            if is_valid_string_ptr(existing)
                && (*existing).byte_len as usize == bytes.len()
                && std::slice::from_raw_parts(super::string_data(existing), bytes.len()) == bytes
            {
                return Some(existing);
            }
        }
        None
    })
}
