use super::*;

/// Type ID constant for Buffer/Uint8Array - matches class_id 0xFFFF0004
pub const BUFFER_TYPE_ID: u32 = 0xFFFF0004;

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

/// Live entry counts for the two registries the GC buffer sweep prunes (#6337).
/// Test-only: the leak regression asserts these DRAIN after the owning buffers
/// are collected, which a per-address `is_*` probe cannot show.
#[cfg(test)]
pub(crate) fn test_data_view_registry_len() -> usize {
    DATA_VIEW_REGISTRY.with(|r| r.borrow().len())
}

#[cfg(test)]
pub(crate) fn test_shared_array_buffer_registry_len() -> usize {
    SHARED_ARRAY_BUFFER_REGISTRY.with(|r| r.borrow().len())
}

/// Register a buffer pointer in the thread-local registry
pub fn register_buffer(ptr: *const BufferHeader) {
    // A FRESH buffer must not inherit the own properties of a dead one that
    // happened to sit at the same address (the own-prop table is address-keyed
    // and buffer storage is recycled). mysql2 measures a packet against a
    // zero-length Buffer whose write methods it overrode with no-ops, then
    // allocates the real packet buffer — which lands on the freed mock's
    // address, and without this the no-ops would carry over and the real packet
    // would serialize as all zeros (the MySQL server then times out reading it).
    super::own_props::clear_buffer_own_props(ptr as usize);
    BUFFER_REGISTRY.with(|r| r.borrow_mut().insert(ptr as usize));
}

/// Historical tier boundary, retained for callers that size test fixtures
/// around it. Since the 2026-07-09 audit fix every buffer allocates through
/// the GC old arena (see `buffer_alloc`) — there is no slab tier anymore.
pub const SMALL_BUF_THRESHOLD: u32 = 256;

/// The small-buffer slab allocator is gone (2026-07-09 audit): slab
/// allocations carried no GcHeader, were never freed, and were invisible to
/// every GC trigger. Every buffer now has a real header in the old arena.
/// `addr_class::try_read_gc_header` still consults this probe; no slab
/// ranges can exist, so it is constant `false`.
pub(crate) fn is_small_buf_slab_addr(_addr: usize) -> bool {
    false
}

/// Check if a pointer is a registered buffer (for instanceof Uint8Array)
pub fn is_registered_buffer(addr: usize) -> bool {
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

/// Post-trace registry pruning (mirrors the #6010 Map/Set pattern): collect
/// registered buffers whose header is genuinely dead so the sweep subphase
/// can drop their side-table state. All buffers are TENURED old-arena
/// residents, and minor traces never mark the old generation — deadness is
/// only trustworthy after a FULL trace.
pub(crate) fn collect_dead_registered_buffers_post_trace(full_trace: bool) -> Vec<usize> {
    if !full_trace {
        return Vec::new();
    }
    // Lock the process-global SAB registry ONCE for the whole scan rather than
    // once per registered buffer (see `registered_buffer_is_dead_post_trace`).
    // `None` — nearly every process — means no SAB was ever allocated.
    let shared_sabs = crate::shared_sab::snapshot_shared_sabs();
    BUFFER_REGISTRY.with(|r| {
        r.borrow()
            .iter()
            .copied()
            .filter(|&addr| unsafe {
                registered_buffer_is_dead_post_trace(addr, shared_sabs.as_ref())
            })
            .collect()
    })
}

unsafe fn registered_buffer_is_dead_post_trace(
    addr: usize,
    shared_sabs: Option<&std::collections::HashSet<usize>>,
) -> bool {
    // A process-global `SharedArrayBuffer` backing is NOT a GC allocation:
    // `shared_sab::alloc_shared_sab` takes it straight from `alloc_zeroed`, it
    // carries no `GcHeader`, and it is never freed (#4913 — that is what lets
    // the same bytes alias across `perry/thread` agents). But
    // `js_shared_array_buffer_new` DOES `register_buffer` it, so it lands in
    // `BUFFER_REGISTRY` and reaches this scan on every full trace.
    //
    // `try_read_gc_header` below would then read the 8 bytes BEFORE the malloc
    // block — the allocator's own metadata — and interpret them as a `GcHeader`:
    // one arbitrary byte compared against `GC_TYPE_BUFFER` (10), the next
    // against the mark/pin/forward bits. A chance match declares a LIVE,
    // never-freed SAB dead, and `finalize_collected_dead_buffer` then runs on
    // it — including `view::remove_entries_for_dead_buffer`, which retains on
    // `info.backing != addr` and so unregisters EVERY live typed-array view
    // over that SAB. Those views are exactly how cross-agent `Atomics`
    // wait/notify resolve their absolute slot addresses.
    //
    // So: veto first, and never sniff a header the object does not have. The
    // set is snapshotted once per scan by the caller and is `None` for the
    // processes that never allocate a SAB — nearly all of them — so the common
    // path here is a single null check.
    if shared_sabs.is_some_and(|sabs| sabs.contains(&addr)) {
        return false;
    }
    let Some(header) = crate::value::addr_class::try_read_gc_header(addr) else {
        return false;
    };
    if header.obj_type != crate::gc::GC_TYPE_BUFFER {
        return false;
    }
    header.gc_flags
        & (crate::gc::GC_FLAG_MARKED | crate::gc::GC_FLAG_PINNED | crate::gc::GC_FLAG_FORWARDED)
        == 0
}

/// Drop every registry/side-table entry keyed by a dead buffer's address.
/// Without this, the recycled address inherits buffer identity
/// (`is_registered_buffer`/`is_array_buffer` misclassify the next tenant —
/// the #6080 ABA class) and the entries leak forever.
pub(crate) fn finalize_collected_dead_buffer(addr: usize) {
    BUFFER_REGISTRY.with(|r| {
        r.borrow_mut().remove(&addr);
    });
    ARRAY_BUFFER_REGISTRY.with(|r| {
        r.borrow_mut().remove(&addr);
    });
    // #6337: the two sibling buffer-identity registries were missing from this
    // list — they had no `.remove`/`.retain` site anywhere in the tree. Like
    // the three above they are plain address-keyed sets that never rooted the
    // `BufferHeader`, so a collected view left its entry behind forever:
    //
    //  * an unbounded leak — one permanent entry per `DataView` (and per
    //    SAB-flagged buffer) ever created;
    //  * the #6080 ABA class this function exists to prevent —
    //    `arena_reset_empty_blocks` resets a fully-empty block's offset to 0
    //    while KEEPING its base pointer, so a reset block re-issues the same
    //    addresses. A recycled address then inherits the dead view's identity:
    //    `is_data_view`/`is_shared_array_buffer` gate `util.types.isDataView`/
    //    `isSharedArrayBuffer`, `ArrayBuffer.isView`, the `[object DataView]`
    //    tag, and the structuredClone/`.slice()` re-marking above — an
    //    unrelated fresh Buffer landing there would answer to all of them.
    //
    // Only GC-heap buffers reach here. A process-global SAB backing is never
    // freed and is vetoed as a dead candidate in
    // `registered_buffer_is_dead_post_trace`, so the entries pruned from
    // SHARED_ARRAY_BUFFER_REGISTRY are the arena-allocated SAB-flagged copies
    // (`SharedArrayBuffer.prototype.slice`, structuredClone) — the ones that
    // genuinely die and whose addresses genuinely get recycled.
    SHARED_ARRAY_BUFFER_REGISTRY.with(|r| {
        r.borrow_mut().remove(&addr);
    });
    DATA_VIEW_REGISTRY.with(|r| {
        r.borrow_mut().remove(&addr);
    });
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
    SECRET_KEY_REGISTRY.with(|r| {
        r.borrow_mut().remove(&addr);
    });
    UINT8ARRAY_FROM_CTOR.with(|r| {
        r.borrow_mut().remove(&addr);
    });
    // `js_buffer_mark_as_crypto_key_external` writes all three global maps, and
    // `is_registered_buffer`/`is_uint8array_buffer` consult them, so a dead
    // external key buffer has to be dropped from every one of them.
    if let Ok(mut r) = external_buffers().lock() {
        r.remove(&addr);
    }
    if let Ok(mut r) = external_uint8arrays().lock() {
        r.remove(&addr);
    }
    if let Ok(mut r) = external_crypto_keys().lock() {
        r.remove(&addr);
    }
    // perry-stdlib keeps its own `addr -> CryptoKeyMaterial` map (the primary
    // one `lookup_crypto_key` consults; the runtime table above is only its
    // fallback), and this crate cannot call into perry-stdlib. Notify it
    // through the hook it installs at startup. The callback only removes a
    // HashMap entry — no allocation, so it is safe to run inside the sweep.
    notify_crypto_key_death(addr);
    super::detach::remove_detached_entry_for_dead_buffer(addr);
    super::view::remove_entries_for_dead_buffer(addr);
}

/// Get the data pointer for a buffer
pub fn buffer_data(buf: *const BufferHeader) -> *const u8 {
    unsafe { (buf as *const u8).add(std::mem::size_of::<BufferHeader>()) }
}

/// Get the mutable data pointer for a buffer
pub fn buffer_data_mut(buf: *mut BufferHeader) -> *mut u8 {
    unsafe { (buf as *mut u8).add(std::mem::size_of::<BufferHeader>()) }
}
