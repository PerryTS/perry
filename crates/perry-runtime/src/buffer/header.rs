/// Reserved class id for `instanceof Uint8Array` (a plain `Uint8Array` or a
/// Node `Buffer`, which is a `Uint8Array` subclass).
pub const BUFFER_TYPE_ID: u32 = 0xFFFF0004;

/// Reserved class id for `instanceof Buffer` (#11239): Node Buffers only, not
/// a plain `Uint8Array`. Keep in sync with codegen's `lower_instanceof` map.
pub const NODE_BUFFER_CLASS_ID: u32 = 0xFFFF000C;

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

#[inline]
pub(crate) fn buffer_payload_size(capacity: usize) -> usize {
    std::mem::size_of::<BufferHeader>() + capacity
}

// # Which flavor a `BufferHeader` cell is (#10694)
//
// The brand is the cell's GC type byte: `GC_TYPE_BUFFER` for a Node `Buffer`
// (what `buffer_alloc` births) and one `GC_TYPE_BUFFER_*` per other flavor
// (`gc/types.rs`, contiguous block). Every recognizer below is the header
// load the runtime's other brand probes already do, plus a compare.
//
// It used to be membership in ten address-keyed side tables (`BUFFER_REGISTRY`,
// `UINT8ARRAY_FROM_CTOR`, `ARRAY_BUFFER_REGISTRY`, `SHARED_ARRAY_BUFFER_REGISTRY`,
// `DATA_VIEW_REGISTRY`, `SECRET_KEY_REGISTRY`, a process-global external
// Uint8Array set, ...), each behind a latch and an address window. Natively
// compiled `tsc` paid 79.7M `is_registered_buffer` probes for 9 live buffers,
// 26.2M of them past the window into a thread-local hash (#10694); stdlib
// threads had to publish into process-global copies because a thread-local set
// cannot see a buffer born on another thread; and every table needed a death
// prune, a thread-exit hook and an ABA argument. The type byte has none of
// those problems: it is born with the cell, travels with it across threads,
// and dies with it.
use crate::fast_hash::{new_ptr_hash_map, PtrHashMap};
use crate::gc::{
    is_buffer_family_type, is_uint8array_buffer_type, GcHeader, GC_HEADER_SIZE,
    GC_TYPE_BUFFER_ARRAY_BUFFER, GC_TYPE_BUFFER_CRYPTO_KEY, GC_TYPE_BUFFER_DATA_VIEW,
    GC_TYPE_BUFFER_SECRET_KEY, GC_TYPE_BUFFER_SHARED_ARRAY_BUFFER, GC_TYPE_BUFFER_UINT8ARRAY,
};
use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

/// The buffer-family GC type of the cell at `addr`, or `None` when `addr` is
/// not a `BufferHeader` cell. One magnitude/alignment check, then the header
/// load every other brand probe in the runtime does (`try_read_gc_header`'s
/// contract: `addr` is a GC allocation's user address or non-pointer bits).
#[inline(always)]
pub(crate) fn buffer_family_type(addr: usize) -> Option<u8> {
    let obj_type = unsafe { crate::value::addr_class::try_read_gc_header(addr) }?.obj_type;
    if !is_buffer_family_type(obj_type) {
        return None;
    }
    // The one POINTER-tagged value with no `GcHeader` is a `Box`-leaked symbol
    // (`Symbol.for`, the well-knowns): its `addr - 8` is foreign allocator
    // bytes that can equal any type byte. Every symbol carries `SYMBOL_MAGIC`
    // in its first word, so a header that claims a buffer is believed unless
    // that word matches (the #7850 screen); a buffer whose `length` happens to
    // equal the magic pays one ownership check instead.
    if unsafe { crate::symbol::may_be_symbol_header(addr as *const u8) } && !header_is_owned(addr) {
        return None;
    }
    Some(obj_type)
}

/// [`buffer_family_type`] for a word that may not be an address at all, or may
/// name unmapped memory (a receiver decoded from arbitrary bits, #7531 /
/// #8067): the allocator proves ownership BEFORE the header is read. The
/// recognizers above assume what every other header probe in the runtime
/// assumes — a real cell or bits the magnitude/alignment gate rejects — and a
/// caller that cannot promise that uses this instead. Not for hot paths.
pub(crate) fn buffer_family_type_owned(addr: usize) -> Option<u8> {
    let obj_type = match unsafe { crate::value::addr_class::try_read_tracked_gc_header(addr) } {
        Some(header) => unsafe { header.as_ref() }.obj_type,
        None if crate::shared_sab::is_shared_sab(addr) => GC_TYPE_BUFFER_SHARED_ARRAY_BUFFER,
        None => return None,
    };
    is_buffer_family_type(obj_type).then_some(obj_type)
}

/// Allocator-proven ownership of `addr`'s header: a tracked arena/malloc GC
/// allocation, or a process-global SharedArrayBuffer block.
#[cold]
#[inline(never)]
pub(crate) fn header_is_owned(addr: usize) -> bool {
    unsafe { crate::value::addr_class::try_read_tracked_gc_header(addr) }.is_some()
        || crate::shared_sab::is_shared_sab(addr)
}

/// Re-stamp the brand of the buffer-family cell at `addr`. A producer allocates
/// through `buffer_alloc` (a Node `Buffer`) and then says what it made; this is
/// that statement. Anything that is not already a buffer-family cell is left
/// alone, so no address can be "registered" into a brand it does not carry.
///
/// Every brand change drops the address from the emitted-code byte admission
/// cache: only a byte-view brand may sit there (#10515), and the next access
/// re-admits it if the new brand allows.
fn set_buffer_brand(addr: usize, brand: u8) -> bool {
    debug_assert!(is_buffer_family_type(brand));
    if buffer_family_type(addr).is_none() {
        return false;
    }
    u8_inline_cache_invalidate(addr);
    // SAFETY: `buffer_family_type` just read this header through the same
    // magnitude/alignment gate; every flavor shares one GcTypeInfo, so only
    // the brand changes.
    unsafe { (*((addr - GC_HEADER_SIZE) as *mut GcHeader)).obj_type = brand };
    true
}

static EXTERNAL_CRYPTO_KEY_META_REGISTRY: OnceLock<Mutex<HashMap<usize, CryptoKeyMeta>>> =
    OnceLock::new();

/// Test probe (#11547): is `addr` in the PROCESS-GLOBAL external CryptoKey
/// metadata registry (`EXTERNAL_CRYPTO_KEY_META_REGISTRY`)?
///
/// Unlike `crypto_key_meta`, which reads the cell's brand and this thread's
/// table first, it touches neither the cell nor a thread-local, so a
/// thread-exit range hook may call it from a TLS destructor. Plain lock, no
/// allocation.
#[doc(hidden)]
pub fn external_registries_hold_for_test(addr: usize) -> bool {
    use std::sync::PoisonError;
    EXTERNAL_CRYPTO_KEY_META_REGISTRY.get().is_some_and(|m| {
        m.lock()
            .unwrap_or_else(PoisonError::into_inner)
            .contains_key(&addr)
    })
}

fn external_crypto_keys() -> &'static Mutex<HashMap<usize, CryptoKeyMeta>> {
    crate::once_init::get_or_init(&EXTERNAL_CRYPTO_KEY_META_REGISTRY, || {
        Mutex::new(HashMap::new())
    })
}

/// Called by the GC's buffer sweep when a CryptoKey-flagged `BufferHeader`
/// dies, so perry-stdlib can drop the matching entry from its own
/// `addr -> CryptoKeyMaterial` map. Registered by
/// `js_set_crypto_key_death_hook` at startup; stays null when stdlib isn't
/// linked. Must not allocate — it runs inside the sweep.
pub type CryptoKeyDeathHookFn = extern "C" fn(usize);
static CRYPTO_KEY_DEATH_HOOK: std::sync::atomic::AtomicPtr<()> =
    std::sync::atomic::AtomicPtr::new(std::ptr::null_mut());

/// Install the dead-CryptoKey callback (called by perry-stdlib at startup —
/// this crate can't call into perry-stdlib, which depends on it). Same
/// contract as the `js_set_native_*_dispatch` family in `value::handle`.
#[no_mangle]
pub extern "C" fn js_set_crypto_key_death_hook(func: CryptoKeyDeathHookFn) {
    CRYPTO_KEY_DEATH_HOOK.store(func as *mut (), std::sync::atomic::Ordering::SeqCst);
}

fn notify_crypto_key_death(addr: usize) {
    let ptr = CRYPTO_KEY_DEATH_HOOK.load(std::sync::atomic::Ordering::SeqCst);
    if ptr.is_null() {
        return;
    }
    let hook: CryptoKeyDeathHookFn = unsafe { std::mem::transmute(ptr) };
    hook(addr);
}

pub type CryptoKeyMeta = (u8, u8, u8, bool, u32, u32);

// Attributes of a branded cell, keyed by its (non-moving) address. These are
// not brands: each one is consulted only after the cell's type byte says it can
// have the attribute, and each entry is dropped by the cell's finalize hook
// (`finalize_collected_dead_buffer`).
crate::perry_thread_local! {
    /// #10873: `ArrayBuffer addr -> maxByteLength` for RESIZABLE buffers.
    /// Presence IS the `[[ArrayBufferMaxByteLength]]` internal slot. A plain
    /// address-keyed attribute of a non-moving buffer, never dereferenced and
    /// never a root, pruned in `finalize_collected_dead_buffer` (the #6080 ABA
    /// class). The resize logic lives in `buffer::resizable`.
    static RESIZABLE_BUFFER_MAX: RefCell<PtrHashMap<usize, ResizableInfo>> =
        RefCell::new(new_ptr_hash_map());
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
    static BUFFER_AB_ALIAS: RefCell<PtrHashMap<usize, Box<usize>>> =
        RefCell::new(new_ptr_hash_map());
    /// Metadata of `GC_TYPE_BUFFER_CRYPTO_KEY` cells. Numeric to keep
    /// perry-runtime independent from perry-stdlib enums:
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
    /// property dispatch can expose Node's KeyObject metadata surface. A string
    /// has no buffer brand to carry this; it goes when KeyObject becomes an
    /// ordinary object (#11919).
    static ASYMMETRIC_KEY_REGISTRY: RefCell<PtrHashMap<usize, (u8, u8)>> =
        RefCell::new(new_ptr_hash_map());
}

use crate::registry_latch::RegistryLatch;

static RESIZABLE_BUFFER_EVER_MARKED: RegistryLatch = RegistryLatch::new();
static ASYMMETRIC_KEY_EVER_MARKED: RegistryLatch = RegistryLatch::new();
static BUFFER_AB_ALIAS_EVER_SET: RegistryLatch = RegistryLatch::new();

/// Brand the buffer at `addr` as an `ArrayBuffer` (issue #579: a source that
/// `new Uint8Array(ab)` should ALIAS rather than copy).
pub fn mark_as_array_buffer(addr: usize) {
    set_buffer_brand(addr, GC_TYPE_BUFFER_ARRAY_BUFFER);
}

#[inline]
pub fn is_array_buffer(addr: usize) -> bool {
    buffer_family_type(addr) == Some(GC_TYPE_BUFFER_ARRAY_BUFFER)
}

#[derive(Copy, Clone, Debug)]
pub(crate) struct ResizableInfo {
    /// `[[ArrayBufferMaxByteLength]]` — also the payload's reserved capacity.
    pub max_byte_length: u32,
    /// Every payload byte at or past this offset is known to read as zero, so
    /// a grow only has to clear `[old byteLength, dirty_end)`. Never below the
    /// current `byteLength`. See `buffer::resizable`.
    pub dirty_end: u32,
}

/// Record `addr` as a resizable ArrayBuffer.
pub(crate) fn mark_as_resizable_buffer(addr: usize, info: ResizableInfo) {
    // Arm before the insert — see `crate::registry_latch`.
    RESIZABLE_BUFFER_EVER_MARKED.arm();
    RESIZABLE_BUFFER_MAX.with(|r| {
        r.borrow_mut().insert(addr, info);
    });
}

/// The resizable state of `addr`, or `None` for a fixed-length buffer.
#[inline]
pub(crate) fn resizable_info(addr: usize) -> Option<ResizableInfo> {
    if RESIZABLE_BUFFER_EVER_MARKED.is_idle() {
        return None;
    }
    RESIZABLE_BUFFER_MAX.with(|r| r.borrow().get(&addr).copied())
}

/// Move a resizable buffer's known-zero boundary. A no-op for any other address.
pub(crate) fn set_resizable_dirty_end(addr: usize, dirty_end: u32) {
    RESIZABLE_BUFFER_MAX.with(|r| {
        if let Some(info) = r.borrow_mut().get_mut(&addr) {
            info.dirty_end = dirty_end;
        }
    });
}

/// True once any resizable ArrayBuffer has existed in this process.
#[inline]
pub(crate) fn any_resizable_buffer() -> bool {
    RESIZABLE_BUFFER_EVER_MARKED.is_armed()
}

/// `[[ArrayBufferMaxByteLength]]`, or `None` for a fixed-length buffer.
#[inline]
pub fn resizable_max_byte_length(addr: usize) -> Option<u32> {
    resizable_info(addr).map(|info| info.max_byte_length)
}

#[cfg(test)]
pub(crate) fn test_resizable_registry_len() -> usize {
    RESIZABLE_BUFFER_MAX.with(|r| r.borrow().len())
}

/// Brand the buffer at `addr` as a `SharedArrayBuffer`. (A process-global
/// `shared_sab` block is stamped at allocation and needs no call.)
pub fn mark_as_shared_array_buffer(addr: usize) {
    set_buffer_brand(addr, GC_TYPE_BUFFER_SHARED_ARRAY_BUFFER);
}

/// A thread-local or process-global `SharedArrayBuffer` — both carry the brand.
#[inline]
pub fn is_shared_array_buffer(addr: usize) -> bool {
    buffer_family_type(addr) == Some(GC_TYPE_BUFFER_SHARED_ARRAY_BUFFER)
}

#[inline]
pub fn is_any_array_buffer(addr: usize) -> bool {
    matches!(
        buffer_family_type(addr),
        Some(GC_TYPE_BUFFER_ARRAY_BUFFER | GC_TYPE_BUFFER_SHARED_ARRAY_BUFFER)
    )
}

/// Brand the view at `addr` as a `DataView`, so util.types can tell the
/// ArrayBufferView predicate from the TypedArray ones.
pub fn mark_as_data_view(addr: usize) {
    set_buffer_brand(addr, GC_TYPE_BUFFER_DATA_VIEW);
}

#[inline]
pub fn is_data_view(addr: usize) -> bool {
    buffer_family_type(addr) == Some(GC_TYPE_BUFFER_DATA_VIEW)
}

/// A freshly allocated buffer cell at `addr`. The brand is already in its
/// header (`buffer_alloc` births a Node `Buffer`); this only clears what a
/// previous occupant of the address could have left in the attribute tables
/// (belt and suspenders: the finalize hook drops them when a cell dies).
pub fn register_buffer(ptr: *const BufferHeader) {
    super::own_props::clear_buffer_own_props(ptr as usize);
    // A fresh cell at a reused address must never inherit the previous
    // occupant's inline-admission entry (#9342).
    u8_inline_cache_invalidate(ptr as usize);
}

/// Buffers this size or smaller used to come from a bump slab
/// (`SMALL_BUF_SLAB`) that was never reclaimed. Kept as the size boundary
/// other modules quote; the slab itself is gone.
pub const SMALL_BUF_THRESHOLD: u32 = 256;

/// Always false: the small-buffer slab no longer exists, so no buffer lives at
/// a heap-plausible address without a `GcHeader`.
pub(crate) fn is_small_buf_slab_addr(_addr: usize) -> bool {
    false
}

/// Is `addr` a `BufferHeader` cell of any flavor — a Node `Buffer`, a
/// `Uint8Array`, an `ArrayBuffer`, a `SharedArrayBuffer` (thread-local or
/// process-global), a `DataView` or a key object? One header load and two
/// compares; there is no registry behind it.
#[inline]
pub fn is_registered_buffer(addr: usize) -> bool {
    buffer_family_type(addr).is_some()
}

/// Brand the buffer at `addr` as a `Uint8Array` (formatted as
/// `Uint8Array(N) [ a, b, c ]` instead of `<Buffer aa bb cc>`). A cell that is
/// already Uint8Array-backed (a plain Uint8Array, a secret `KeyObject`, a
/// `CryptoKey`) keeps its more specific brand.
pub fn mark_as_uint8array(addr: usize) {
    if buffer_family_type(addr).is_some_and(is_uint8array_buffer_type) {
        return;
    }
    set_buffer_brand(addr, GC_TYPE_BUFFER_UINT8ARRAY);
}

/// Forget process-wide facts about addresses inside arena blocks a dying
/// thread is giving back (#11463): the emitted-code byte admission cache and
/// the external CryptoKey metadata, both of which outlive the thread's TLS.
fn release_external_buffer_registries_in_freed_ranges(
    freed: &crate::arena::thread_exit::FreedRanges,
) {
    use std::sync::atomic::Ordering;
    use std::sync::PoisonError;
    for slot in PERRY_U8_INLINE_CACHE.iter() {
        let old = slot.load(Ordering::Relaxed);
        if old != 0 && freed.contains(old as usize) {
            let _ = slot.compare_exchange(old, 0, Ordering::Relaxed, Ordering::Relaxed);
        }
    }
    if let Some(map) = EXTERNAL_CRYPTO_KEY_META_REGISTRY.get() {
        map.lock()
            .unwrap_or_else(PoisonError::into_inner)
            .retain(|&addr, _| !freed.contains(addr));
    }
}

fn register_thread_exit_hook() {
    static REGISTER: std::sync::Once = std::sync::Once::new();
    REGISTER.call_once(|| {
        crate::arena::thread_exit::register_thread_exit_range_hook(
            release_external_buffer_registries_in_freed_ranges,
        )
    });
}

/// Brand a Uint8Array produced by perry-stdlib (which may run off the main
/// thread). The brand is in the cell, so every thread sees it; nothing is
/// published anywhere else.
#[no_mangle]
pub extern "C" fn js_buffer_mark_as_uint8array_external(addr: usize) {
    mark_as_uint8array(addr);
}

/// Brand the buffer at `addr` as a secret `KeyObject` (`crypto.createSecretKey`).
/// It keeps Buffer storage so crypto/HMAC call paths can still read the raw
/// key bytes, while property/method dispatch exposes the KeyObject surface. A
/// `CryptoKey` keeps its brand.
pub fn mark_as_secret_key(addr: usize) {
    if buffer_family_type(addr) == Some(GC_TYPE_BUFFER_CRYPTO_KEY) {
        return;
    }
    set_buffer_brand(addr, GC_TYPE_BUFFER_SECRET_KEY);
}

#[inline]
pub fn is_secret_key(addr: usize) -> bool {
    buffer_family_type(addr) == Some(GC_TYPE_BUFFER_SECRET_KEY)
}

pub fn mark_as_crypto_key(addr: usize, algo: u8, hash: u8, kind: u8) {
    mark_as_crypto_key_with_flags(
        addr,
        algo,
        hash,
        kind,
        true,
        default_crypto_key_usages(algo, kind),
        0,
    );
}

pub fn mark_as_crypto_key_with_flags(
    addr: usize,
    algo: u8,
    hash: u8,
    kind: u8,
    extractable: bool,
    usages: u32,
    bit_length: u32,
) {
    if !set_buffer_brand(addr, GC_TYPE_BUFFER_CRYPTO_KEY) {
        return;
    }
    CRYPTO_KEY_META_REGISTRY.with(|r| {
        r.borrow_mut()
            .insert(addr, (algo, hash, kind, extractable, usages, bit_length));
    });
}

/// Brand a CryptoKey produced by perry-stdlib's WebCrypto, possibly on another
/// thread. The brand travels with the cell; the metadata also goes to the
/// process-global table because the creating thread's table is not the
/// reader's.
#[no_mangle]
pub extern "C" fn js_buffer_mark_as_crypto_key_external(
    addr: usize,
    algo: u8,
    hash: u8,
    kind: u8,
    extractable: u8,
    usages: u32,
    bit_length: u32,
) {
    register_thread_exit_hook();
    if !set_buffer_brand(addr, GC_TYPE_BUFFER_CRYPTO_KEY) {
        return;
    }
    let meta = (algo, hash, kind, extractable != 0, usages, bit_length);
    CRYPTO_KEY_META_REGISTRY.with(|r| {
        r.borrow_mut().insert(addr, meta);
    });
    if let Ok(mut r) = external_crypto_keys().lock() {
        r.insert(addr, meta);
    }
}

/// Test probe: does this thread's CryptoKey metadata table hold `addr`,
/// regardless of the cell's brand?
#[cfg(test)]
pub(crate) fn test_crypto_key_meta_registered(addr: usize) -> bool {
    CRYPTO_KEY_META_REGISTRY.with(|r| r.borrow().contains_key(&addr))
}

/// The CryptoKey metadata of `addr`, or `None` when `addr` is not a CryptoKey.
/// The brand decides; the tables are read only for a real CryptoKey.
pub fn crypto_key_meta(addr: usize) -> Option<CryptoKeyMeta> {
    if buffer_family_type(addr) != Some(GC_TYPE_BUFFER_CRYPTO_KEY) {
        return None;
    }
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

/// `kind`: 1 public, 2 private. `asym_type`: 1 rsa, 2 ec (P-256), 3 ed25519,
/// 4 x25519, 5 ec (P-384), 6 ec (P-521).
pub fn mark_as_asymmetric_key(addr: usize, kind: u8, asym_type: u8) {
    // A non-byte-view brand revokes inline element admission (#10515).
    u8_inline_cache_invalidate(addr);
    ASYMMETRIC_KEY_EVER_MARKED.arm();
    ASYMMETRIC_KEY_REGISTRY.with(|r| {
        r.borrow_mut().insert(addr, (kind, asym_type));
    });
}

#[inline]
pub fn asymmetric_key_meta(addr: usize) -> Option<(u8, u8)> {
    if ASYMMETRIC_KEY_EVER_MARKED.is_idle() {
        return None;
    }
    ASYMMETRIC_KEY_REGISTRY.with(|r| r.borrow().get(&addr).copied())
}

/// #9342: direct-mapped inline element-access admission cache for byte-view
/// `BufferHeader`s, exported under a stable link name for the codegen's
/// guarded inline byte loads and stores (`perry-codegen/src/expr/
/// u8_buffer_read.rs`) and consulted first by the runtime byte accessors.
///
/// An entry holds the full address of a **live, registered byte view — a
/// `Uint8Array` or a Node `Buffer` (`buffer_brand` says so) — whose
/// authoritative bytes are inline at `header + 8`** (no foreign backing and no
/// registered view). Under that contract the emitted code may do
/// `len = *(u32*)addr; addr + 8 + idx` directly, for a read AND for a write:
/// a write to an owning buffer is exactly `js_buffer_set`'s store, because
/// every view over it resolves its bytes through the backing (`buffer/view.rs`)
/// rather than holding a copy.
///
///  * `Buffer` was admitted too in #10515: its element semantics are the
///    `Uint8Array`'s, and requiring the `mark_as_uint8array` marker sent every
///    `Buffer.alloc` byte through the registry probes on every access. An
///    `ArrayBuffer`, `SharedArrayBuffer`, `DataView` or key object shares the
///    `BufferHeader` storage but is NOT integer-indexed (a DataView even keeps
///    its data pointer in that payload), so it is never admitted, and every
///    `mark_as_*` for those brands invalidates the address in case a mark ever
///    follows a prime;
///
///  * shared views (`js_buffer_slice` / `new Uint8Array(arrayBuffer)`) are
///    excluded — their allocation is only a header. Runtime reads resolve
///    through `buffer_data` to the ultimate backing plus the view offset;
///  * foreign-backed wrappers (`buffer_alloc_foreign`, bun:ffi externals) are
///    excluded at prime time — their payload after `BufferHeader` holds a
///    native data pointer, not inline bytes;
///  * ABA is closed the same way as every other buffer identity table:
///    `finalize_collected_dead_buffer` clears the entry when the buffer dies,
///    and `register_buffer` clears it again when the address is re-issued
///    (belt and suspenders, mirroring its own-props clear).
///
/// #10515: TWO-WAY set-associative. An address maps to the slot PAIR
/// `(addr >> 3) & 62` and may live in either of its two slots,
/// so two hot buffers that hash together (nanoid's pool + its alphabet table)
/// no longer evict each other on every alternate access — each miss re-ran the
/// admission probes, which cost more than the access itself. The pair formula
/// is duplicated by codegen (`u8_buffer_read.rs::emit_u8_cache_admission`) —
/// keep in sync.
pub const U8_INLINE_CACHE_SLOTS: usize = 64;
#[no_mangle]
pub static PERRY_U8_INLINE_CACHE: [std::sync::atomic::AtomicU64; U8_INLINE_CACHE_SLOTS] =
    [const { std::sync::atomic::AtomicU64::new(0) }; U8_INLINE_CACHE_SLOTS];

/// The first slot of `addr`'s pair; the pair is `[p, p + 1]`.
#[inline(always)]
fn u8_inline_cache_pair(addr: usize) -> usize {
    (addr >> 3) & (U8_INLINE_CACHE_SLOTS - 2)
}

/// Test-only: does the admission cache currently hold exactly `addr`?
/// Reads the pair the way the emitted guard does — full-address compares.
#[cfg(test)]
pub(crate) fn test_u8_inline_cache_holds(addr: usize) -> bool {
    u8_inline_cache_hit(addr)
}

/// Test probe (#11589): does the admission cache hold exactly `addr`, in
/// either way of its pair? The public twin of `test_u8_inline_cache_holds`
/// for out-of-crate tests, so they never re-derive the slot formula. Reads no
/// thread-local, so a thread-exit range hook may call it.
#[doc(hidden)]
pub fn u8_inline_cache_holds_for_test(addr: usize) -> bool {
    u8_inline_cache_hit(addr)
}

#[inline]
pub(crate) fn u8_inline_cache_invalidate(addr: usize) {
    let pair = u8_inline_cache_pair(addr);
    for slot in [pair, pair + 1] {
        if PERRY_U8_INLINE_CACHE[slot].load(std::sync::atomic::Ordering::Relaxed) == addr as u64 {
            PERRY_U8_INLINE_CACHE[slot].store(0, std::sync::atomic::Ordering::Relaxed);
        }
    }
}

/// `addr` holds an admission in [`PERRY_U8_INLINE_CACHE`]: it is a live
/// owning byte view whose `length` is the `u32` at offset 0 and whose bytes
/// are inline at `addr + 8`. Two loads and two compares.
#[inline(always)]
pub(crate) fn u8_inline_cache_hit(addr: usize) -> bool {
    use std::sync::atomic::Ordering::Relaxed;
    let pair = u8_inline_cache_pair(addr);
    addr != 0
        && (PERRY_U8_INLINE_CACHE[pair].load(Relaxed) == addr as u64
            || PERRY_U8_INLINE_CACHE[pair + 1].load(Relaxed) == addr as u64)
}

/// Resolve an immutable byte receiver once for generated synchronous code.
/// Buffer-family cells and their native backing are nonmoving. Length remains
/// live at each access: detach and resize invalidate bounds, not this pointer.
/// Foreign wrappers can rebind and therefore never receive this proof.
#[no_mangle]
pub extern "C" fn js_u8_resolve_read_data(boxed: f64) -> usize {
    let value = crate::value::JSValue::from_bits(boxed.to_bits());
    if !value.is_pointer() {
        return 0;
    }
    let addr = value.as_pointer::<u8>() as usize;
    if !buffer_family_type_owned(addr)
        .is_some_and(|kind| kind == crate::gc::GC_TYPE_BUFFER || kind == GC_TYPE_BUFFER_UINT8ARRAY)
    {
        return 0;
    }
    let header = unsafe { crate::gc::header_from_trusted_user_ptr(addr as *const u8) };
    if unsafe { (*header)._reserved } & crate::gc::GC_BUFFER_VIEW_DATA != 0 {
        return unsafe { super::view::cached_data_ptr(addr as *const BufferHeader) } as usize;
    }
    u8_inline_cache_try_prime(addr);
    if u8_inline_cache_hit(addr) {
        addr + std::mem::size_of::<BufferHeader>()
    } else {
        0
    }
}

#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_JS_U8_RESOLVE_READ_DATA: extern "C" fn(f64) -> usize = js_u8_resolve_read_data;

/// Admit `addr` to the inline-access cache iff it satisfies the cache
/// contract above. Called from the codegen slow arms (`js_u8_buffer_read_f64`
/// and the #10515 i32 get/set twins) and from the runtime byte accessors'
/// registry arm, so a miss primes the next access. A new admission takes an
/// empty slot of its pair, else the first slot, demoting that slot's entry to
/// the second (which drops the older of the two).
pub(crate) fn u8_inline_cache_try_prime(addr: usize) {
    use std::sync::atomic::Ordering::Relaxed;
    if u8_inline_cache_hit(addr) {
        return;
    }
    if super::exotic_view::is_uint8_view_buffer(addr)
        // View metadata is thread-local; its absence on a different agent
        // cannot admit a pointer-slot allocation as owning inline storage.
        && unsafe { (*crate::gc::header_from_trusted_user_ptr(addr as *const u8))._reserved }
            & crate::gc::GC_BUFFER_VIEW_DATA == 0
        && foreign_backing(addr).is_none()
        && super::view::lookup(addr).is_none()
    {
        register_thread_exit_hook();
        let pair = u8_inline_cache_pair(addr);
        let first = PERRY_U8_INLINE_CACHE[pair].load(Relaxed);
        if first == 0 {
            PERRY_U8_INLINE_CACHE[pair].store(addr as u64, Relaxed);
        } else {
            // The first way's entry moves to the second (dropping whatever was
            // older there); the new admission takes the first.
            PERRY_U8_INLINE_CACHE[pair + 1].store(first, Relaxed);
            PERRY_U8_INLINE_CACHE[pair].store(addr as u64, Relaxed);
        }
    }
}

/// Is `addr` Uint8Array-backed storage whose JS value is not a Node
/// `Buffer` — a plain `Uint8Array`, or a secret `KeyObject`'s / `CryptoKey`'s
/// key bytes? Reached from `typedarray_props::typed_array_owner_kind` for every
/// untyped element access: one header load and a compare.
#[inline]
pub fn is_uint8array_buffer(addr: usize) -> bool {
    buffer_family_type(addr).is_some_and(is_uint8array_buffer_type)
}

/// Record that `buf`'s `.buffer` property should resolve to `alias` instead of
/// `buf` itself.  Used by copy paths (`Buffer.from(src)`) to propagate the
/// source's ArrayBuffer identity onto the new buffer — see #1225.
pub fn set_buffer_ab_alias(buf: usize, alias: usize) {
    BUFFER_AB_ALIAS_EVER_SET.arm();
    BUFFER_AB_ALIAS.with(|m| {
        let mut m = m.borrow_mut();
        let slot = m.entry(buf).or_insert_with(|| Box::new(0));
        **slot = alias;
        crate::gc::runtime_write_barrier_external_slot(
            buf,
            &mut **slot as *mut usize as usize,
            alias as u64,
        );
    });
}

/// Look up the ArrayBuffer-identity alias for a Buffer.  Returns `None` for
/// buffers that haven't been involved in a copy chain (their `.buffer` just
/// returns themselves, as before).
#[inline]
pub fn buffer_ab_alias(buf: usize) -> Option<usize> {
    if BUFFER_AB_ALIAS_EVER_SET.is_idle() {
        return None;
    }
    BUFFER_AB_ALIAS.with(|m| m.borrow().get(&buf).map(|alias| **alias))
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
        let alias = super::view::alloc(src, 0, len);
        mark_as_array_buffer(alias as usize);
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

/// Allocate a buffer with the given capacity.
///
/// 2026-07-09 audit: EVERY buffer is now a GC-heap (old-arena) object with a
/// real GcHeader. The former three-tier scheme left <256 B slab buffers and
/// 256 B–16 KB raw-`alloc`'d buffers permanently invisible to the collector
/// — never freed, never counted by any GC trigger — so servers churning
/// small binary data (HTTP chunks, digests, protocol frames) grew RSS
/// monotonically with no GC recourse. The old arena is the right space:
/// buffers are non-movable (raw data pointers are handed to FFI/tokio), and
/// dead buffer runs are reclaimed by full-cycle whole-block resets plus the
/// post-trace registry pruning below. Their bytes now also count toward
/// `arena_total_bytes`, so allocation pressure finally triggers collections.
pub fn buffer_alloc(capacity: u32) -> *mut BufferHeader {
    super::bytes::assert_allocation_allowed();
    // RULE 3 (`object/shape_rule3.rs`): `capacity` occupies payload `+4`, the
    // word the emitted property-read path compares against a cached ShapeId,
    // and a 2 GiB buffer would write `0x8000_0000` there — shape #1. Every
    // user-facing constructor (`Buffer.alloc`, `new ArrayBuffer`, the typed
    // arrays) already stops at `i32::MAX` and raises exactly this
    // `RangeError`; the paths that reached here still clamping at `u32::MAX`
    // (`Buffer.from(arrayLike)`, `Buffer.concat`, `buffer::copy_bytes`) now
    // agree with them instead of producing an unreadable cell.
    let capacity = crate::object::shape_rule3::checked_plus_four_word(
        capacity,
        b"Array buffer allocation failed",
    );
    let ptr = crate::arena::arena_alloc_gc_old(
        buffer_payload_size(capacity as usize),
        8,
        crate::gc::GC_TYPE_BUFFER,
    ) as *mut BufferHeader;
    unsafe {
        let header = (ptr as *mut u8).sub(crate::gc::GC_HEADER_SIZE) as *mut crate::gc::GcHeader;
        (*header).gc_flags |= crate::gc::GC_FLAG_TENURED;
        (*ptr).length = 0;
        (*ptr).capacity = capacity;
    }
    register_buffer(ptr);
    ptr
}

/// Allocate a Buffer-shaped GC wrapper over native-owned memory.
///
/// The header and native data pointer live in Perry's old arena. The byte span
/// is owned by the caller, or released by the supplied Node-API finalizer.
/// A borrowed span must outlive the returned JS value.
/// Fresh allocations start with no foreign-data bit, so recycled addresses
/// cannot inherit a previous owner's native pointer.
pub(crate) fn buffer_alloc_foreign(data: *mut u8, length: u32) -> *mut BufferHeader {
    super::bytes::assert_allocation_allowed();
    // RULE 3: this wrapper is reached from `extern "C"` Node-API entry points
    // where a JS throw has nowhere to land, so the over-range span is clamped
    // rather than refused — the policy `instance_memory_span` already applies
    // to a wasm memory wider than an `i32` byte count (the excess stays
    // invisible to JS instead of wrapping the header). Every caller rejects
    // an over-range length first (`node_api_host::buffers::checked_length`,
    // `bun_ffi::memory`, `webassembly`), so the clamp is a backstop and the
    // debug assertion inside it is what tells us if a new caller skips one.
    let length = crate::object::shape_rule3::clamp_plus_four_word(
        "BufferHeader::capacity (foreign span)",
        length,
    );
    let ptr = crate::arena::arena_alloc_gc_old(
        std::mem::size_of::<ForeignBuffer>(),
        8,
        crate::gc::GC_TYPE_BUFFER,
    ) as *mut ForeignBuffer;
    unsafe {
        let gc = (ptr as *mut u8).sub(crate::gc::GC_HEADER_SIZE) as *mut crate::gc::GcHeader;
        (*gc).gc_flags |= crate::gc::GC_FLAG_TENURED;
        (*gc)._reserved |= crate::gc::GC_BUFFER_FOREIGN_DATA;
        (*ptr).header.length = length;
        (*ptr).header.capacity = length;
        (*ptr).data = data;
        std::ptr::write(&mut (*ptr).owned, None);
        #[cfg(feature = "node-api-host")]
        {
            (*ptr).finalizer = None;
        }
    }
    register_buffer(ptr.cast());
    if crate::hot_diag::receiver_repr_on() {
        crate::hot_diag::receiver_repr_note_constructed(
            crate::hot_diag::ReceiverReprFamily::ExternalBuffer,
        );
    }
    ptr.cast()
}

/// Allocate native backing from birth, so ordinary ArrayBuffer transfer moves
/// the original byte pointer. The wrapper alone lives in the old arena.
pub(crate) fn buffer_alloc_owned(capacity: u32, length: u32) -> *mut BufferHeader {
    let capacity = crate::object::shape_rule3::checked_plus_four_word(
        capacity,
        b"Array buffer allocation failed",
    );
    buffer_adopt_backing(super::backing::Backing::zeroed(capacity), length)
}

pub(crate) fn buffer_adopt_backing(
    backing: super::backing::Backing,
    length: u32,
) -> *mut BufferHeader {
    assert!(length <= backing.capacity());
    let capacity = backing.capacity();
    let ptr = buffer_alloc_foreign(backing.data(), length);
    unsafe {
        (*ptr).capacity = capacity;
        (*(ptr as *mut ForeignBuffer)).owned = Some(backing);
    }
    // Pressure accounting may collect: publish a consistent cell and root it.
    let scope = crate::gc::RuntimeHandleScope::new();
    let root = scope.root_raw_mut_ptr(ptr);
    crate::gc::gc_note_external_side_alloc(capacity as usize);
    root.get_raw_mut_ptr()
}

/// Whether this foreign-shaped cell owns bytes whose release Perry controls.
pub(crate) fn has_owned_backing(addr: usize) -> bool {
    is_foreign_backed_buffer(addr) && unsafe { (*(addr as *const ForeignBuffer)).owned.is_some() }
}

pub(crate) fn take_owned_backing(addr: usize) -> Option<super::backing::Backing> {
    #[cfg(test)]
    let defer = !super::bytes::sabotage("detach_free");
    #[cfg(not(test))]
    let defer = true;
    if !is_foreign_backed_buffer(addr) || (defer && super::bytes::has_pins(addr)) {
        return None;
    }
    let backing = unsafe { (*(addr as *mut ForeignBuffer)).owned.take() };
    if let Some(ref backing) = backing {
        crate::gc::gc_note_external_side_free(backing.capacity() as usize);
    }
    backing
}

/// TLS destruction cannot consult ownership tables or GC accounting. Both
/// the normal finalizer and this path take the same in-cell owner exactly once.
pub(crate) unsafe fn drop_owned_backing_at_thread_exit(header: *mut crate::gc::GcHeader) {
    if is_buffer_family_type((*header).obj_type)
        && (*header).gc_flags & crate::gc::GC_FLAG_FORWARDED == 0
        && (*header)._reserved & crate::gc::GC_BUFFER_FOREIGN_DATA != 0
    {
        let cell = header.cast::<u8>().add(crate::gc::GC_HEADER_SIZE) as *mut ForeignBuffer;
        drop((*cell).owned.take());
    }
}

/// Foreign bytes remain owned by the caller. The data pointer is a raw native
/// address, never a GC edge; Buffer's collector descriptor traces no byte slots.
#[repr(C)]
struct ForeignBuffer {
    header: BufferHeader,
    data: *mut u8,
    owned: Option<super::backing::Backing>,
    #[cfg(feature = "node-api-host")]
    finalizer: Option<crate::node_api_host::FinalizerRecord>,
}

#[cfg(feature = "node-api-host")]
crate::perry_thread_local! {
    /// Foreign-backed buffers on this thread that hold a pending Node-API
    /// finalizer: the native resources shutdown must release. Not a brand (the
    /// cell's header says it is foreign-backed); an inventory of owed native
    /// work, entered when a finalizer is attached and left when it runs.
    static FOREIGN_FINALIZER_OWNERS: RefCell<crate::fast_hash::PtrHashSet<usize>> =
        RefCell::new(crate::fast_hash::new_ptr_hash_set());
}

#[cfg(feature = "node-api-host")]
pub(crate) fn set_foreign_finalizer(
    buffer: *mut BufferHeader,
    finalizer: Option<crate::node_api_host::FinalizerRecord>,
) {
    assert!(is_foreign_backed_buffer(buffer as usize));
    let owed = finalizer.is_some();
    // Native callback/data/module identities are POD, never GC edges.
    unsafe { (*(buffer as *mut ForeignBuffer)).finalizer = finalizer };
    FOREIGN_FINALIZER_OWNERS.with(|owners| {
        let mut owners = owners.borrow_mut();
        if owed {
            owners.insert(buffer as usize);
        } else {
            owners.remove(&(buffer as usize));
        }
    });
}

#[cfg(feature = "node-api-host")]
fn enqueue_foreign_finalizer(addr: usize) {
    if is_foreign_backed_buffer(addr) {
        if let Some(finalizer) = unsafe { (*(addr as *mut ForeignBuffer)).finalizer.take() } {
            crate::node_api_host::enqueue_finalizer(finalizer);
        }
    }
    FOREIGN_FINALIZER_OWNERS.with(|owners| {
        owners.borrow_mut().remove(&addr);
    });
}

/// Node-API shutdown releases live native resources before unloading addons.
/// Finalizer state itself lives only in the cell. Taking it also prevents a
/// later sweep running it twice.
#[cfg(feature = "node-api-host")]
pub(crate) fn enqueue_all_foreign_finalizers() {
    let owners: Vec<usize> =
        FOREIGN_FINALIZER_OWNERS.with(|owners| owners.borrow().iter().copied().collect());
    for addr in owners {
        enqueue_foreign_finalizer(addr);
    }
}

/// Rebind a wasm linear-memory wrapper after memory.grow relocates its bytes.
#[cfg(feature = "wasm-host")]
pub(crate) fn rebind_foreign_buffer(addr: usize, data: *mut u8, length: u32) -> bool {
    if !is_foreign_backed_buffer(addr) {
        return false;
    }
    let length = crate::object::shape_rule3::clamp_plus_four_word(
        "BufferHeader::capacity (foreign rebind)",
        length,
    );
    unsafe {
        let buffer = addr as *mut ForeignBuffer;
        (*buffer).header.length = length;
        (*buffer).header.capacity = length;
        (*buffer).data = data;
    }
    true
}

#[inline]
fn foreign_backing(addr: usize) -> Option<usize> {
    // Only buffer_data calls this, with a live BufferHeader. Every producer,
    // including process-global SAB, now reserves a real preceding GcHeader.
    // Keep the byte-access hot path to a header-bit load, without a registry
    // or ownership lookup for each byte read.
    unsafe {
        let gc = (addr as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
        if (*gc)._reserved & crate::gc::GC_BUFFER_FOREIGN_DATA == 0 {
            return None;
        }
        Some((*(addr as *const ForeignBuffer)).data as usize)
    }
}

pub(crate) fn is_foreign_backed_buffer(addr: usize) -> bool {
    // This public-address probe must prove ownership before reading the header.
    // SAB is a buffer too, but cannot have this per-heap foreign-data layout.
    unsafe { crate::value::addr_class::try_read_tracked_gc_header(addr) }.is_some_and(|header| {
        let header = unsafe { header.as_ref() };
        is_buffer_family_type(header.obj_type)
            && header._reserved & crate::gc::GC_BUFFER_FOREIGN_DATA != 0
    })
}

/// Drop every attribute-table entry keyed by a dead buffer cell's address and
/// run a foreign-backed buffer's finalizer. The `BufferSideTables` finalize
/// hook calls this for every buffer-family cell the sweep frees (#10694; it
/// used to be driven by a post-trace scan of `BUFFER_REGISTRY`). The brand
/// itself needs no pruning: it is the dead cell's type byte, and the next
/// tenant of the address writes its own. Without this the attribute entries
/// would leak and a recycled address would inherit them (the #6080 ABA class).
pub(crate) fn finalize_collected_dead_buffer(addr: usize) {
    drop(take_owned_backing(addr));
    #[cfg(feature = "node-api-host")]
    enqueue_foreign_finalizer(addr);
    // #10873: a recycled address must not inherit resizability.
    if RESIZABLE_BUFFER_EVER_MARKED.is_armed() {
        RESIZABLE_BUFFER_MAX.with(|r| {
            r.borrow_mut().remove(&addr);
        });
    }
    BUFFER_AB_ALIAS.with(|r| {
        r.borrow_mut().remove(&addr);
    });
    // The WebCrypto/KeyObject side tables were missing from this list. They are
    // plain `addr -> metadata` maps that do not root the `BufferHeader`, so a
    // collected CryptoKey/secret-key buffer left its entries behind forever.
    // Two consequences, both real:
    //
    //  * an unbounded leak — every CryptoKey ever created kept an entry in the
    //    thread-local map AND in the process-global one (a 60k-key run leaked
    //    59,998 of them);
    //  * the #6080 ABA class this very function exists to prevent: the old
    //    arena resets a fully-empty block's offset to 0 while keeping its base
    //    pointer (`arena_reset_empty_blocks` + the block-reuse forward scan in
    //    `Arena::alloc`), so a recycled address inherits CryptoKey identity.
    //    `crypto_key_meta`/`is_secret_key` gate `instanceof CryptoKey`,
    //    `util.types.isCryptoKey`/`isKeyObject`, the `[object CryptoKey]` tag,
    //    the `.algorithm`/`.type`/`.usages` property surface, `KeyObject.from`
    //    and `.export()` — an unrelated fresh Buffer landing on a dead key's
    //    address would answer to all of them.
    CRYPTO_KEY_META_REGISTRY.with(|r| {
        r.borrow_mut().remove(&addr);
    });
    // `js_buffer_mark_as_crypto_key_external` also writes the process-global
    // metadata map.
    if let Some(keys) = EXTERNAL_CRYPTO_KEY_META_REGISTRY.get() {
        if let Ok(mut r) = keys.lock() {
            r.remove(&addr);
        }
    }
    // perry-stdlib keeps its own `addr -> CryptoKeyMaterial` map (the primary
    // one `lookup_crypto_key` consults; the runtime table above is only its
    // fallback), and this crate cannot call into perry-stdlib. Notify it
    // through the hook it installs at startup. The callback only removes a
    // HashMap entry — no allocation, so it is safe to run inside the sweep.
    notify_crypto_key_death(addr);
    // The own-property table (`buf.foo = v`, #6406) was missing from this list.
    // It is the same shape as every table above — a plain address-keyed map that
    // does not root the `BufferHeader` — but it had only ONE clear site,
    // `register_buffer`, so an entry was dropped only when the recycled address
    // was re-issued to another *buffer*. Two consequences, both real:
    //
    //  * an unbounded leak — one permanent entry per property-carrying
    //    Buffer/DataView ever created — made worse than the registries above by
    //    the fact that `scan_buffer_own_props_roots_mut` TRACES the stored
    //    values in every GC phase, so a dead buffer's expando closure (and
    //    everything it captures) stayed reachable for the life of the process;
    //  * the #6080 ABA class this function exists to prevent. The surviving
    //    entry's key is a dead address that the scanner keeps handing to
    //    `visit_metadata_usize_slot`, which resolves it against whatever now
    //    occupies those bytes and rewrites the key to the new tenant's address.
    super::own_props::clear_buffer_own_props(addr);
    // A BufferHeader-backed Uint8Array keeps ordinary expandos and its
    // non-extensible marker in the TypedArray side tables. Prune those here as
    // well as in the typed-array finalizer: this representation is not a
    // `GC_TYPE_TYPED_ARRAY` cell, so that path never sees it (#9347).
    crate::typedarray_props::typed_array_clear_own_props(addr);
    crate::typedarray_props::typed_array_clear_no_extend(addr);
    super::view::remove_entries_for_dead_buffer(addr);
    // #9342: drop the dead address from the inline-read admission cache before
    // its block can be reset and re-issued — a stale hit would read the next
    // tenant's memory as (length, bytes).
    u8_inline_cache_invalidate(addr);
}

/// Trace the cached ArrayBuffer identity only while its owning buffer lives.
/// The boxed slot remains stable if other buffers populate the map mid-cycle.
pub(crate) fn visit_ab_alias_slot(addr: usize, mut visit: impl FnMut(*mut u64)) {
    BUFFER_AB_ALIAS.with(|m| {
        if let Some(alias) = m.borrow_mut().get_mut(&addr) {
            visit(&mut **alias as *mut usize as *mut u64);
        }
    });
}

/// Get the canonical data pointer for a buffer or shared view.
pub fn buffer_data(buf: *const BufferHeader) -> *const u8 {
    // The cell carries the derived pointer. No TLS lookup on the hot path;
    // the existing view metadata still owns the GC edge and resize/detach work.
    let gc = unsafe { &*((buf as *const u8).sub(GC_HEADER_SIZE) as *const GcHeader) };
    if gc._reserved & crate::gc::GC_BUFFER_VIEW_DATA != 0 {
        return unsafe { super::view::cached_data_ptr(buf) };
    }
    if let Some(info) = super::view::lookup(buf as usize) {
        // Registration flattens nested views; the owner is retained by the GC
        // descriptor. Detach zeroes view lengths before releasing any pages.
        return unsafe {
            buffer_data(info.backing as *const BufferHeader).add(info.offset as usize)
        };
    }
    foreign_backing(buf as usize)
        .map(|addr| addr as *const u8)
        .unwrap_or_else(|| unsafe { (buf as *const u8).add(std::mem::size_of::<BufferHeader>()) })
}

/// Get the mutable data pointer for a buffer
pub fn buffer_data_mut(buf: *mut BufferHeader) -> *mut u8 {
    buffer_data(buf) as *mut u8
}
