use super::*;

/// Type ID constant for Buffer/Uint8Array - matches class_id 0xFFFF0004
pub const BUFFER_TYPE_ID: u32 = 0xFFFF0004;

/// #5067 — throw a catchable `RangeError: Array buffer allocation failed`
/// (Node/V8's message) when a buffer backing block cannot be allocated,
/// rather than aborting the process.
#[cold]
fn throw_buffer_alloc_failed() -> ! {
    let msg = b"Array buffer allocation failed";
    let s = crate::string::js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
    let err = crate::error::js_rangeerror_new(s);
    crate::exception::js_throw(crate::value::js_nanbox_pointer(err as i64))
}

/// Buffer header - similar to StringHeader but specifically for binary data
/// NOTE: Layout must match ArrayHeader (length at offset 0, capacity at offset 4)
/// because the codegen treats Uint8Array like arrays with hardcoded offsets.
#[repr(C)]
pub struct BufferHeader {
    /// Length in bytes
    pub length: u32,
    /// Capacity (allocated space)
    pub capacity: u32,
}

/// Calculate the layout for a buffer with given capacity
fn buffer_layout(capacity: usize) -> Layout {
    let total_size = std::mem::size_of::<BufferHeader>() + capacity;
    Layout::from_size_align(total_size, 8).unwrap()
}

#[inline]
fn buffer_payload_size(capacity: usize) -> usize {
    std::mem::size_of::<BufferHeader>() + capacity
}

#[inline]
fn buffer_gc_total_size(capacity: usize) -> usize {
    let payload = buffer_payload_size(capacity);
    (crate::gc::GC_HEADER_SIZE + payload + 7) & !7
}

/// Thread-local registry of buffer pointers for instanceof checks.
/// Since BufferHeader has the same layout as ArrayHeader (no type_id field),
/// we track buffer pointers separately to distinguish them from arrays.
use crate::fast_hash::{new_ptr_hash_map, new_ptr_hash_set, PtrHashMap, PtrHashSet};
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::sync::{Mutex, OnceLock};

static EXTERNAL_BUFFER_REGISTRY: OnceLock<Mutex<HashSet<usize>>> = OnceLock::new();
/// Latched true by the first external-buffer registration. Lets the hot
/// `is_registered_buffer` probe — which JSON.stringify runs for every pointer
/// value it serializes (#6009) — skip the registry mutex entirely in the
/// (overwhelmingly common) processes that never register an external buffer.
static EXTERNAL_BUFFERS_NONEMPTY: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);
static EXTERNAL_UINT8ARRAY_REGISTRY: OnceLock<Mutex<HashSet<usize>>> = OnceLock::new();
static EXTERNAL_CRYPTO_KEY_META_REGISTRY: OnceLock<Mutex<HashMap<usize, CryptoKeyMeta>>> =
    OnceLock::new();

fn external_buffers() -> &'static Mutex<HashSet<usize>> {
    EXTERNAL_BUFFER_REGISTRY.get_or_init(|| Mutex::new(HashSet::new()))
}

fn external_uint8arrays() -> &'static Mutex<HashSet<usize>> {
    EXTERNAL_UINT8ARRAY_REGISTRY.get_or_init(|| Mutex::new(HashSet::new()))
}

fn external_crypto_keys() -> &'static Mutex<HashMap<usize, CryptoKeyMeta>> {
    EXTERNAL_CRYPTO_KEY_META_REGISTRY.get_or_init(|| Mutex::new(HashMap::new()))
}

pub type CryptoKeyMeta = (u8, u8, u8, bool, u32);

thread_local! {
    static BUFFER_REGISTRY: RefCell<PtrHashSet<usize>> = RefCell::new(new_ptr_hash_set());
    /// Buffers that were specifically created via `new Uint8Array(...)` —
    /// formatted as `Uint8Array(N) [ a, b, c ]` instead of `<Buffer aa bb cc>`.
    static UINT8ARRAY_FROM_CTOR: RefCell<PtrHashSet<usize>> = RefCell::new(new_ptr_hash_set());
    /// Issue #579: buffers allocated as `new ArrayBuffer(n)` — sources that
    /// `new Uint8Array(ab)` should ALIAS rather than copy. Survives across
    /// `mark_as_uint8array` calls so a second view of the same ArrayBuffer
    /// still aliases (without a separate registry, the first view's mark
    /// would make the second `js_uint8array_new` call mistake the source
    /// for a Uint8Array and fall into the spec-mandated COPY branch).
    static ARRAY_BUFFER_REGISTRY: RefCell<PtrHashSet<usize>> = RefCell::new(new_ptr_hash_set());
    /// SharedArrayBuffer uses the same BufferHeader storage model as
    /// ArrayBuffer, but it must remain distinguishable for util.types
    /// predicates (`isArrayBuffer` is false, `isSharedArrayBuffer` is true).
    static SHARED_ARRAY_BUFFER_REGISTRY: RefCell<PtrHashSet<usize>> =
        RefCell::new(new_ptr_hash_set());
    /// DataView is currently modeled as a view over an existing BufferHeader
    /// backing store. Track constructor-created views so util.types can
    /// distinguish the ArrayBufferView predicate from TypedArray predicates.
    static DATA_VIEW_REGISTRY: RefCell<PtrHashSet<usize>> = RefCell::new(new_ptr_hash_set());
    /// Issue #1225: ArrayBuffer-identity alias map for Buffers produced by
    /// copy paths like `Buffer.from(buf)`.  Node-compatible semantics: the
    /// new Buffer's `.buffer` returns the same ArrayBuffer object as the
    /// source's `.buffer` because both views live inside the shared 8 KiB
    /// pool slab.  Perry allocates fresh inline storage per Buffer, so the
    /// `.buffer` getter would otherwise return the new BufferHeader pointer
    /// and `src.buffer === cp.buffer` would be false.  Storing the source's
    /// resolved alias here lets the getter return a stable identity token.
    /// Limitation: the bytes are not actually inside the aliased buffer, so
    /// reads/writes through `.buffer` won't observe the view's data — only
    /// the `===` identity check matches Node.
    static BUFFER_AB_ALIAS: RefCell<PtrHashMap<usize, usize>> =
        RefCell::new(new_ptr_hash_map());
    /// Buffers returned by `crypto.createSecretKey`. They intentionally keep
    /// Buffer storage so crypto/HMAC call paths can still read raw key bytes,
    /// while object property/method dispatch exposes the KeyObject surface.
    static SECRET_KEY_REGISTRY: RefCell<PtrHashSet<usize>> = RefCell::new(new_ptr_hash_set());
    /// Buffers that should behave as WebCrypto CryptoKey values. Metadata is
    /// numeric to keep perry-runtime independent from perry-stdlib enums:
    /// algo: 1 HMAC, 2 AES-GCM, 3 AES-KW, 4 AES-CBC, 5 AES-CTR, 6 HKDF,
    ///       7 PBKDF2, 8 ECDSA, 9 ECDH, 10 Ed25519, 11 X25519,
    ///       12 RSASSA-PKCS1-v1_5, 13 RSA-OAEP, 14 RSA-PSS,
    ///       15 ECDSA P-384, 16 ECDH P-384, 17 ECDSA P-521,
    ///       18 ECDH P-521, 19 Argon2d, 20 Argon2i, 21 Argon2id,
    ///       22 ChaCha20-Poly1305, 23 KMAC128, 24 KMAC256, 25 AES-OCB,
    ///       26 X448, 27 Ed448, 30 ML-KEM-512, 31 ML-KEM-768,
    ///       32 ML-KEM-1024
    /// hash: 1 SHA-1, 2 SHA-256, 3 SHA-384, 4 SHA-512
    /// kind: 1 secret, 2 private, 3 public
    /// extractable: WebCrypto CryptoKey.extractable
    /// usages: bitset matching WebCrypto usage names
    static CRYPTO_KEY_META_REGISTRY: RefCell<PtrHashMap<usize, CryptoKeyMeta>> =
        RefCell::new(new_ptr_hash_map());
    /// String-backed asymmetric KeyObject surrogates returned by crypto
    /// helpers. They intentionally keep PEM/internal-string storage so the
    /// stdlib crypto routines can parse/read them directly, while runtime
    /// property dispatch can expose Node's KeyObject metadata surface.
    static ASYMMETRIC_KEY_REGISTRY: RefCell<PtrHashMap<usize, (u8, u8)>> =
        RefCell::new(new_ptr_hash_map());
}

pub fn mark_as_array_buffer(addr: usize) {
    ARRAY_BUFFER_REGISTRY.with(|r| {
        r.borrow_mut().insert(addr);
    });
}

pub fn is_array_buffer(addr: usize) -> bool {
    ARRAY_BUFFER_REGISTRY.with(|r| r.borrow().contains(&addr))
}

pub fn mark_as_shared_array_buffer(addr: usize) {
    SHARED_ARRAY_BUFFER_REGISTRY.with(|r| {
        r.borrow_mut().insert(addr);
    });
}

pub fn is_shared_array_buffer(addr: usize) -> bool {
    if SHARED_ARRAY_BUFFER_REGISTRY.with(|r| r.borrow().contains(&addr)) {
        return true;
    }
    // #4913: a SAB backing is process-global. If this thread received it as a
    // module-level value (not a serialized `perry/thread` capture, which would
    // have re-registered it locally) the thread-local set misses, so fall back
    // to the process-global registry. Slow path only — thread-local hits first.
    crate::shared_sab::is_shared_sab(addr)
}

pub fn is_any_array_buffer(addr: usize) -> bool {
    is_array_buffer(addr) || is_shared_array_buffer(addr)
}

pub fn mark_as_data_view(addr: usize) {
    DATA_VIEW_REGISTRY.with(|r| {
        r.borrow_mut().insert(addr);
    });
}

pub fn is_data_view(addr: usize) -> bool {
    DATA_VIEW_REGISTRY.with(|r| r.borrow().contains(&addr))
}

/// Register a buffer pointer in the thread-local registry
pub fn register_buffer(ptr: *const BufferHeader) {
    BUFFER_REGISTRY.with(|r| r.borrow_mut().insert(ptr as usize));
}

// ----- Small-buffer slab allocator ----------------------------------------
//
// GC interaction:
//   Buffers carry no GcHeader and are not tracked in MALLOC_STATE (the existing
//   malloc path also never calls `dealloc` on individual buffers — they live for
//   the lifetime of the thread). Slab blocks are malloc'd once and retained for
//   the same duration. No GC behaviour changes.
//
// Registry:
//   Large buffers (capacity >= SMALL_BUF_THRESHOLD) still go through
//   `register_buffer` and appear in BUFFER_REGISTRY (HashSet).
//   Small buffers skip the HashSet insert; `is_registered_buffer` instead
//   performs a range-check against the (tiny) list of slab blocks — O(n_slabs),
//   typically ≤ 5 entries for a 100k-iteration allocation loop.
//   No false positives: slab blocks exclusively contain BufferHeader allocations
//   and all callers of `is_registered_buffer` pass the header pointer (the
//   NaN-boxed POINTER_TAG value always points to the header start, never to
//   interior data bytes).

/// Capacities strictly below this threshold use the slab fast path.
pub const SMALL_BUF_THRESHOLD: u32 = 256;

/// One slab block covers this many bytes of BufferHeader+data storage.
/// 256 KB → ≥ 1 000 allocations of the max small size (255 bytes), or up to
/// 32 768 allocations of the minimum (0 bytes / header only).
const SLAB_CAPACITY: usize = 256 * 1024;

/// Per-thread bump-pointer slab for small buffers.
/// Raw pointers stored as `usize` to keep the type `Send + Sync`.
struct SmallBufSlab {
    /// Byte offset of the next free slot within the current slab block.
    current: usize,
    /// One-past-the-end offset (absolute address as usize) of the current block.
    end: usize,
    /// (start, end) address pair for every slab block allocated so far.
    /// Used by `is_registered_buffer` to confirm an address is a small buffer.
    ranges: Vec<(usize, usize)>,
}

thread_local! {
    static SMALL_BUF_SLAB: RefCell<SmallBufSlab> = const { RefCell::new(SmallBufSlab {
        current: 0,
        end: 0,
        ranges: Vec::new(),
    }) };
}

fn buffer_alloc_small(capacity: u32) -> *mut BufferHeader {
    let needed = std::mem::size_of::<BufferHeader>() + capacity as usize;
    // Round up to 8-byte boundary so every header is naturally aligned.
    let aligned = (needed + 7) & !7;
    // #5226: reserve an extra `GC_HEADER_SIZE` per buffer for a zeroed sentinel
    // that precedes the returned pointer (see the `write_bytes` below). Buffers
    // are off-GC-heap with no real `GcHeader`, but the runtime is littered with
    // `*(ptr - GC_HEADER_SIZE)` obj_type probes (Promise / Date / Array / class
    // dispatch). Without the sentinel the back-read crosses outside the slab —
    // into the unmapped page before a freshly mapped block for the first buffer
    // — and segfaults. A `0` sentinel matches no `GC_TYPE_*`, so every probe
    // cleanly classifies the buffer as "not my type".
    let slot = crate::gc::GC_HEADER_SIZE + aligned;

    SMALL_BUF_SLAB.with(|slab_ref| {
        let mut slab = slab_ref.borrow_mut();

        if slab.current + slot > slab.end {
            // Current block exhausted (or first call): allocate a fresh slab.
            let layout = Layout::from_size_align(SLAB_CAPACITY, 8).unwrap();
            let block = unsafe { alloc(layout) };
            if block.is_null() {
                panic!(
                    "buffer: failed to allocate small-buffer slab ({} bytes)",
                    SLAB_CAPACITY
                );
            }
            let block_start = block as usize;
            let block_end = block_start + SLAB_CAPACITY;
            slab.ranges.push((block_start, block_end));
            slab.current = block_start;
            slab.end = block_end;
        }

        // Zero the 8-byte sentinel, then hand out the pointer just past it. The
        // sentinel always lands inside the slab block, so `ptr - GC_HEADER_SIZE`
        // is a mapped read; `is_registered_buffer`'s slab-range check still
        // covers `ptr` (it stays within `[block_start, block_end)`).
        let ptr = unsafe {
            std::ptr::write_bytes(slab.current as *mut u8, 0, crate::gc::GC_HEADER_SIZE);
            (slab.current + crate::gc::GC_HEADER_SIZE) as *mut BufferHeader
        };
        slab.current += slot;

        unsafe {
            (*ptr).length = 0;
            (*ptr).capacity = capacity;
        }

        ptr
    })
}

/// True when `addr` lies inside a small-buffer slab block. Slab allocations
/// carry NO GcHeader, so reading `addr - GC_HEADER_SIZE` there yields the
/// previous allocation's trailing data bytes — a content-dependent fake
/// header. `addr_class::try_read_gc_header` consults this before any deref so
/// brand probes (Temporal/Date/Map/Set) can't misroute a small Buffer whose
/// payload happens to spell a matching `obj_type`.
pub(crate) fn is_small_buf_slab_addr(addr: usize) -> bool {
    SMALL_BUF_SLAB.with(|slab_ref| {
        slab_ref
            .borrow()
            .ranges
            .iter()
            .any(|&(start, end)| addr >= start && addr < end)
    })
}

/// Check if a pointer is a registered buffer (for instanceof Uint8Array)
pub fn is_registered_buffer(addr: usize) -> bool {
    // Fast path: address falls within a small-buffer slab block.  All bytes in
    // a slab block belong exclusively to BufferHeader allocations, so any match
    // is definitively a buffer pointer.
    if is_small_buf_slab_addr(addr) {
        return true;
    }
    // Slow path: large buffers tracked in the HashSet registry.
    if BUFFER_REGISTRY.with(|r| r.borrow().contains(&addr)) {
        return true;
    }
    if EXTERNAL_BUFFERS_NONEMPTY.load(std::sync::atomic::Ordering::Acquire)
        && external_buffers()
            .lock()
            .map(|r| r.contains(&addr))
            .unwrap_or(false)
    {
        return true;
    }
    // #4913: recognise a process-global SAB backing reached as a module-level
    // value on a thread that never locally registered it (see
    // `is_shared_array_buffer`).
    crate::shared_sab::is_shared_sab(addr)
}

/// Mark this buffer as one that came from `new Uint8Array(...)` so it
/// formats as `Uint8Array(N) [ ... ]` rather than `<Buffer ...>`.
pub fn mark_as_uint8array(addr: usize) {
    UINT8ARRAY_FROM_CTOR.with(|r| {
        r.borrow_mut().insert(addr);
    });
}

#[no_mangle]
pub extern "C" fn js_buffer_register_external(addr: usize) {
    register_buffer(addr as *const BufferHeader);
    // Latch BEFORE the insert: a concurrent `is_registered_buffer` that
    // observed the latch after the insert-but-before-the-store window would
    // skip the mutex and miss an already-registered buffer.
    EXTERNAL_BUFFERS_NONEMPTY.store(true, std::sync::atomic::Ordering::Release);
    if let Ok(mut r) = external_buffers().lock() {
        r.insert(addr);
    }
}

#[no_mangle]
pub extern "C" fn js_buffer_mark_as_uint8array_external(addr: usize) {
    mark_as_uint8array(addr);
    if let Ok(mut r) = external_uint8arrays().lock() {
        r.insert(addr);
    }
}

pub fn mark_as_secret_key(addr: usize) {
    SECRET_KEY_REGISTRY.with(|r| {
        r.borrow_mut().insert(addr);
    });
}

pub fn is_secret_key(addr: usize) -> bool {
    SECRET_KEY_REGISTRY.with(|r| r.borrow().contains(&addr))
}

pub fn mark_as_crypto_key(addr: usize, algo: u8, hash: u8, kind: u8) {
    mark_as_crypto_key_with_flags(
        addr,
        algo,
        hash,
        kind,
        true,
        default_crypto_key_usages(algo, kind),
    );
}

pub fn mark_as_crypto_key_with_flags(
    addr: usize,
    algo: u8,
    hash: u8,
    kind: u8,
    extractable: bool,
    usages: u32,
) {
    CRYPTO_KEY_META_REGISTRY.with(|r| {
        r.borrow_mut()
            .insert(addr, (algo, hash, kind, extractable, usages));
    });
}

#[no_mangle]
pub extern "C" fn js_buffer_mark_as_crypto_key_external(
    addr: usize,
    algo: u8,
    hash: u8,
    kind: u8,
    extractable: u8,
    usages: u32,
) {
    register_buffer(addr as *const BufferHeader);
    mark_as_uint8array(addr);
    mark_as_crypto_key_with_flags(addr, algo, hash, kind, extractable != 0, usages);
    // Latch BEFORE the insert — see js_buffer_register_external.
    EXTERNAL_BUFFERS_NONEMPTY.store(true, std::sync::atomic::Ordering::Release);
    if let Ok(mut r) = external_buffers().lock() {
        r.insert(addr);
    }
    if let Ok(mut r) = external_uint8arrays().lock() {
        r.insert(addr);
    }
    if let Ok(mut r) = external_crypto_keys().lock() {
        r.insert(addr, (algo, hash, kind, extractable != 0, usages));
    }
}

pub fn crypto_key_meta(addr: usize) -> Option<CryptoKeyMeta> {
    CRYPTO_KEY_META_REGISTRY
        .with(|r| r.borrow().get(&addr).copied())
        .or_else(|| {
            external_crypto_keys()
                .lock()
                .ok()
                .and_then(|r| r.get(&addr).copied())
        })
}

fn default_crypto_key_usages(algo: u8, kind: u8) -> u32 {
    const ENCRYPT: u32 = 1 << 0;
    const DECRYPT: u32 = 1 << 1;
    const SIGN: u32 = 1 << 2;
    const VERIFY: u32 = 1 << 3;
    const DERIVE_KEY: u32 = 1 << 4;
    const DERIVE_BITS: u32 = 1 << 5;
    const WRAP_KEY: u32 = 1 << 6;
    const UNWRAP_KEY: u32 = 1 << 7;
    const ENCAPSULATE_BITS: u32 = 1 << 8;
    const DECAPSULATE_BITS: u32 = 1 << 9;
    const ENCAPSULATE_KEY: u32 = 1 << 10;
    const DECAPSULATE_KEY: u32 = 1 << 11;

    match (algo, kind) {
        (1, 1) => SIGN | VERIFY,
        (23 | 24, 1) => SIGN | VERIFY,
        (2 | 4 | 5 | 22 | 25, 1) => ENCRYPT | DECRYPT | WRAP_KEY | UNWRAP_KEY,
        (3, 1) => WRAP_KEY | UNWRAP_KEY,
        (6 | 7 | 19 | 20 | 21, 1) => DERIVE_KEY | DERIVE_BITS,
        (8 | 10 | 12 | 14 | 15 | 17 | 27, 2) => SIGN,
        (8 | 10 | 12 | 14 | 15 | 17 | 27, 3) => VERIFY,
        (9 | 11 | 16 | 18 | 26, 2) => DERIVE_KEY | DERIVE_BITS,
        (13, 2) => DECRYPT | UNWRAP_KEY,
        (13, 3) => ENCRYPT | WRAP_KEY,
        (30..=32, 2) => DECAPSULATE_BITS | DECAPSULATE_KEY,
        (30..=32, 3) => ENCAPSULATE_BITS | ENCAPSULATE_KEY,
        _ => 0,
    }
}

/// `kind`: 1 public, 2 private. `asym_type`: 1 rsa, 2 ec, 3 ed25519, 4 x25519.
pub fn mark_as_asymmetric_key(addr: usize, kind: u8, asym_type: u8) {
    ASYMMETRIC_KEY_REGISTRY.with(|r| {
        r.borrow_mut().insert(addr, (kind, asym_type));
    });
}

pub fn asymmetric_key_meta(addr: usize) -> Option<(u8, u8)> {
    ASYMMETRIC_KEY_REGISTRY.with(|r| r.borrow().get(&addr).copied())
}

pub fn is_uint8array_buffer(addr: usize) -> bool {
    UINT8ARRAY_FROM_CTOR.with(|r| r.borrow().contains(&addr))
        || external_uint8arrays()
            .lock()
            .map(|r| r.contains(&addr))
            .unwrap_or(false)
}

/// Record that `buf`'s `.buffer` property should resolve to `alias` instead of
/// `buf` itself.  Used by copy paths (`Buffer.from(src)`) to propagate the
/// source's ArrayBuffer identity onto the new buffer — see #1225.
pub fn set_buffer_ab_alias(buf: usize, alias: usize) {
    BUFFER_AB_ALIAS.with(|m| {
        m.borrow_mut().insert(buf, alias);
    });
}

/// Look up the ArrayBuffer-identity alias for a Buffer.  Returns `None` for
/// buffers that haven't been involved in a copy chain (their `.buffer` just
/// returns themselves, as before).
pub fn buffer_ab_alias(buf: usize) -> Option<usize> {
    BUFFER_AB_ALIAS.with(|m| m.borrow().get(&buf).copied())
}

/// Collapse an alias chain to its root: if `buf` already aliases something,
/// return that; otherwise return `buf` itself.  Callers use this to seed the
/// alias on a fresh copy so chained `Buffer.from(Buffer.from(src))` keeps
/// `===` identity with the original source.
pub fn resolve_buffer_ab_alias(buf: usize) -> usize {
    ensure_buffer_ab_alias(buf)
}

/// Return a stable ArrayBuffer identity for a Buffer's `.buffer` / `.parent`
/// property. Perry stores Buffer bytes inline in BufferHeader allocations, so
/// create a BufferHeader-backed ArrayBuffer object lazily and cache it.
pub fn ensure_buffer_ab_alias(buf: usize) -> usize {
    if buf < 0x1000 || !is_registered_buffer(buf) {
        return buf;
    }
    if is_array_buffer(buf) || is_shared_array_buffer(buf) {
        return buf;
    }

    if let Some(alias) = buffer_ab_alias(buf) {
        if is_array_buffer(alias) || is_shared_array_buffer(alias) {
            return alias;
        }
        if alias != buf {
            let resolved = ensure_buffer_ab_alias(alias);
            set_buffer_ab_alias(buf, resolved);
            return resolved;
        }
    }

    unsafe {
        let src = buf as *const BufferHeader;
        let len = (*src).length;
        let alias = buffer_alloc(len);
        (*alias).length = len;
        if len > 0 {
            std::ptr::copy_nonoverlapping(buffer_data(src), buffer_data_mut(alias), len as usize);
        }
        mark_as_array_buffer(alias as usize);
        super::view::register(alias as usize, buf, 0, len);
        set_buffer_ab_alias(buf, alias as usize);
        alias as usize
    }
}

pub fn buffer_backing_array_buffer(buf: usize) -> usize {
    let backing = super::view::backing_of(buf);
    ensure_buffer_ab_alias(backing)
}

pub fn buffer_byte_offset(buf: usize) -> u32 {
    super::view::byte_offset_of(buf)
}

/// Allocate a buffer with the given capacity
pub fn buffer_alloc(capacity: u32) -> *mut BufferHeader {
    // Fast path: small buffers come from a per-thread bump slab (no malloc,
    // no HashSet insert).  Large buffers fall through to the existing malloc path.
    if capacity < SMALL_BUF_THRESHOLD {
        return buffer_alloc_small(capacity);
    }
    if crate::gc::is_large_object_total_size(buffer_gc_total_size(capacity as usize)) {
        let ptr = crate::arena::arena_alloc_gc_old(
            buffer_payload_size(capacity as usize),
            8,
            crate::gc::GC_TYPE_BUFFER,
        ) as *mut BufferHeader;
        unsafe {
            let header =
                (ptr as *mut u8).sub(crate::gc::GC_HEADER_SIZE) as *mut crate::gc::GcHeader;
            (*header).gc_flags |= crate::gc::GC_FLAG_TENURED;
            (*ptr).length = 0;
            (*ptr).capacity = capacity;
        }
        register_buffer(ptr);
        return ptr;
    }
    // #5226: mid-size buffers are raw-`alloc`'d off the GC heap. Reserve a
    // zeroed `GC_HEADER_SIZE` sentinel before the returned pointer so the
    // runtime's `*(ptr - GC_HEADER_SIZE)` obj_type probes read a mapped `0`
    // (matching no `GC_TYPE_*`) instead of faulting at a region boundary.
    let inner = buffer_layout(capacity as usize);
    let layout = Layout::from_size_align(crate::gc::GC_HEADER_SIZE + inner.size(), 8).unwrap();
    unsafe {
        let raw = alloc(layout);
        if raw.is_null() {
            // #5067 — surface a catchable `RangeError` instead of aborting.
            throw_buffer_alloc_failed();
        }
        std::ptr::write_bytes(raw, 0, crate::gc::GC_HEADER_SIZE);
        let ptr = raw.add(crate::gc::GC_HEADER_SIZE) as *mut BufferHeader;
        (*ptr).length = 0;
        (*ptr).capacity = capacity;
        register_buffer(ptr);
        ptr
    }
}

/// Get the data pointer for a buffer
pub fn buffer_data(buf: *const BufferHeader) -> *const u8 {
    unsafe { (buf as *const u8).add(std::mem::size_of::<BufferHeader>()) }
}

/// Get the mutable data pointer for a buffer
pub fn buffer_data_mut(buf: *mut BufferHeader) -> *mut u8 {
    unsafe { (buf as *mut u8).add(std::mem::size_of::<BufferHeader>()) }
}
