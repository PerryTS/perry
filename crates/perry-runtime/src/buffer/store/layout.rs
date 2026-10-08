/// Reserved class id for `instanceof Uint8Array` (a plain `Uint8Array` or a
/// Node `Buffer`, which is a `Uint8Array` subclass).
pub const BUFFER_TYPE_ID: u32 = 0xFFFF0004;

/// Reserved class id for `instanceof Buffer` (#11239): Node Buffers only, not
/// a plain `Uint8Array`. Keep in sync with codegen's `lower_instanceof` map.
pub const NODE_BUFFER_CLASS_ID: u32 = 0xFFFF000C;

pub use super::BufferHeader;

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
    GC_TYPE_BUFFER_SECRET_KEY, GC_TYPE_BUFFER_SHARED_ARRAY_BUFFER,
};
use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

/// Candidate address of a word offered to a byte-cell probe: a POINTER_TAG
/// payload or a legacy untagged raw pointer, classified by TAG before any
/// header is read. Every other tag (a double such as a numeric fd, INT32, SSO
/// and heap strings, handles, singletons) is a primitive whose low 48 bits
/// are not an address; stripping its tag and probing the remainder read a
/// header at an arbitrary address (fs.appendFileSync(fd, ..) segfault). A bare
/// top-16-clear word is also what a denormal double looks like, so it is an
/// address only when Perry's memory owns its header word (this thread's
/// allocator, any thread's arena region, or a process-global
/// SharedArrayBuffer block) — proven before the header is read.
#[inline]
pub(crate) fn byte_word_address(bits: u64) -> Option<usize> {
    if bits < 0x1000 {
        return None;
    }
    if (bits & crate::value::TAG_MASK) == crate::value::POINTER_TAG {
        return Some((bits & crate::value::POINTER_MASK) as usize);
    }
    if (bits >> 48) != 0 {
        return None;
    }
    let addr = bits as usize;
    raw_byte_word_is_owned(addr).then_some(addr)
}

#[cold]
fn raw_byte_word_is_owned(addr: usize) -> bool {
    unsafe { crate::value::addr_class::try_read_tracked_gc_header(addr) }.is_some()
        || addr
            .checked_sub(GC_HEADER_SIZE)
            .is_some_and(|header| crate::arena::region_contains(header, GC_HEADER_SIZE))
        || crate::shared_sab::is_shared_sab(addr)
}

/// [`byte_cell_type`] for a word that may be any JS value: classified by tag
/// first ([`byte_word_address`]), then admitted. The address and full type.
#[inline]
pub(crate) fn byte_cell_of_word(bits: u64) -> Option<(usize, u8)> {
    let addr = byte_word_address(bits)?;
    Some((addr, byte_cell_type(addr)?))
}

/// The full GC type of the byte cell at `addr` (Buffer, Uint8Array, the
/// %TypedArray% kinds, ArrayBuffer, SharedArrayBuffer, DataView, key objects;
/// owners and views), or `None` when `addr` is not one.
///
/// This is the one admission every runtime byte-cell recognizer goes through.
/// A receiver word reaching the runtime need not be a GC cell: a
/// pointer-tagged native value (a RegExp, a host object) can point just past
/// a word that starts with a byte-family type byte, and a view's `link` or an
/// owner's bag read from such a word follows garbage. So the plain header load
/// only decides the negative, and any other type byte answers `None` with one
/// load. A byte-family type byte is confirmed by the allocator
/// ([`byte_cell_is_owned`]) before anything reads the cell.
#[inline(always)]
pub(crate) fn byte_cell_type(addr: usize) -> Option<u8> {
    let obj_type = unsafe { crate::value::addr_class::try_read_gc_header(addr) }?.obj_type;
    if !crate::gc::is_byte_family_type(obj_type) {
        return None;
    }
    byte_cell_is_owned(addr, obj_type).then_some(obj_type)
}

/// The ownership proof behind [`byte_cell_type`]: `addr` is a GC cell whose
/// header names exactly `obj_type`, or a process-global SharedArrayBuffer
/// block. For a caller that already loaded `obj_type` from a plain header read
/// of a word that may not be a GC cell.
///
/// Every byte cell is an old-arena cell, and the process-wide region registry
/// (`arena::region_contains`) owns all arena memory whichever thread
/// allocated it, so the proof holds on any thread. This thread's tracked
/// header read answers first: it is the cheaper probe for its own cells.
///
/// Out of line: [`byte_cell_type`] is inlined into every generic receiver
/// probe (`lookup_typed_array_kind` on each dynamic index get and set,
/// `is_registered_buffer`), and only a byte-family type byte reaches the
/// proof. Inlined there, the allocator walk grew those probes past LLVM's
/// inlining budget, so every non-byte receiver paid an out-of-line call for
/// a test it answers with one load.
#[inline(never)]
pub(crate) fn byte_cell_is_owned(addr: usize, obj_type: u8) -> bool {
    #[cfg(test)]
    if byte_cell_proof_sabotaged() {
        return true;
    }
    (crate::gc::gc_type_is_known(obj_type)
        && unsafe { crate::value::addr_class::try_read_tracked_gc_header_of_type(addr, obj_type) }
            .is_some())
        || byte_cell_in_any_region(addr, obj_type)
}

/// The process-wide arm of [`byte_cell_is_owned`]: a cell of another thread's
/// arena (its header and extent inside one live region, arena-flagged, of the
/// type the caller read), or a process-global SharedArrayBuffer block.
#[cold]
#[inline(never)]
fn byte_cell_in_any_region(addr: usize, obj_type: u8) -> bool {
    if obj_type == GC_TYPE_BUFFER_SHARED_ARRAY_BUFFER && crate::shared_sab::is_shared_sab(addr) {
        return true;
    }
    #[cfg(test)]
    if super::super::bytes::b4_sabotage("byte_cell_region") {
        return false;
    }
    let Some(header_addr) = addr.checked_sub(GC_HEADER_SIZE) else {
        return false;
    };
    if !crate::arena::region_contains(header_addr, GC_HEADER_SIZE) {
        return false;
    }
    // SAFETY: the header word lies inside a live arena region.
    let header = unsafe { &*(header_addr as *const GcHeader) };
    header.obj_type == obj_type
        && header.gc_flags & crate::gc::GC_FLAG_ARENA != 0
        && header.size as usize >= GC_HEADER_SIZE
        && crate::arena::region_contains(header_addr, header.size as usize)
}

/// `PERRY_B4_SABOTAGE=byte_cell_proof` admits every byte-family type byte
/// without the allocator proof; `byte_cell_admission_tests` must go red.
#[cfg(test)]
fn byte_cell_proof_sabotaged() -> bool {
    static SABOTAGED: OnceLock<bool> = OnceLock::new();
    *SABOTAGED.get_or_init(|| super::super::bytes::b4_sabotage("byte_cell_proof"))
}

/// The buffer-family GC type of the cell at `addr` with the view bit cleared,
/// or `None` when `addr` is not a `BufferHeader` cell ([`byte_cell_type`]).
#[inline(always)]
pub(crate) fn buffer_family_type(addr: usize) -> Option<u8> {
    let obj_type = byte_cell_type(addr)?;
    if !is_buffer_family_type(obj_type) {
        return None;
    }
    Some(obj_type & !crate::codegen_abi::BYTES_TYPE_VIEW)
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
    is_buffer_family_type(obj_type).then_some(obj_type & !crate::codegen_abi::BYTES_TYPE_VIEW)
}

/// Allocator-proven ownership of `addr`'s header: a tracked arena/malloc GC
/// allocation, or a process-global SharedArrayBuffer block.
#[cold]
#[inline(never)]
pub(crate) fn header_is_owned(addr: usize) -> bool {
    unsafe { crate::value::addr_class::try_read_tracked_gc_header(addr) }.is_some()
        || crate::shared_sab::is_shared_sab(addr)
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

static ASYMMETRIC_KEY_EVER_MARKED: RegistryLatch = RegistryLatch::new();

#[inline]
pub fn is_array_buffer(addr: usize) -> bool {
    buffer_family_type(addr) == Some(GC_TYPE_BUFFER_ARRAY_BUFFER)
}

#[derive(Copy, Clone, Debug)]
pub(crate) struct ResizableInfo {
    /// `[[ArrayBufferMaxByteLength]]` — also the payload's reserved capacity.
    pub max_byte_length: u32,
}

/// Resizability is an owner-header fact; capacity reserves maxByteLength.
pub(crate) fn set_resizable(addr: usize, _info: ResizableInfo) {
    unsafe {
        (*super::super::store::header(addr))._reserved |= crate::codegen_abi::BYTES_RESIZABLE;
    }
}

#[inline]
pub(crate) fn resizable_info(addr: usize) -> Option<ResizableInfo> {
    unsafe {
        let owner = super::super::store::owner(addr);
        if (*super::super::store::header(owner))._reserved & crate::codegen_abi::BYTES_RESIZABLE
            == 0
        {
            return None;
        }
        let cell = &*(owner as *const BufferHeader);
        Some(ResizableInfo {
            max_byte_length: cell.capacity,
        })
    }
}

#[inline]
pub fn resizable_max_byte_length(addr: usize) -> Option<u32> {
    resizable_info(addr).map(|i| i.max_byte_length)
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

#[inline]
pub fn is_data_view(addr: usize) -> bool {
    buffer_family_type(addr) == Some(GC_TYPE_BUFFER_DATA_VIEW)
}

/// A freshly allocated buffer cell at `addr`. The brand is already in its
/// header (`buffer_alloc` births a Node `Buffer`); this only clears what a
/// previous occupant of the address could have left in the attribute tables
/// (belt and suspenders: the finalize hook drops them when a cell dies).
pub fn register_buffer(_ptr: *const BufferHeader) {
    // A fresh cell at a reused address must never inherit the previous
    // occupant's inline-admission entry (#9342).
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
/// process-global), a `DataView` or a key object? The byte-cell admission
/// ([`byte_cell_type`]); there is no registry behind it.
#[inline]
pub fn is_registered_buffer(addr: usize) -> bool {
    buffer_family_type(addr).is_some()
}

/// A byte cell proven by its allocator ([`byte_cell_type`]). Generic object
/// paths ask this of every receiver; any other type byte answers in one load.
#[inline]
pub(crate) fn is_owned_byte_cell(addr: usize) -> bool {
    byte_cell_type(addr).is_some()
}

/// Forget process-wide facts about addresses inside arena blocks a dying
/// thread is giving back (#11463): the emitted-code byte admission cache and
/// the external CryptoKey metadata, both of which outlive the thread's TLS.
fn release_external_buffer_registries_in_freed_ranges(
    freed: &crate::arena::thread_exit::FreedRanges,
) {
    use std::sync::PoisonError;
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

#[inline]
pub fn is_secret_key(addr: usize) -> bool {
    buffer_family_type(addr) == Some(GC_TYPE_BUFFER_SECRET_KEY)
}

pub fn set_crypto_key_meta(addr: usize, algo: u8, hash: u8, kind: u8) {
    set_crypto_key_meta_with_flags(
        addr,
        algo,
        hash,
        kind,
        true,
        default_crypto_key_usages(algo, kind),
        0,
    );
}

pub fn set_crypto_key_meta_with_flags(
    addr: usize,
    algo: u8,
    hash: u8,
    kind: u8,
    extractable: bool,
    usages: u32,
    bit_length: u32,
) {
    if buffer_family_type(addr) != Some(GC_TYPE_BUFFER_CRYPTO_KEY) {
        return;
    }
    CRYPTO_KEY_META_REGISTRY.with(|r| {
        r.borrow_mut()
            .insert(addr, (algo, hash, kind, extractable, usages, bit_length));
    });
}

/// Attach metadata to a CryptoKey born by perry-stdlib's WebCrypto, possibly
/// on another thread. The brand is already final; the metadata also goes to the
/// process-global table because the creating thread's table is not the
/// reader's.
#[no_mangle]
pub extern "C" fn js_buffer_set_crypto_key_meta_external(
    addr: usize,
    algo: u8,
    hash: u8,
    kind: u8,
    extractable: u8,
    usages: u32,
    bit_length: u32,
) {
    register_thread_exit_hook();
    if buffer_family_type(addr) != Some(GC_TYPE_BUFFER_CRYPTO_KEY) {
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

/// Is `addr` Uint8Array-backed storage whose JS value is not a Node
/// `Buffer` — a plain `Uint8Array`, or a secret `KeyObject`'s / `CryptoKey`'s
/// key bytes? Reached from `typedarray_props::typed_array_owner_kind` for every
/// untyped element access: one header load and a compare.
#[inline]
pub fn is_uint8array_buffer(addr: usize) -> bool {
    buffer_family_type(addr).is_some_and(is_uint8array_buffer_type)
}

/// Materialize one real ArrayBuffer view, retained by the owner's shaped bag.
pub fn ensure_buffer_ab_alias(addr: usize) -> usize {
    if addr == 0 {
        return 0;
    }
    let _suppress = crate::gc::GcSuppressScope::new();
    unsafe {
        let owner = super::super::store::owner(addr);
        let ty = (*super::super::store::header(owner)).obj_type;
        if ty == GC_TYPE_BUFFER_ARRAY_BUFFER || ty == GC_TYPE_BUFFER_SHARED_ARRAY_BUFFER {
            return owner;
        }
        if let Some(value) =
            super::super::store::bag_get(owner, super::super::store::ARRAY_BUFFER_KEY)
        {
            return crate::value::JSValue::from_bits(value.to_bits()).as_pointer::<u8>() as usize;
        }
        let view = super::super::store::new_view(GC_TYPE_BUFFER_ARRAY_BUFFER, owner, 0, 0, true);
        super::super::store::bag_set(
            owner,
            super::super::store::ARRAY_BUFFER_KEY,
            crate::value::js_nanbox_pointer(view as i64),
            true,
        );
        view as usize
    }
}

pub fn buffer_backing_array_buffer(buf: usize) -> usize {
    let backing = super::super::view::backing_of(buf);
    ensure_buffer_ab_alias(backing)
}

pub fn buffer_byte_offset(buf: usize) -> u32 {
    super::super::view::byte_offset_of(buf)
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
    let ptr = super::super::store::store_alloc(
        crate::gc::GC_TYPE_BUFFER,
        capacity,
        super::super::store::Init::Uninit,
    );
    unsafe {
        (*ptr).length = 0;
    }
    ptr
}

pub(crate) fn alloc_inline(brand: u8, capacity: u32, length: u32) -> *mut BufferHeader {
    let ptr = crate::arena::arena_alloc_gc_old(buffer_payload_size(capacity as usize), 8, brand)
        as *mut BufferHeader;
    unsafe {
        (*super::super::store::header(ptr as usize)).gc_flags |= crate::gc::GC_FLAG_TENURED;
        (*ptr).length = length;
        (*ptr).capacity = capacity;
        (*ptr).link = 0;
    }
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
    super::super::store::store_alloc(
        crate::gc::GC_TYPE_BUFFER,
        length,
        super::super::store::Init::Foreign(data),
    )
}

pub(crate) fn alloc_foreign(brand: u8, data: *mut u8, length: u32) -> *mut BufferHeader {
    super::super::bytes::assert_allocation_allowed();
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
    let ptr = crate::arena::arena_alloc_gc_old(std::mem::size_of::<ForeignBuffer>(), 8, brand)
        as *mut ForeignBuffer;
    unsafe {
        let gc = (ptr as *mut u8).sub(crate::gc::GC_HEADER_SIZE) as *mut crate::gc::GcHeader;
        (*gc).gc_flags |= crate::gc::GC_FLAG_TENURED;
        (*gc)._reserved |= crate::gc::GC_BUFFER_FOREIGN_DATA;
        (*ptr).header.length = length;
        (*ptr).header.capacity = length;
        (*ptr).header.link = 0;
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
    // NativeArena has explicit Native provenance, even for small reservations.
    let capacity = crate::object::shape_rule3::checked_plus_four_word(
        capacity,
        b"Array buffer allocation failed",
    );
    assert!(length <= capacity);
    super::super::store::store_alloc(
        crate::gc::GC_TYPE_BUFFER,
        length,
        super::super::store::Init::AdoptBacking(super::super::backing::Backing::zeroed(capacity)),
    )
}

pub(crate) fn buffer_adopt_backing(
    backing: super::super::backing::Backing,
    length: u32,
) -> *mut BufferHeader {
    super::super::store::store_alloc(
        crate::gc::GC_TYPE_BUFFER_ARRAY_BUFFER,
        length,
        super::super::store::Init::AdoptBacking(backing),
    )
}

pub(crate) fn alloc_backing(
    brand: u8,
    backing: super::super::backing::Backing,
    length: u32,
) -> *mut BufferHeader {
    assert!(length <= backing.capacity());
    let capacity = backing.capacity();
    let ptr = alloc_foreign(brand, backing.data(), length);
    unsafe {
        (*ptr).capacity = capacity;
        (*(ptr as *mut ForeignBuffer)).owned = Some(backing);
    }
    crate::gc::gc_note_external_side_alloc(capacity as usize);
    ptr
}

/// Whether this foreign-shaped cell owns bytes whose release Perry controls.
pub(crate) fn has_owned_backing(addr: usize) -> bool {
    is_foreign_backed_buffer(addr) && unsafe { (*(addr as *const ForeignBuffer)).owned.is_some() }
}

pub(crate) fn take_owned_backing(addr: usize) -> Option<super::super::backing::Backing> {
    #[cfg(test)]
    let defer = !super::super::bytes::sabotage("detach_free")
        && !super::super::bytes::sabotage("arena_free");
    #[cfg(not(test))]
    let defer = true;
    if !is_foreign_backed_buffer(addr) || (defer && super::super::bytes::has_pins(addr)) {
        return None;
    }
    let backing = unsafe {
        let cell = addr as *mut ForeignBuffer;
        let backing = (*cell).owned.take();
        if backing.is_some() {
            (*cell).data = std::ptr::null_mut();
        }
        backing
    };
    if let Some(ref backing) = backing {
        crate::gc::gc_note_external_side_free(backing.capacity() as usize);
    }
    backing
}

/// TLS destruction cannot consult ownership tables or GC accounting. Both
/// the normal finalizer and this path take the same in-cell owner exactly once.
pub(crate) unsafe fn drop_owned_backing_at_thread_exit(header: *mut crate::gc::GcHeader) {
    if crate::gc::is_byte_family_type((*header).obj_type)
        && !crate::gc::is_byte_view_type((*header).obj_type)
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
    owned: Option<super::super::backing::Backing>,
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

pub(crate) fn is_foreign_backed_buffer(addr: usize) -> bool {
    // This public-address probe must prove ownership before reading the header.
    // SAB is a buffer too, but cannot have this per-heap foreign-data layout.
    unsafe { crate::value::addr_class::try_read_tracked_gc_header(addr) }.is_some_and(|header| {
        let header = unsafe { header.as_ref() };
        crate::gc::is_byte_family_type(header.obj_type)
            && !crate::gc::is_byte_view_type(header.obj_type)
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
    CRYPTO_KEY_META_REGISTRY.with(|r| {
        r.borrow_mut().remove(&addr);
    });
    // `js_buffer_set_crypto_key_meta_external` also writes the process-global
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
}

/// Get the canonical data pointer for a buffer or shared view.
pub fn buffer_data(buf: *const BufferHeader) -> *const u8 {
    unsafe { super::super::store::data(buf as usize) }
}

/// Get the mutable data pointer for a buffer
pub fn buffer_data_mut(buf: *mut BufferHeader) -> *mut u8 {
    unsafe { super::super::store::data(buf as usize) }
}

/// Test-only recreation of the deleted byte-moving attach transition.
#[cfg(test)]
pub(crate) unsafe fn externalize_on_attach_for_test(addr: usize) {
    if super::super::store::is_view(addr) || is_foreign_backed_buffer(addr) {
        return;
    }
    let h = super::super::store::header(addr);
    assert!((*h).size as usize >= crate::gc::GC_HEADER_SIZE + std::mem::size_of::<ForeignBuffer>());
    let old = &*(addr as *const BufferHeader);
    let header = BufferHeader {
        length: old.length,
        capacity: old.capacity,
        link: old.link,
    };
    let backing = super::super::backing::Backing::copy(
        super::super::store::owner_data(addr),
        header.capacity,
    );
    let data = backing.data();
    std::ptr::write(
        addr as *mut ForeignBuffer,
        ForeignBuffer {
            header,
            data,
            owned: Some(backing),
            #[cfg(feature = "node-api-host")]
            finalizer: None,
        },
    );
    (*h)._reserved |= crate::codegen_abi::BYTES_OUT_OF_LINE;
}
