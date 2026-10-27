//! Executable literals/dispatch keys are emitted image data, outside these
//! tables. The remaining atom index serves runtime-created static key lists
//! and unloadable output; its pinned Longlived leaves have agent lifetime.
//! Dynamic reuse is weak and discarded before collections or string reallocations. Neither
//! index needs a scanner or young-entry log.

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

// Static dispatch keys now live in their image descriptors. The remaining
// table serves only short dynamic strings; collisions still compare bytes.
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
    static INTERN_TABLE: std::cell::UnsafeCell<Box<[InternEntry]>> =
        std::cell::UnsafeCell::new(crate::zeroed_cache::new_zeroed_cache(INTERN_TABLE_SIZE));
}

/// Residual canonical index for runtime-created static keys and unloadable
/// output. Executable literals are linker-coalesced image objects and do not
/// enter this index. Runtime atoms are pinned Longlived leaves, bounded by
/// static key vocabulary rather than runtime data.
///
/// Image literals are immutable interned keys at birth. Runtime atoms retain
/// the existing learned eligibility, independently of their pinned lifetime.
/// Pointer mismatch always falls back to comparing bytes.
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

    /// First slot of `hash`. FNV-1a ends in a multiply, so its low bits depend
    /// only on the low bits of the input bytes and cluster for ASCII keys
    /// that differ in their last characters; fold the well-mixed high half in.
    #[inline(always)]
    fn home(hash: u64, mask: usize) -> usize {
        ((hash ^ (hash >> 32)) as usize) & mask
    }

    /// The atom for `bytes` (whose FNV-1a hash is `hash`), if any.
    ///
    /// # Safety
    /// Every tabled pointer is an immortal, non-moving string for this agent.
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
        let mut i = Self::home(hash, mask);
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
        if (self.len + 1) * 2 > self.slots.len() {
            self.resize((self.slots.len() * 2).max(256));
        }
        self.place(InternEntry {
            hash,
            string_ptr: atom as usize,
        });
        self.len += 1;
    }

    /// Room for `additional` more atoms without growing on the way.
    fn reserve(&mut self, additional: usize) {
        let cap = ((self.len + additional) * 2).next_power_of_two().max(256);
        if cap > self.slots.len() {
            self.resize(cap);
        }
    }

    fn resize(&mut self, cap: usize) {
        // A filled table, not a zeroed allocation: freed tables of earlier
        // growth are reused already mapped (a zeroed allocation took fresh
        // pages on every growth: +70 faults on fastify_inject).
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

    fn place(&mut self, entry: InternEntry) {
        let mask = self.slots.len() - 1;
        let mut i = Self::home(entry.hash, mask);
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
    if is_image_string(key) {
        return Some(key);
    }
    let bytes = std::slice::from_raw_parts(string_data(key), (*key).byte_len as usize);
    ATOMS
        .try_with(|t| (*t.get()).lookup_from(key as usize, bytes, hash))
        .ok()
        .flatten()
}

/// Size this agent's atom table for `count` static atoms at once: the static
/// seed knows its count up front, while growing to it one doubling at a time
/// rehashed and re-zeroed every intermediate table.
pub(crate) fn reserve_static_atoms(count: usize) {
    let _ = ATOMS.try_with(|t| unsafe { (*t.get()).reserve(count) });
}

/// Object model step 0: make `string` (an immutable image string, a static
/// atom of the program) this agent's atom for `bytes` unless one exists.
///
/// # Safety
/// `string` is an image string whose bytes are `bytes`.
pub(crate) unsafe fn seed_static_atom(bytes: &[u8], hash: u64, string: *const StringHeader) {
    if bytes.is_empty() || bytes.len() > INTERN_MAX_BYTE_LEN as usize {
        return;
    }
    let _ = ATOMS.try_with(|t| {
        let t = &mut *t.get();
        if t.lookup(bytes, hash).is_none() {
            t.insert(hash, string);
        }
    });
}

/// Mint (or find) the atom for a pooled literal: the one string object this
/// agent uses for that text from now on. Called from `__perry_init_strings_*`
/// in place of `js_string_from_bytes` for pool entries that can be property
/// keys (at most `INTERN_MAX_BYTE_LEN` bytes). `hash` is the pool's
/// precomputed FNV-1a hash of the bytes — the same function as every other
/// key hash here.
///
/// An image allocation: the atom is not interned and does not adopt the
/// weak intern cache's string (see
/// `AtomTable`: identity, never eligibility). Longer literals are not keys
/// worth an atom entry use image space without a table entry.
///
/// The minted atom is a program property atom, exactly as a literal image
/// registered by `js_string_register_property_atom` is (§3.5): it carries its
/// creation hash, so `property_atom` answers with it and a data key with the
/// same spelling (JSON.parse, a computed key) meets the program's literal at
/// one address. A tabled string that is not a property atom (legacy layout
/// seeding) is replaced by it, as registration replaces one.
#[no_mangle]
pub extern "C" fn js_string_pool_atom(
    bytes: *const u8,
    len: u32,
    hash: u64,
    is_wtf8: i32,
) -> *mut StringHeader {
    if is_wtf8 != 0 || len == 0 || len > INTERN_MAX_BYTE_LEN || bytes.is_null() {
        return image_string(bytes, len);
    }
    let input = unsafe { std::slice::from_raw_parts(bytes, len as usize) };
    let existing = atom_lookup(input, hash);
    if let Some(atom) = existing {
        if unsafe { (*atom).flags } & PROPERTY_ATOM != 0 {
            return atom as *mut StringHeader;
        }
    }
    // Nothing is held across the allocation: `bytes` is read-only data in the
    // compiled image. Shared (`refcount = 0`) like every pool literal.
    let atom: *const StringHeader = program_atom_image(bytes, len, hash);
    let _ = ATOMS.try_with(|t| unsafe {
        let table = &mut *t.get();
        match existing.and_then(|old| {
            table
                .slots
                .iter_mut()
                .find(|entry| entry.string_ptr == old as usize)
        }) {
            // GC_STORE_AUDIT(ROOT): Weak metadata custody, not a strong root: ATOMS is agent TLS. program_atom_image creates pinned long-lived image string, then replaces only weak metadata address. scan_property_atoms registered gc/mod.rs1028 repairs addresses ONLY in metadata rewrite phase; dead_owner.rs249/274 prunes before sweep.
            Some(entry) => entry.string_ptr = atom as usize,
            None => table.insert(hash, atom),
        }
    });
    atom as *mut StringHeader
}

/// An image string laid out as a property atom: the payload, its NUL, then
/// the creation hash at the next 8-byte boundary (`property_atom_hash`).
fn program_atom_image(bytes: *const u8, len: u32, hash: u64) -> *mut StringHeader {
    let header_size = std::mem::size_of::<StringHeader>();
    let hash_at = (header_size + len as usize + 1 + 7) / 8 * 8;
    let (string, data) = super::string_storage_alloc_longlived((hash_at + 8 - header_size) as u32);
    unsafe {
        let (units, flags) = super::raw_string_metadata(bytes, len);
        super::init_string_header(string, units, len, len, 0, flags);
        (*string).flags |= IMAGE_ATOM_FLAGS;
        std::ptr::copy_nonoverlapping(bytes, data, len as usize);
        // GC_STORE_AUDIT(POINTER_FREE): Writes byte0 string terminator into property atom payload; not a managed slot.
        data.add(len as usize).write(0);
        // GC_STORE_AUDIT(POINTER_FREE): Writes precomputed u64 string hash into trailing payload metadata; hash is consumed as integer, never traced as JSValue.
        std::ptr::write_unaligned(
            ((data.add(len as usize + 1) as usize + 7) & !7) as *mut u64,
            hash,
        );
        let header = (string as *mut u8).sub(crate::gc::GC_HEADER_SIZE) as *mut crate::gc::GcHeader;
        crate::gc::pin_object_non_young(header);
    }
    string
}

/// I2 birth funnel. Longlived is an existing image space: never reset and
/// never selected by old-page defrag or idle compaction. The leaf pin prevents
/// full sweep from invalidating the header, without a root or pin-summary walk.
fn image_string(bytes: *const u8, len: u32) -> *mut StringHeader {
    let string = js_string_from_bytes_longlived(bytes, len);
    unsafe {
        let header = (string as *mut u8).sub(crate::gc::GC_HEADER_SIZE) as *mut crate::gc::GcHeader;
        crate::gc::pin_object_non_young(header);
    }
    string
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

/// Discard weak addresses before any object can move or its storage be reused.
/// Called at collection scope entry and before a tracked string realloc.
/// Unrelated reallocations cannot invalidate a StringHeader address. No entry
/// keeps its string alive, and no heap-change scope may publish an entry.
pub(crate) fn discard_weak_interns() {
    let _ = INTERN_TABLE.try_with(|c| unsafe {
        (*c.get()).fill(InternEntry {
            hash: 0,
            string_ptr: 0,
        });
    });
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

/// A heap change can move or free another object after a callback returns.
/// Never publish an address inside that window. Collection entry and string
/// realloc invalidation make ordinary lookups safe without checking the scope.
#[inline]
fn publish_intern_entry(hash: u64, key: *const StringHeader) {
    if crate::gc::heap_generation::heap_change_open() {
        return;
    }
    with_intern_table(|table| unsafe {
        (*table)[(hash as usize) & INTERN_TABLE_MASK] = InternEntry {
            hash,
            string_ptr: key as usize,
        };
    });
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

        // Executable image strings are shared immutable leaves. Their data
        // header already carries the eligibility appropriate to that site;
        // neither the weak table nor a mutator may write into the image.
        let gc_header =
            (key as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *mut crate::gc::GcHeader;
        if (*gc_header)._reserved == STRING_IMAGE_MARKER {
            return key;
        }

        let slot = (hash as usize) & INTERN_TABLE_MASK;
        let hit = with_intern_table(|table| {
            let entry = &(*table)[slot];
            if entry.string_ptr != 0 && entry.hash == hash {
                let existing = entry.string_ptr as *const StringHeader;
                if existing == key
                    || ((*existing).byte_len == byte_len
                        && intern_content_equals(key, existing, byte_len))
                {
                    return Some(existing);
                }
            }
            None
        });
        if let Some(existing) = hit {
            return existing;
        }

        let bytes = std::slice::from_raw_parts(string_data(key), byte_len as usize);
        if let Some(atom) = atom_lookup(bytes, hash) {
            publish_intern_entry(hash, atom);
            return atom;
        }

        // Miss or collision — insert (evict on collision)
        publish_intern_entry(hash, key);

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
    intern_prehashed_bytes(bytes, byte_len, hash, is_wtf8)
}

/// One content probe with a hash already supplied by codegen or a caller.
/// Separating hashing from this body lets runtime literal callers fold FNV at
/// their call site, while using the existing weak intern table.
pub(super) fn intern_prehashed_bytes(
    bytes: *const u8,
    byte_len: usize,
    hash: u64,
    is_wtf8: bool,
) -> *const StringHeader {
    let input = if byte_len == 0 {
        &[][..]
    } else {
        unsafe { std::slice::from_raw_parts(bytes, byte_len) }
    };
    let slot = (hash as usize) & INTERN_TABLE_MASK;

    let hit = with_intern_table(|table| unsafe {
        let entry = &mut (*table)[slot];
        if entry.string_ptr == 0 {
            return None;
        }
        let existing = entry.string_ptr as *const StringHeader;
        if entry.hash == hash
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

    if let Some(atom) = atom_lookup(input, hash) {
        publish_intern_entry(hash, atom);
        return atom;
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
    publish_intern_entry(hash, key);
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

/// Empty every slot so a test does not inherit another test's weak entries
/// on a reused thread.
#[cfg(test)]
pub(crate) fn test_clear_intern_table() {
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

/// Read-only intern-table probe: the canonical pointer for `bytes`, or `None`
/// when they are not currently tabled. Never inserts, never allocates, never
/// touches GC state — safe to call with unrooted raw pointers live.
pub(crate) fn intern_lookup_bytes(bytes: &[u8]) -> Option<*const StringHeader> {
    if bytes.is_empty() || bytes.len() > INTERN_MAX_BYTE_LEN as usize {
        return None;
    }
    let hash = crate::object::key_bytes_hash(bytes.as_ptr(), bytes.len());
    let slot = (hash as usize) & INTERN_TABLE_MASK;
    with_intern_table(|table| unsafe {
        let entry = &(*table)[slot];
        if entry.string_ptr != 0 && entry.hash == hash {
            let existing = entry.string_ptr as *const StringHeader;
            if (*existing).byte_len as usize == bytes.len()
                && std::slice::from_raw_parts(super::string_data(existing), bytes.len()) == bytes
            {
                return Some(existing);
            }
        }
        None
    })
}

// Property atoms retain their creation hash after the byte payload. Plain
// strings carry no hash and keep the existing string ABI and allocation size.
const PROPERTY_ATOM: u32 = 1 << 3;
const PROGRAM_ATOM: u32 = 1 << 4;
/// The atom flags of every image string (codegen's literal images and the
/// runtime's layout key images, which share their symbols).
pub(crate) const IMAGE_ATOM_FLAGS: u32 = PROPERTY_ATOM | PROGRAM_ATOM;

/// Is `key` a property atom (the one atom of its text in this agent, its
/// creation hash stored after the payload)?
#[inline(always)]
pub(crate) unsafe fn is_property_atom(key: *const StringHeader) -> bool {
    (*key).flags & PROPERTY_ATOM != 0
}

/// Object model step 5 (case 2c): a coloured program atom carries its colour
/// in `flags` bits 16..24 (mask bit + 1; 0 = no colour) and 24..32 (the
/// colour slot), written once per agent by [`set_atom_colour`] before user
/// code runs. A record's colour masks are then one flags load per key at mint
/// (`object/layout_probe.rs`), not a table lookup.
const ATOM_COLOUR_SHIFT: u32 = 16;

/// The colour `(mask bit, slot)` of program atom `key`, if it has one.
///
/// # Safety
/// `key` is a live `StringHeader`.
#[inline(always)]
pub(crate) unsafe fn atom_colour(key: *const StringHeader) -> Option<(u32, u32)> {
    let f = (*key).flags;
    if f & IMAGE_ATOM_FLAGS != IMAGE_ATOM_FLAGS {
        return None;
    }
    let c = (f >> ATOM_COLOUR_SHIFT) & 0xFF;
    (c != 0).then(|| (c - 1, f >> (ATOM_COLOUR_SHIFT + 8)))
}

/// Give this agent's atom of `bytes` colour `(bit, slot)`; returns that atom.
/// A colour that does not fit (`bit >= 127`, `slot >= 256`) stays unset, so
/// those keys never enter a mask.
pub(crate) unsafe fn set_atom_colour(
    bytes: &[u8],
    hash: u64,
    bit: u32,
    slot: u32,
) -> Option<*const StringHeader> {
    if bit >= 127 || slot >= 256 {
        return None;
    }
    let a = atom_lookup(bytes, hash)?;
    if !is_program_atom(a) {
        return None;
    }
    // An image atom is immutable data that codegen already coloured; only an
    // atom the runtime allocated for the text is written here.
    if atom_colour(a) != Some((bit, slot)) {
        if super::is_image_string(a) {
            return None;
        }
        let s = a as *mut StringHeader;
        (*s).flags = ((*s).flags & 0xFFFF)
            | ((bit + 1) << ATOM_COLOUR_SHIFT)
            | (slot << (ATOM_COLOUR_SHIFT + 8));
    }
    Some(a)
}

/// Is `key` a program atom (a property atom the compiled image or a pool
/// literal owns: immortal, the one atom of its text)?
#[inline(always)]
pub(crate) unsafe fn is_program_atom(key: *const StringHeader) -> bool {
    (*key).flags & IMAGE_ATOM_FLAGS == IMAGE_ATOM_FLAGS
}

pub(crate) unsafe fn property_atom_hash(key: *const StringHeader) -> u64 {
    debug_assert!((*key).flags & PROPERTY_ATOM != 0);
    std::ptr::read_unaligned(
        (((string_data(key).add((*key).byte_len as usize + 1) as usize) + 7) & !7) as *const u64,
    )
}

/// The atom of `key`'s spelling if one exists, without allocating or
/// inserting: a property atom answers from its stored hash and identity, a
/// plain string by hashing its bytes. `None` also when the only tabled string
/// is not a property atom: no object's ordered keys can then hold the spelling.
pub(crate) unsafe fn existing_property_atom(
    key: *const StringHeader,
) -> Option<*const StringHeader> {
    let bytes = std::slice::from_raw_parts(string_data(key), (*key).byte_len as usize);
    let existing = if (*key).flags & PROPERTY_ATOM != 0 {
        let hash = property_atom_hash(key);
        ATOMS
            .try_with(|t| (*t.get()).lookup_from(key as usize, bytes, hash))
            .ok()
            .flatten()?
    } else {
        atom_lookup(
            bytes,
            crate::object::key_bytes_hash(bytes.as_ptr(), bytes.len()),
        )?
    };
    ((*existing).flags & PROPERTY_ATOM != 0).then_some(existing)
}

pub(crate) unsafe fn property_atom(key: *const StringHeader) -> *const StringHeader {
    if (*key).flags & PROPERTY_ATOM != 0 {
        let hash = property_atom_hash(key);
        let bytes = std::slice::from_raw_parts(string_data(key), (*key).byte_len as usize);
        return ATOMS.with(|t| {
            let table = &mut *t.get();
            if let Some(existing) = table.lookup_from(key as usize, bytes, hash) {
                if (*existing).flags & PROPERTY_ATOM != 0 {
                    return existing;
                }
                if let Some(entry) = table
                    .slots
                    .iter_mut()
                    .find(|e| e.string_ptr == existing as usize)
                {
                    // GC_STORE_AUDIT(ROOT): Weak metadata custody, not a strong root: ATOMS stores canonical PROPERTY_ATOM key as weak metadata, not a retention edge. gc/mod.rs1028 registers metadata-only address repair; dead_owner.rs249/274 prunes dead owners. Agent TLS bounds lifetime.
                    entry.string_ptr = key as usize;
                    return key;
                }
            }
            table.insert(hash, key);
            key
        });
    }
    let bytes = crate::string::OwnedStringBytes::copy_from_header(key);
    let hash = crate::object::key_bytes_hash(bytes.as_bytes().as_ptr(), bytes.as_bytes().len());
    if let Some(atom) = atom_lookup(bytes.as_bytes(), hash) {
        if (*atom).flags & PROPERTY_ATOM != 0 {
            return atom;
        }
    }
    let units = (*key).utf16_len;
    let flags = (*key).flags;
    let len = bytes.as_bytes().len();
    let atom = crate::arena::arena_alloc_gc(
        (std::mem::size_of::<StringHeader>() + len + 1 + 7) / 8 * 8 + 8,
        8,
        crate::gc::GC_TYPE_STRING,
    ) as *mut StringHeader;
    // GC_STORE_AUDIT(POINTER_FREE): StringHeader contains only utf16_len,byte_len,capacity,refcount,flags scalars; newly allocated atom header has no child pointer fields.
    atom.write(StringHeader {
        utf16_len: units,
        byte_len: len as u32,
        capacity: len as u32,
        refcount: 0,
        flags: flags | PROPERTY_ATOM,
    });
    std::ptr::copy_nonoverlapping(bytes.as_bytes().as_ptr(), string_data(atom) as *mut u8, len);
    // GC_STORE_AUDIT(POINTER_FREE): Writes byte0 string terminator into property atom payload; not a managed slot.
    (string_data(atom) as *mut u8).add(len).write(0);
    // GC_STORE_AUDIT(POINTER_FREE): Writes u64 key hash into trailing payload metadata; scalar data never traced as JSValue.
    std::ptr::write_unaligned(
        (((string_data(atom).add(len + 1) as usize) + 7) & !7) as *mut u64,
        hash,
    );
    let _ = ATOMS.try_with(|t| {
        let table = &mut *t.get();
        if let Some(existing) = table.lookup(bytes.as_bytes(), hash) {
            if let Some(entry) = table
                .slots
                .iter_mut()
                .find(|entry| entry.string_ptr == existing as usize)
            {
                // GC_STORE_AUDIT(ROOT): Weak metadata custody, not a strong root: Fresh atom replaces equivalent plain-string entry in agent-local weak ATOMS. Repair occurs only after trace and dead entries are pruned before sweep (intern.rs893-920, gc/mod.rs1028, dead_owner.rs249/274).
                entry.string_ptr = atom as usize;
                return;
            }
        }
        table.insert(hash, atom);
    });
    atom
}

/// [`existing_property_atom`] for a string key VALUE: a heap string, or an
/// SSO immediate whose text is read from the value itself, so no heap copy
/// of the key is made. `None` for any other value and for a spelling no
/// property atom has.
///
/// # Safety
/// `bits` is a live key value.
#[inline]
pub(crate) unsafe fn existing_property_atom_of_value(bits: u64) -> Option<*const StringHeader> {
    let key = crate::JSValue::from_bits(bits);
    if key.is_string() {
        return existing_property_atom(key.as_string_ptr());
    }
    if !key.is_short_string() {
        return None;
    }
    let mut scratch = [0u8; crate::value::SHORT_STRING_MAX_LEN];
    let bytes = crate::string::js_string_key_bytes(key, &mut scratch)?;
    let atom = atom_lookup(
        bytes,
        crate::object::key_bytes_hash(bytes.as_ptr(), bytes.len()),
    )?;
    ((*atom).flags & PROPERTY_ATOM != 0).then_some(atom)
}

pub(crate) unsafe fn property_atom_from_value(bits: u64) -> u64 {
    // A spelling that already has its atom is answered from the value (an
    // SSO key needs no heap copy); only a new spelling mints one below.
    if let Some(atom) = existing_property_atom_of_value(bits) {
        return crate::JSValue::string_ptr(atom as *mut _).bits();
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let key = scope.root_nanbox_f64(f64::from_bits(bits));
    let header = crate::value::js_get_string_pointer_unified(key.get_nanbox_f64());
    crate::JSValue::string_ptr(property_atom(header as *const StringHeader) as *mut _).bits()
}

/// Called by literal initialization before module execution. The image itself
/// is the program atom; no copied literal or per-site identity is installed.
#[no_mangle]
pub unsafe extern "C" fn js_string_register_property_atom(key: *const StringHeader) {
    let hash = property_atom_hash(key);
    let _ = ATOMS.try_with(|t| {
        let table = &mut *t.get();
        let bytes = std::slice::from_raw_parts(string_data(key), (*key).byte_len as usize);
        if let Some(existing) = table.lookup(bytes, hash) {
            if existing == key {
                return;
            }
            if (*existing).flags & PROPERTY_ATOM != 0 {
                // A live data atom already owns this spelling. Keep canonical
                // equality until it dies; source-name membership is immutable.
                if !is_image_string(existing) {
                    (*(existing as *mut StringHeader)).flags |= PROGRAM_ATOM;
                }
                return;
            }
            // Legacy layout seeding precedes pools and makes plain pinned
            // strings. They are not table keys: adopt the image atom here.
            if let Some(entry) = table
                .slots
                .iter_mut()
                .find(|entry| entry.string_ptr == existing as usize)
            {
                // GC_STORE_AUDIT(ROOT): Weak metadata custody, not a strong root: Program-name seeding adopts immutable image atom only when existing canonical entry is not already a live data atom. Agent-local weak ATOMS participates in metadata repair and pre-sweep pruning, not marking.
                entry.string_ptr = key as usize;
                return;
            }
        }
        table.insert(hash, key);
    });
}

/// This weak registry never emits roots in the mark or copy phase. Live atom
/// addresses are repaired after tracing; dead entries are pruned before sweep.
pub(crate) fn scan_property_atoms(visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    if !visitor.is_metadata_rewrite_phase() {
        return;
    }
    let _ = ATOMS.try_with(|t| unsafe {
        for entry in &mut (*t.get()).slots {
            if entry.string_ptr != 0 {
                visitor.visit_metadata_usize_slot(&mut entry.string_ptr);
            }
        }
    });
}

pub(crate) fn prune_property_atoms(is_dead: &dyn Fn(usize) -> bool) {
    let _ = ATOMS.try_with(|t| unsafe {
        let table = &mut *t.get();
        let mut removed = false;
        for entry in &mut table.slots {
            if entry.string_ptr != 0 && is_dead(entry.string_ptr) {
                // GC_STORE_AUDIT(POINTER_FREE): Weak atom pruning writes string_ptr=0 after is_dead; removes pointer rather than installing an edge.
                entry.string_ptr = 0;
                table.len -= 1;
                removed = true;
            }
        }
        if removed {
            let capacity = table.slots.len();
            let old = std::mem::replace(
                &mut table.slots,
                vec![
                    InternEntry {
                        hash: 0,
                        string_ptr: 0
                    };
                    capacity
                ],
            );
            for entry in old.into_iter().filter(|e| e.string_ptr != 0) {
                table.place(entry);
            }
        }
    });
}

#[no_mangle]
pub unsafe extern "C" fn js_string_register_property_atoms(
    keys: *const *const StringHeader,
    count: u32,
) {
    for i in 0..count as usize {
        js_string_register_property_atom(*keys.add(i));
    }
}
