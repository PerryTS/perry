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

/// Property-key ATOMS: exactly one string object per key text among the
/// program's pooled literals.
///
/// The direct-mapped table above is a CACHE — a collision evicts — so two
/// strings with one text can both be "interned" at different moments, and a
/// key compare can never conclude anything from pointer equality alone. That
/// is why every runtime key match, and the megamorphic read's confirm against
/// the receiver's key list, paid a byte compare: a canonical key list held the
/// string its first grower happened to pass, and a read site holds its
/// module's pooled literal, and the two were different objects with the same
/// bytes.
///
/// An atom is the one string for its text in this agent, for the agent's
/// lifetime. Atoms are minted only from the compiled program's string pools
/// (`js_string_pool_atom`, at module init), so the table is bounded by the
/// program TEXT, never by runtime data, and it can hold its strings strongly
/// without a death prune: every atom is also the value of a registered pool
/// handle, so the table keeps nothing alive that was not already live. The
/// collector rewrites the entries on move through the intern-table root
/// scanner (`scan_intern_table_roots_mut`), exactly like the cache's.
///
/// One funnel consults it: canonical key lists when they write a key
/// (`canonical_keys::Appended::atomized`), so a read site's pooled key and the
/// receiver's shape key are one pointer.
///
/// An atom is NOT an interned string. It is minted by a plain allocation and
/// never carries `GC_FLAG_INTERNED`, and the intern cache neither adopts nor
/// hands out atoms. `GC_FLAG_INTERNED` is an ELIGIBILITY bit: the own-property
/// read lane, the set fast paths, the chain store and the proxy put paths
/// admit only interned keys. Minting atoms as interned strings silently widened
/// every one of those lanes to every pool-literal key, and on Zod the widened
/// read lane MISSES (the key is inherited, not own) at ~330 instructions each:
/// +0.3% instructions, measured, with the atom table itself inert. Identity is
/// the atom's job; eligibility stays exactly what it was.
///
/// The table never decides an answer. A pointer match proves equal text; a
/// pointer MISmatch proves nothing (a list written before its atom existed
/// holds another string), so every consumer still falls back to bytes on a
/// mismatch.
pub(crate) struct AtomTable {
    /// Open addressing, linear probe, power-of-two capacity; `string_ptr == 0`
    /// is an empty slot. Entries are never removed.
    slots: Vec<InternEntry>,
    len: usize,
}

impl AtomTable {
    const fn new() -> Self {
        AtomTable {
            slots: Vec::new(),
            len: 0,
        }
    }

    /// The atom for `bytes` (whose FNV-1a hash is `hash`), if any.
    ///
    /// # Safety
    /// Every tabled pointer is a live string (the collector keeps them so).
    unsafe fn lookup(&self, bytes: &[u8], hash: u64) -> Option<*const StringHeader> {
        self.lookup_from(0, bytes, hash)
    }

    /// [`Self::lookup`] for a string that may itself BE the atom (`from`, or
    /// 0): a tabled pointer equal to it answers without comparing bytes, which
    /// is the common case for a key a list already holds.
    ///
    /// # Safety
    /// As [`Self::lookup`].
    unsafe fn lookup_from(
        &self,
        from: usize,
        bytes: &[u8],
        hash: u64,
    ) -> Option<*const StringHeader> {
        if self.slots.is_empty() {
            return None;
        }
        let mask = self.slots.len() - 1;
        let mut i = (hash as usize) & mask;
        loop {
            let entry = &self.slots[i];
            if entry.string_ptr == 0 {
                return None;
            }
            if entry.hash == hash {
                let existing = entry.string_ptr as *const StringHeader;
                if entry.string_ptr == from {
                    return Some(existing);
                }
                if (*existing).byte_len as usize == bytes.len()
                    && std::slice::from_raw_parts(string_data(existing), bytes.len()) == bytes
                {
                    return Some(existing);
                }
            }
            i = (i + 1) & mask;
        }
    }

    /// Table `atom` under `hash`. The caller has proved no atom exists for
    /// its text. Rust-heap only: never allocates on the GC heap.
    fn insert(&mut self, hash: u64, atom: *const StringHeader) {
        if (self.len + 1) * 4 > self.slots.len() * 3 {
            let cap = (self.slots.len() * 2).max(256);
            let old = std::mem::replace(
                &mut self.slots,
                vec![
                    InternEntry {
                        hash: 0,
                        string_ptr: 0,
                    };
                    cap
                ],
            );
            for entry in old.into_iter().filter(|e| e.string_ptr != 0) {
                self.place(entry);
            }
        }
        self.place(InternEntry {
            hash,
            string_ptr: atom as usize,
        });
        self.len += 1;
    }

    fn place(&mut self, entry: InternEntry) {
        let mask = self.slots.len() - 1;
        let mut i = (entry.hash as usize) & mask;
        while self.slots[i].string_ptr != 0 {
            i = (i + 1) & mask;
        }
        self.slots[i] = entry;
    }
}

crate::perry_thread_local! {
    /// Per agent, like the cache: an atom is a string in THIS agent's heap.
    pub(crate) static ATOMS: std::cell::UnsafeCell<AtomTable> =
        std::cell::UnsafeCell::new(AtomTable::new());
}

/// The atom for `bytes`, without allocating. `None` when no atom exists — or
/// when the agent's table is already torn down.
#[inline]
pub(crate) fn atom_lookup(bytes: &[u8], hash: u64) -> Option<*const StringHeader> {
    ATOMS
        .try_with(|t| unsafe { (*t.get()).lookup(bytes, hash) })
        .ok()
        .flatten()
}

/// The atom with `key`'s text, if one exists. `hash` is the FNV-1a hash of
/// `key`'s bytes (`key_bytes_hash`). Allocation-free, GC-free.
///
/// # Safety
/// `key` is a live heap `StringHeader`.
#[inline]
pub(crate) unsafe fn atom_for_key(
    key: *const StringHeader,
    hash: u64,
) -> Option<*const StringHeader> {
    if key.is_null() || (*key).byte_len > INTERN_MAX_BYTE_LEN {
        return None;
    }
    let bytes = std::slice::from_raw_parts(string_data(key), (*key).byte_len as usize);
    ATOMS
        .try_with(|t| (*t.get()).lookup_from(key as usize, bytes, hash))
        .ok()
        .flatten()
}

/// Mint (or find) the atom for a pooled literal: the one string object this
/// agent uses for that text from now on. Called from `__perry_init_strings_*`
/// in place of `js_string_from_bytes` for pool entries that can be property
/// keys (at most `INTERN_MAX_BYTE_LEN` bytes). `hash` is the pool's
/// precomputed FNV-1a hash of the bytes — the same function as every other
/// key hash here.
///
/// A plain allocation, exactly what the pool minted before atoms existed: the
/// atom is not interned and does not adopt the intern cache's string (see
/// `AtomTable`: identity, never eligibility). Longer literals are not keys
/// worth an atom and take the plain allocation without a table entry.
#[no_mangle]
pub extern "C" fn js_string_pool_atom(
    bytes: *const u8,
    len: u32,
    hash: u64,
    is_wtf8: i32,
) -> *mut StringHeader {
    if len == 0 || len > INTERN_MAX_BYTE_LEN || bytes.is_null() {
        return if is_wtf8 != 0 {
            js_string_from_wtf8_bytes(bytes, len)
        } else {
            js_string_from_bytes(bytes, len)
        };
    }
    let input = unsafe { std::slice::from_raw_parts(bytes, len as usize) };
    if let Some(atom) = atom_lookup(input, hash) {
        return atom as *mut StringHeader;
    }
    // Nothing is held across the allocation: `bytes` is read-only data in the
    // compiled image. Shared (`refcount = 0`) like every pool literal.
    let atom: *const StringHeader = if is_wtf8 != 0 {
        js_string_from_wtf8_bytes(bytes, len)
    } else {
        js_string_from_bytes(bytes, len)
    };
    let _ = ATOMS.try_with(|t| unsafe { (*t.get()).insert(hash, atom) });
    atom as *mut StringHeader
}

/// Test hook: evict `bytes` from the intern CACHE (a collision would).
#[cfg(test)]
pub(crate) fn test_evict_interned(bytes: &[u8]) {
    let hash = crate::object::key_bytes_hash(bytes.as_ptr(), bytes.len());
    with_intern_table(|table| unsafe {
        (*table)[(hash as usize) & INTERN_TABLE_MASK] = InternEntry {
            hash: 0,
            string_ptr: 0,
        };
    });
}

/// Test view: is `p` the atom of its text?
#[cfg(test)]
pub(crate) unsafe fn is_atom_for_test(p: *const StringHeader) -> bool {
    let bytes = std::slice::from_raw_parts(string_data(p), (*p).byte_len as usize);
    let hash = crate::object::key_bytes_hash(bytes.as_ptr(), bytes.len());
    atom_lookup(bytes, hash) == Some(p)
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
    with_intern_table(|table| unsafe {
        for i in 0..INTERN_TABLE_SIZE {
            let entry = &mut (*table)[i];
            visitor.visit_tagged_usize_slot(&mut entry.string_ptr, crate::value::STRING_TAG);
        }
    });
    // The atoms: strong like the cache (every atom is also a registered pool
    // handle's value, so this keeps nothing alive that was not), rewritten on
    // move. A key's hash is its content's, so a move never rehashes.
    let _ = ATOMS.try_with(|t| unsafe {
        for entry in (*t.get()).slots.iter_mut() {
            if entry.string_ptr != 0 {
                visitor.visit_tagged_usize_slot(&mut entry.string_ptr, crate::value::STRING_TAG);
            }
        }
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
