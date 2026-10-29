/// GC header prepended to every heap allocation.
/// Callers receive a pointer AFTER this header (ptr + 8).
#[repr(C)]
pub struct GcHeader {
    /// GC_TYPE_ARRAY, GC_TYPE_STRING, etc.
    pub obj_type: u8,
    /// GC_FLAG_MARKED | GC_FLAG_ARENA | GC_FLAG_PINNED
    pub gc_flags: u8,
    /// Reserved for future use
    pub _reserved: u16,
    /// Total allocation size (header + payload) for arena block walking
    pub size: u32,
}

impl GcHeader {
    /// One load of kind and creation-time object facts. Header alignment is
    /// u32 and these fields occupy its first four bytes on every target.
    #[inline(always)]
    pub(crate) fn kind_facts_word(&self) -> u32 {
        unsafe { std::ptr::read((self as *const Self).cast::<u32>()) }
    }
}

/// [[Call]] for OBJECT and PROXY. Set with the class shape at birth, or with
/// the proxy target at creation; never changed by properties or revocation.
pub(crate) const GC_OBJECT_CALLABLE: u16 = perry_abi::GC_OBJECT_CALLABLE;
const _: () = assert!(std::mem::offset_of!(GcHeader, _reserved) == 2);
const _: () = assert!(GC_TYPE_OBJECT == perry_abi::GC_TYPE_OBJECT);
const _: () = assert!(GC_TYPE_PROXY == perry_abi::GC_TYPE_PROXY);
const _: () = assert!(GC_TYPE_SYMBOL == perry_abi::GC_TYPE_SYMBOL);
const _: () = assert!(GC_TYPE_STRING == perry_abi::GC_TYPE_STRING);

pub const GC_HEADER_SIZE: usize = std::mem::size_of::<GcHeader>(); // 8 bytes

/// The value stored in [`GcHeader::size`]: the exact allocation total, which
/// must fit the field's `u32`.
///
/// Arena walking and every size-derived address (an array's front offset,
/// `array::storage::array_physical_capacity`) read this word back, so a
/// truncated size is a wrong address, not a lost statistic. A kind whose size
/// follows from a script-controlled count rejects an oversized request with its
/// own JS error before it gets here (an array's capacity:
/// `array::alloc::ARRAY_MAX_CAPACITY`, `RangeError: Invalid array length`).
/// Reaching the panic means an allocation funnel skipped that check.
#[inline]
pub fn gc_header_size_word(total: usize) -> u32 {
    match u32::try_from(total) {
        Ok(size) => size,
        Err(_) => gc_header_size_overflow(total),
    }
}

#[cold]
#[inline(never)]
fn gc_header_size_overflow(total: usize) -> ! {
    panic!(
        "gc: an allocation of {total} bytes does not fit GcHeader.size (u32); \
         its allocation funnel must reject the request first"
    )
}

// Object type constants
pub const GC_TYPE_ARRAY: u8 = 1;
pub const GC_TYPE_OBJECT: u8 = 2;
pub const GC_TYPE_STRING: u8 = 3;
pub const GC_TYPE_CLOSURE: u8 = 4;
const _: () = assert!(GC_TYPE_CLOSURE == crate::codegen_abi::GC_TYPE_CLOSURE);
pub const GC_TYPE_PROMISE: u8 = 5;
pub const GC_TYPE_BIGINT: u8 = 6;
pub const GC_TYPE_ERROR: u8 = 7;
pub const GC_TYPE_MAP: u8 = 8;
/// Issue #179 Step 2 Phase 2: lazy JSON-parse top-level array.
/// Arena-allocated, same fast-alloc path as regular arrays.
/// `js_array_length` and `js_json_stringify` recognize this type and
/// serve reads / stringify directly from the tape + blob bytes
/// without materializing the tree. Any other accessor
/// force-materializes (mutates the header's `materialized` field so
/// subsequent accesses hit the tree).
pub const GC_TYPE_LAZY_ARRAY: u8 = 9;
pub const GC_TYPE_BUFFER: u8 = 0x4c;
const _: () = assert!(GC_TYPE_BUFFER == crate::codegen_abi::GC_TYPE_BUFFER);
/// Representative typed-array tag (Uint8). Admission uses `is_typed_array_type`.
pub const GC_TYPE_TYPED_ARRAY: u8 = crate::codegen_abi::BYTES_TYPE_BASE;
pub const GC_TYPE_SET: u8 = 12;
pub const GC_TYPE_NATIVE_ARENA_OWNER: u8 = 0x52;
pub const GC_TYPE_NATIVE_TYPED_VIEW: u8 = 0x60;
pub const GC_TYPE_NATIVE_HANDLE: u8 = 15;
pub const GC_TYPE_NATIVE_POD_VIEW: u8 = 16;
/// A 1-slot mutable `Date` cell (`DateCell { ts: f64 }`). Arena-allocated,
/// non-movable (so a NaN-boxed pointer held in a plain f64/DOUBLE local
/// never goes stale across a copying GC), pointer-free (the `ts` slot is a
/// raw IEEE double, not a JSValue). Gives `Date` reference semantics so
/// setter mutations propagate through aliasing / function / closure
/// boundaries (#2089).
pub const GC_TYPE_DATE_CELL: u8 = 17;
/// A `Temporal.*` cell (`TemporalCell { kind, value }`) wrapping a `temporal_rs`
/// value (Duration / Instant / PlainDate / …). One shared tag with an internal
/// `TemporalKind` discriminator rather than 9 separate tags (#4687).
/// Arena-allocated, non-movable (a NaN-boxed pointer held in a plain f64/DOUBLE
/// local stays valid across GC), and `pointer_free` from the GC's view — the
/// embedded `temporal_rs` value holds plain integers/`'static` calendar data,
/// never a JSValue. Heap-owning variants (a `ZonedDateTime`'s IANA timezone
/// string) are released by the `TemporalCleanup` finalize hook on sweep.
pub const GC_TYPE_TEMPORAL: u8 = 18;
/// #6759 Phase B: a per-object [`crate::object::ObjectMeta`] record — the
/// self-describing metadata a shaped `GC_TYPE_OBJECT` reaches through its
/// `ObjectHeader::meta` slot (custom `[[Prototype]]` today; descriptor
/// tables and exotic kind as later tranches migrate). Reachable ONLY via
/// its owner's header slot, so ordinary tracing gives it exactly the
/// owner's lifetime; movable, and holds one traced NaN-box slot.
pub const GC_TYPE_OBJECT_META: u8 = 19;
/// Native `RegExpHeader`. RegExp used to share `GC_TYPE_OBJECT`, forcing every
/// ObjectHeader consumer to inspect unrelated payload words for a magic value.
/// A distinct GC kind is the authoritative, header-external discriminator.
pub const GC_TYPE_REGEXP: u8 = 20;
/// Immutable Perex program words. All operands are integers/relative offsets;
/// the allocation is a movable leaf reached through its RegExp owner.
pub const GC_TYPE_REGEX_PROGRAM: u8 = 21;
/// The lazy automaton's storage for one Perex program: plain `u32` words only
/// `perex::dfa` reads and writes, reached through its program cell's one traced
/// edge. Allocated old at the program's first automaton search; a leaf.
pub const GC_TYPE_REGEX_CACHE: u8 = 41;
pub const GC_TYPE_BOX: u8 = 22;
const _: () = assert!(GC_TYPE_BOX == perry_abi::GC_TYPE_BOX);
pub const GC_TYPE_I32_BOX: u8 = 23;
pub const GC_TYPE_BOOL_BOX: u8 = 24;
/// A scope context object: N NaN-boxed binding slots shared by the closures of
/// one lexical scope activation (`box/scope.rs`).
pub const GC_TYPE_SCOPE: u8 = 25;

// Buffer-family flavors (#10694). Every one of these is a `BufferHeader` cell
// with exactly `GC_TYPE_BUFFER`'s layout, tracing, size and lifetime; the type
// byte is the brand. It used to be membership in ten address-keyed side tables
// (`BUFFER_REGISTRY`, `ARRAY_BUFFER_REGISTRY`, `UINT8ARRAY_FROM_CTOR`, ...),
// which every "is this a Buffer?" question paid a latch, an address window, a
// thread-local and a hash for: 79.7M probes on a natively compiled `tsc` for 9
// live buffers. Now the question is the header load every brand probe in the
// runtime already does. `GcHeader::_reserved` was not an option (no free bit
// survives the layout machinery), `BufferHeader` has no spare word (codegen
// fixes its offsets), and a buffer is not a `GC_TYPE_OBJECT`, so it has no
// class id to carry one.
//
// `GC_TYPE_BUFFER` itself is the Node `Buffer` flavor. `store_alloc` births
// every byte cell with its final brand; the brand never changes.
// Keep the block contiguous:
// `is_buffer_family_type` is one equality and one range compare.
/// A `Uint8Array` stored as a `BufferHeader` (formats as `Uint8Array(n) [...]`).
pub const GC_TYPE_BUFFER_UINT8ARRAY: u8 = 0x40;
const _: () = assert!(GC_TYPE_BUFFER_UINT8ARRAY == crate::codegen_abi::GC_TYPE_BUFFER_UINT8ARRAY);
/// An `ArrayBuffer`.
pub const GC_TYPE_BUFFER_ARRAY_BUFFER: u8 = 0x4e;
/// A `SharedArrayBuffer`, thread-local or a process-global `shared_sab` block.
pub const GC_TYPE_BUFFER_SHARED_ARRAY_BUFFER: u8 = 0x4f;
/// A `DataView`.
pub const GC_TYPE_BUFFER_DATA_VIEW: u8 = 0x4d;
/// A secret `KeyObject` (`crypto.createSecretKey`): Uint8Array storage holding
/// the raw key bytes.
pub const GC_TYPE_BUFFER_SECRET_KEY: u8 = 0x50;
/// A WebCrypto `CryptoKey`: Uint8Array storage holding the key material; its
/// algorithm/usages metadata is `buffer::header::crypto_key_meta`.
pub const GC_TYPE_BUFFER_CRYPTO_KEY: u8 = 0x51;
/// Process-lifetime symbols carry a readable leaf header even though their
/// allocation is deliberately outside the collecting heap.
pub const GC_TYPE_SYMBOL: u8 = 32;
/// Object-owned weak identity table; key/value pairs are conditional edges.
pub const GC_TYPE_WEAK_STORAGE: u8 = 33;
/// Nonmoving old-generation context holder; lifetime follows its owner edges.
pub const GC_TYPE_ASYNC_SNAPSHOT: u8 = 34;
/// Collecting nonmoving Map store, with inline traced key/value slots.
pub const GC_TYPE_MAP_STORE: u8 = 35;
/// Collecting nonmoving Set store with inline traced element slots.
pub const GC_TYPE_SET_STORE: u8 = 36;
/// Nonmoving collecting Proxy with object-owned target, handler and props.
pub const GC_TYPE_PROXY: u8 = 37;
/// One native parsed-function Rc, owned by an interpreter closure capture.
pub const GC_TYPE_INTERP_FUNCTION: u8 = 38;
/// Object-owned async-hooks metadata in a collecting nonmoving holder.
pub const GC_TYPE_ASYNC_RESOURCE_STATE: u8 = 39;
pub const GC_TYPE_OVERFLOW: u8 = 40;
pub const GC_TYPE_MAX: u8 = 0x7f;

/// Is `obj_type` a `BufferHeader` cell of any flavor (Buffer, Uint8Array,
/// ArrayBuffer, SharedArrayBuffer, DataView, KeyObject, CryptoKey)?
#[inline(always)]
pub const fn is_buffer_family_type(obj_type: u8) -> bool {
    matches!(obj_type & !0x20, 0x40 | 0x4c..=0x52)
}

/// Is `obj_type` a `BufferHeader` cell whose JS value is a `Uint8Array`
/// instance that is not a Node `Buffer` (a plain `Uint8Array`, a secret
/// `KeyObject`'s or a `CryptoKey`'s storage)?
#[inline(always)]
pub const fn is_uint8array_buffer_type(obj_type: u8) -> bool {
    matches!(obj_type & !0x20, 0x40 | 0x50 | 0x51)
}

#[inline(always)]
pub const fn is_byte_family_type(obj_type: u8) -> bool {
    (obj_type & !0x20).wrapping_sub(0x40) <= 18
}

#[inline(always)]
pub const fn is_byte_view_type(obj_type: u8) -> bool {
    obj_type.wrapping_sub(0x60) <= 18
}

#[inline(always)]
pub const fn is_typed_array_type(obj_type: u8) -> bool {
    (obj_type & !0x20).wrapping_sub(0x40) <= 11
}

pub(super) const MALLOC_KIND_UNKNOWN_INDEX: usize = 0;
pub(super) const MALLOC_KIND_BUCKET_COUNT: usize = GC_TYPE_MAX as usize + 1;

pub const LARGE_OBJECT_THRESHOLD_BYTES: usize = 16 * 1024;

/// The same threshold for an object that can hold POINTERS (`pointer_free ==
/// false`), i.e. every arena type whose payload is traced: arrays, plain
/// objects, closures.
///
/// # Why the two thresholds cannot be the same number
///
/// Crossing the threshold does not merely change where an object is allocated.
/// [`crate::arena::arena_alloc_gc`] births it in the old generation **and
/// stamps `GC_FLAG_TENURED`**, and a minor collection never sweeps old-gen. So
/// the object — and, if it holds pointers, *everything reachable from it* —
/// is immortal until a full mark-sweep runs. The threshold is therefore a
/// trade between two costs:
///
/// * **copy cost**, paid by a young object that survives: one `memcpy`,
///   bounded by the object's own size;
/// * **retention cost**, paid by a born-tenured object that dies: its bytes,
///   held until the next full collection.
///
/// For a `pointer_free` object those two quantities are the *same* quantity,
/// so trading one against the other at 16 KB is a defensible wash. For a
/// pointer-BEARING object the retention is **transitive and unbounded**: the
/// write barrier records old→young edges out of it, and every minor's dirty
/// scan then marks its children live, whether or not anything still refers to
/// the container.
///
/// That is not hypothetical. `gc-handoff/apps/shapes.ts` builds a 2000-element
/// `Node2D[]` per round and drops it. The backing store is
/// `8 + 2048 * 8 + 8 = 16 400` bytes — over the 16 KB line by 16 bytes — so
/// each round's array was born tenured and never reclaimed, and the remembered
/// set re-marked **94 000 then 118 006** slots through arrays no live reference
/// pointed at. Its young-survival ratio read 739‰ and 925‰ while its actual
/// live set was ~3 200 objects, and its two minor collections cost 94 ms of a
/// 139 ms program. Halving the array to 1000 elements — same total work, one
/// step under the line — took survival to 30‰, the remembered-set marks to 0,
/// and the program to 0.07 s.
///
/// 128 KB is V8's `kMaxRegularHeapObjectSize`, which draws exactly this line
/// for exactly this reason. It sits well inside the copier's two structural
/// ceilings — the 1 MB nursery block and `move_young`'s 1 MiB
/// `MAX_YOUNG_MOVE_BYTES` refusal — so a young object admitted by it is always
/// movable, and the worst-case block fragmentation it can cause is 1/8 of one
/// block.
pub const LARGE_POINTER_BEARING_OBJECT_THRESHOLD_BYTES: usize = 128 * 1024;

/// Birth-generation ceiling for JSON-constructed storage (#10123).
///
/// Three quarters of a nursery block. Wide-object storage needs only half
/// (a 50,000-field document is 400 KB), but a record array sized once from the
/// parser's `remaining / 96` estimate does not: a 7.1 MB document of 59,000 rows
/// estimates 74,145 slots, 593 KB. Admitting that single allocation young is
/// what lets a minor reclaim the array and its records together after the
/// document dies. Still inside the 1 MiB `arena::BLOCK_SIZE` and
/// `copying::MAX_YOUNG_MOVE_BYTES`, so anything admitted here stays movable.
/// Past it storage is born old as before.
pub const LARGE_OBJECT_STORAGE_YOUNG_BIRTH_CEILING_BYTES: usize = 768 * 1024;

/// Object types a [`JsonWideBirthScope`] may keep young past the threshold.
pub mod json_wide_birth {
    /// Nothing (the scope is closed).
    pub const NONE: u8 = 0;
    /// A wide object's own property storage.
    pub const OBJECTS: u8 = 1;
    /// A parse shape-keys array.
    pub const KEYS_ARRAY: u8 = 2;
}

crate::perry_thread_local! {
    /// Read ONLY from the cold large-object branch of `arena_alloc_gc` (and its
    /// `ConstructionBatch` twin), never from an allocation hot path.
    static JSON_WIDE_BIRTH_MASK: std::cell::Cell<u8> =
        const { std::cell::Cell::new(json_wide_birth::NONE) };
}

/// Keep a wide JSON document's own storage young past the birth threshold
/// (#10123), for as long as the copier can still move it.
///
/// The threshold's rationale is that a large pointer-bearing object is stamped
/// `GC_FLAG_TENURED` and a minor never sweeps old-gen, so its cost "is not its
/// own bytes, it is every object it can reach, held live through the remembered
/// set by a container nothing refers to any more". A wide document's property
/// storage and its shape-keys array are exactly that container: above 16,384
/// fields each crosses 128 KB, is born tenured, and then holds its whole field
/// or key set live after the document itself is dead. Measured at 64 parses,
/// retained shape-keys arrays / peak RSS: 16,300 fields -> 0 / 29 MiB,
/// 16,500 -> 30 / 98 MiB, 50,000 -> 15 / 177 MiB -- the step landing exactly on
/// the constant.
///
/// **Scoped, and type-masked, on purpose.** Raising the constant globally also
/// moved ordinary ARRAY element storage into the nursery, which those rows do
/// not need and which cost them RSS they could not afford (records_array_8m:scan
/// 442 -> 643 MiB against Node 156 / Bun 166). Only a document's own object
/// storage and its keys array are admitted here; array element storage keeps the
/// flat threshold.
pub struct JsonWideBirthScope(u8);

impl JsonWideBirthScope {
    /// Admit wide-object property storage for the duration of a parse.
    pub fn objects() -> Self {
        Self(JSON_WIDE_BIRTH_MASK.with(|c| c.replace(json_wide_birth::OBJECTS)))
    }

    /// Admit a parse shape-keys array around its single allocation site.
    pub fn keys_array() -> Self {
        Self(JSON_WIDE_BIRTH_MASK.with(|c| c.replace(json_wide_birth::KEYS_ARRAY)))
    }

    /// Admit a JSON-constructed array allocation: a record array sized once
    /// from the parser's estimate (see `ConstructionArray::presized_records`).
    pub fn arrays() -> Self {
        Self(JSON_WIDE_BIRTH_MASK.with(|c| c.replace(json_wide_birth::KEYS_ARRAY)))
    }
}

impl Drop for JsonWideBirthScope {
    fn drop(&mut self) {
        JSON_WIDE_BIRTH_MASK.with(|c| c.set(self.0));
    }
}

/// May an already-oversized allocation still be born young?
///
/// Called ONLY once the size test has already said "large", so the thread-local
/// read never touches the allocation hot path. The ceiling is the copier's own
/// refusal point: past it a young object could not be moved, which is the one
/// thing birth-young must not promise falsely.
#[inline]
pub fn json_wide_birth_permits(total_size: usize, obj_type: u8) -> bool {
    if total_size > LARGE_OBJECT_STORAGE_YOUNG_BIRTH_CEILING_BYTES {
        return false;
    }
    let mask = JSON_WIDE_BIRTH_MASK.with(|c| c.get());
    (mask == json_wide_birth::OBJECTS && obj_type == GC_TYPE_OBJECT)
        || (mask == json_wide_birth::KEYS_ARRAY && obj_type == GC_TYPE_ARRAY)
}

#[inline]
pub fn is_large_object_total_size(total_size: usize) -> bool {
    total_size > LARGE_OBJECT_THRESHOLD_BYTES
}

/// The birth-generation threshold for `obj_type`, i.e. the one
/// [`crate::arena::arena_alloc_gc`] applies.
///
/// An unknown type gets the conservative (smaller) threshold: the widened one
/// is justified by the retention argument above, which needs the type table to
/// say the payload is traced.
#[inline]
pub fn large_object_threshold_for_type(obj_type: u8) -> usize {
    // Buffers retain their byte-storage policy: their new traced edges live
    // in side metadata, and buffer_alloc already births every buffer old.
    if is_buffer_family_type(obj_type) {
        return LARGE_OBJECT_THRESHOLD_BYTES;
    }
    // #10123 NOTE: the widening is SCOPED (see `JsonWideBirthScope`), not a
    // blanket change to this constant. A WIDE OBJECT's property storage is the
    // threshold's own rationale warns about -- "every object it can reach, held
    // live through the remembered set by a container nothing refers to any
    // more". Above 16,384 fields it crosses 128 KB, is born tenured, and then
    // holds its whole field set live after the object itself is dead. Measured
    // at 64 parses of one document, retained shape-keys arrays / peak RSS:
    // 16,300 fields -> 0 / 29 MiB, 16,500 -> 30 / 98 MiB, 50,000 -> 15 / 177 MiB;
    // the step lands exactly on the constant.
    //
    // Widened for OBJECT storage ONLY, not for arrays. That is not a guess: on
    // this benchmark matrix `wide_1m` is the only row with a large
    // GC_TYPE_OBJECT birth (400,024 B, once per parse), while every large birth
    // on records_array_8m/20m is GC_TYPE_ARRAY element storage (131 KB - 2 MB).
    // Widening those too bought RSS those rows did not need and cost it
    // elsewhere: records_array_8m:scan 444 -> 710 MiB, 20m:parse 267 -> 314 MiB.
    //
    // Still inside the copier's structural ceilings (1 MB nursery block,
    // `copying::MAX_YOUNG_MOVE_BYTES`), so an object admitted here is movable --
    // the one thing birth-young must not promise falsely.
    match gc_type_info(obj_type) {
        Some(info) if !info.pointer_free => LARGE_POINTER_BEARING_OBJECT_THRESHOLD_BYTES,
        _ => LARGE_OBJECT_THRESHOLD_BYTES,
    }
}

/// Does `total_size` bytes of `obj_type` have to be born in the non-moving old
/// generation?
#[inline]
pub fn is_large_object_total_size_for_type(total_size: usize, obj_type: u8) -> bool {
    total_size > large_object_threshold_for_type(obj_type)
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum GcAllocationPolicy {
    Arena,
    Malloc,
    ArenaOrMalloc,
    RawOrLargeOldArena,
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum GcRewriteDescriptorKind {
    WeakStorage,
    Overflow,
    Box,
    Scope,
    Leaf,
    Array,
    Object,
    RegExp,
    Closure,
    Promise,
    AsyncSnapshot,
    AsyncResourceState,
    Symbol,
    Error,
    Map,
    MapStore,
    LazyArray,
    Set,
    SetStore,
    Proxy,
    Buffer,
    /// #6759 Phase B: one traced NaN-box slot (`ObjectMeta::prototype`).
    ObjectMeta,
    /// #6759 phase 1: a cell whose ONLY traced edge is its metadata record.
    /// `DateCell` was `Leaf` (pointer-free) before it gained a `meta` field.
    MetaOnly,
    /// A Perex program cell: its one traced edge is the raw pointer to its
    /// lazy automaton's storage (`regex::perex_owner::ProgramCell::dfa`).
    RegexProgram,
    /// The optional NaN-boxed back-edge from a payload cell to its owner.
    NativeHandle,
    NativePodView,
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum GcLayoutSlotKind {
    None,
    ArrayElements,
    ObjectFields,
    RegExpFields,
    ClosureCaptures,
    /// #6812: ObjectMeta records carry two live edges — the custom
    /// `[[Prototype]]` value and the raw spill-buffer pointer. Before the
    /// spill buffer these were enumerated only on the REWRITE path, which
    /// left them invisible to marking (latent for prototypes, which are
    /// normally rooted elsewhere; fatal for the spill buffer, which is
    /// reachable through meta alone).
    ObjectMeta,
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum GcExternalBytePolicy {
    None,
    InlinePayload,
    SideAllocation,
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum GcLargeObjectPolicy {
    OldArenaWhenOverThreshold,
    MallocTracked,
    NotApplicable,
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum GcMoveHookKind {
    None,
    /// A Map/Set store's copy takes over the native indexes: re-point its
    /// inline-payload alias and empty the abandoned original.
    OwnedMapStore,
    OwnedSetStore,
    /// An AsyncLocalStorage snapshot holder's copy takes over its vector.
    OwnedAsyncSnapshot,
    MapForeachStack,
    SetForeachStack,
    /// Rekey the Node diagnostic record keyed by the ErrorHeader address after
    /// a move. Errors are movable; without this a moved error loses its
    /// `err.code`/`err.syscall`/`err.errno`/path fields. User-assigned props
    /// live on the Error's traced `ObjectMeta` edge and need no side-table
    /// rekeying.
    ErrorSideTables,
    /// Rekey a lazy JSON array's tape registration. `json_tape_store` keys a
    /// tape by its owner's address, which is precisely what kept
    /// `GC_TYPE_LAZY_ARRAY` immovable and old-gen until this existed.
    LazyArrayTape,
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum GcRewriteHookKind {
    None,
    /// Re-derive the Map/Set header's inline-payload alias from its (possibly
    /// moved) store after a GC pass rewrote the `store` edge.
    OwnedMapEntries,
    OwnedSetElements,
    /// Refresh the owned Map pointer-key index after a GC pass changed key
    /// bits in this Map's entries: object/bigint keys are
    /// indexed by their pointer bits (identity) or pointee content (bigints),
    /// both of which go stale when the referenced allocation is evacuated.
    OwnedMapIndex,
    WeakStorageIndex,
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum GcFinalizeHookKind {
    None,
    InterpFunction,
    OwnedMapStore,
    OwnedSetStore,
    PromiseCleanup,
    AsyncSnapshot,
    AsyncResourceState,
    NativeHandle,
    /// Drop the embedded `temporal_rs` value in a `GC_TYPE_TEMPORAL` cell so a
    /// heap-owning variant (e.g. a `ZonedDateTime` IANA timezone string) is
    /// released when the cell is swept. POD variants drop to a no-op.
    TemporalCleanup,
    /// Drop a swept error's address-keyed Node diagnostic record so a fresh
    /// error allocated at the recycled address doesn't inherit its fields.
    ErrorSideTables,
    /// #7539: free a dead lazy JSON array's tape bytes, which
    /// `json_tape_store` owns outside the GC heap.
    LazyArrayTape,
    /// Release in-cell native backing, addon finalizers and crypto material.
    ByteStore,
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct GcTypeInfo {
    pub(crate) type_id: u8,
    pub(crate) name: &'static str,
    pub(crate) allocation_policy: GcAllocationPolicy,
    pub(crate) arena_walkable: bool,
    pub(crate) rewrite_descriptor_kind: GcRewriteDescriptorKind,
    pub(crate) layout_slot_kind: GcLayoutSlotKind,
    pub(crate) movable: bool,
    pub(crate) external_byte_policy: GcExternalBytePolicy,
    pub(crate) large_object_policy: GcLargeObjectPolicy,
    pub(crate) pointer_free: bool,
    pub(crate) move_hook_kind: GcMoveHookKind,
    pub(crate) rewrite_hook_kind: GcRewriteHookKind,
    pub(crate) finalize_hook_kind: GcFinalizeHookKind,
}

pub(super) const fn gc_type_info_entry(
    type_id: u8,
    name: &'static str,
    allocation_policy: GcAllocationPolicy,
    arena_walkable: bool,
    rewrite_descriptor_kind: GcRewriteDescriptorKind,
    layout_slot_kind: GcLayoutSlotKind,
    movable: bool,
    external_byte_policy: GcExternalBytePolicy,
    large_object_policy: GcLargeObjectPolicy,
    pointer_free: bool,
    move_hook_kind: GcMoveHookKind,
    rewrite_hook_kind: GcRewriteHookKind,
    finalize_hook_kind: GcFinalizeHookKind,
) -> GcTypeInfo {
    GcTypeInfo {
        type_id,
        name,
        allocation_policy,
        arena_walkable,
        rewrite_descriptor_kind,
        layout_slot_kind,
        movable,
        external_byte_policy,
        large_object_policy,
        pointer_free,
        move_hook_kind,
        rewrite_hook_kind,
        finalize_hook_kind,
    }
}

const CORE_TYPE_INFOS: &[Option<GcTypeInfo>] = &[
    None,
    Some(gc_type_info_entry(
        GC_TYPE_ARRAY,
        "array",
        GcAllocationPolicy::Arena,
        true,
        GcRewriteDescriptorKind::Array,
        GcLayoutSlotKind::ArrayElements,
        true,
        GcExternalBytePolicy::InlinePayload,
        GcLargeObjectPolicy::OldArenaWhenOverThreshold,
        false,
        GcMoveHookKind::None,
        GcRewriteHookKind::None,
        GcFinalizeHookKind::None,
    )),
    Some(gc_type_info_entry(
        GC_TYPE_OBJECT,
        "object",
        GcAllocationPolicy::ArenaOrMalloc,
        true,
        GcRewriteDescriptorKind::Object,
        GcLayoutSlotKind::ObjectFields,
        true,
        GcExternalBytePolicy::InlinePayload,
        GcLargeObjectPolicy::OldArenaWhenOverThreshold,
        false,
        GcMoveHookKind::None,
        GcRewriteHookKind::None,
        GcFinalizeHookKind::None,
    )),
    Some(gc_type_info_entry(
        GC_TYPE_STRING,
        "string",
        GcAllocationPolicy::ArenaOrMalloc,
        true,
        GcRewriteDescriptorKind::Leaf,
        GcLayoutSlotKind::None,
        true,
        GcExternalBytePolicy::InlinePayload,
        GcLargeObjectPolicy::OldArenaWhenOverThreshold,
        true,
        GcMoveHookKind::None,
        GcRewriteHookKind::None,
        GcFinalizeHookKind::None,
    )),
    Some(gc_type_info_entry(
        GC_TYPE_CLOSURE,
        "closure",
        GcAllocationPolicy::ArenaOrMalloc,
        true,
        GcRewriteDescriptorKind::Closure,
        GcLayoutSlotKind::ClosureCaptures,
        true,
        GcExternalBytePolicy::InlinePayload,
        GcLargeObjectPolicy::MallocTracked,
        false,
        GcMoveHookKind::None,
        GcRewriteHookKind::None,
        GcFinalizeHookKind::None,
    )),
    Some(gc_type_info_entry(
        GC_TYPE_PROMISE,
        "promise",
        GcAllocationPolicy::ArenaOrMalloc,
        true,
        GcRewriteDescriptorKind::Promise,
        GcLayoutSlotKind::None,
        true,
        GcExternalBytePolicy::None,
        GcLargeObjectPolicy::MallocTracked,
        false,
        // Own properties are traced through the promise metadata edge.
        GcMoveHookKind::None,
        GcRewriteHookKind::None,
        GcFinalizeHookKind::PromiseCleanup,
    )),
    Some(gc_type_info_entry(
        GC_TYPE_BIGINT,
        "bigint",
        GcAllocationPolicy::ArenaOrMalloc,
        true,
        GcRewriteDescriptorKind::Leaf,
        GcLayoutSlotKind::None,
        true,
        GcExternalBytePolicy::InlinePayload,
        GcLargeObjectPolicy::OldArenaWhenOverThreshold,
        true,
        GcMoveHookKind::None,
        GcRewriteHookKind::None,
        GcFinalizeHookKind::None,
    )),
    Some(gc_type_info_entry(
        GC_TYPE_ERROR,
        "error",
        GcAllocationPolicy::Arena,
        true,
        GcRewriteDescriptorKind::Error,
        GcLayoutSlotKind::None,
        true,
        GcExternalBytePolicy::None,
        GcLargeObjectPolicy::OldArenaWhenOverThreshold,
        false,
        GcMoveHookKind::ErrorSideTables,
        GcRewriteHookKind::None,
        GcFinalizeHookKind::ErrorSideTables,
    )),
    Some(gc_type_info_entry(
        GC_TYPE_MAP,
        "map",
        GcAllocationPolicy::Arena,
        true,
        GcRewriteDescriptorKind::Map,
        GcLayoutSlotKind::None,
        true,
        GcExternalBytePolicy::None,
        GcLargeObjectPolicy::NotApplicable,
        false,
        GcMoveHookKind::MapForeachStack,
        GcRewriteHookKind::OwnedMapEntries,
        GcFinalizeHookKind::None,
    )),
    Some(gc_type_info_entry(
        GC_TYPE_LAZY_ARRAY,
        "lazy_array",
        GcAllocationPolicy::Arena,
        true,
        GcRewriteDescriptorKind::LazyArray,
        GcLayoutSlotKind::None,
        // Movable since the tape registration learned to follow its owner
        // (`GcMoveHookKind::LazyArrayTape`) and a header dying in a copying
        // minor's from-space learned to give its tape back
        // (`json_tape_store::finalize_dead_copied_minor_from_space_lazy_tapes`).
        // Those two were the whole reason this was `false`: the registry keys
        // a tape by its owner's address, and the flip runs no finalize hooks.
        //
        // Pinning was not free. A lazy cluster born old is never swept by a
        // minor, so a DEAD one still holds its entire element graph live
        // through the remembered set until a full collection — which on a
        // parse-and-scan loop never arrives. Measured on
        // `records_array_16k:scan`: every minor promoted essentially the whole
        // nursery (`survival_permille=996`, `copied_objects=0`,
        // `freed_bytes=0`) while `old_in_use` climbed past 48 MB, for 205 MiB
        // peak RSS against Node's 62 MiB.
        true,
        // #7539: the tape is a `json_tape_store` side allocation now, not
        // inline payload. Keeping it inline made the header ~2.4 MB on a
        // 10 k-record blob, which `arena_alloc_gc` routed into the old
        // generation with GC_FLAG_TENURED — reclaimable only by a FULL
        // collection, so per-iteration-dead tapes drove `old_gen_bytes`
        // fulls (6 of 9 fulls on the `field_access` fixture).
        GcExternalBytePolicy::SideAllocation,
        GcLargeObjectPolicy::OldArenaWhenOverThreshold,
        false,
        GcMoveHookKind::LazyArrayTape,
        GcRewriteHookKind::None,
        GcFinalizeHookKind::LazyArrayTape,
    )),
    Some(buffer_family_type_info(GC_TYPE_BUFFER, "buffer")),
    Some(gc_type_info_entry(
        GC_TYPE_SET,
        "set",
        GcAllocationPolicy::Arena,
        true,
        GcRewriteDescriptorKind::Set,
        GcLayoutSlotKind::None,
        true,
        GcExternalBytePolicy::None,
        GcLargeObjectPolicy::NotApplicable,
        false,
        GcMoveHookKind::SetForeachStack,
        GcRewriteHookKind::OwnedSetElements,
        GcFinalizeHookKind::None,
    )),
    Some(gc_type_info_entry(
        GC_TYPE_NATIVE_HANDLE,
        "native_handle",
        GcAllocationPolicy::Malloc,
        false,
        GcRewriteDescriptorKind::NativeHandle,
        GcLayoutSlotKind::None,
        false,
        // #11919 P0: a Rust payload reports its native bytes through
        // `gc_note_external_side_alloc` and finalization releases them.
        GcExternalBytePolicy::SideAllocation,
        GcLargeObjectPolicy::MallocTracked,
        false,
        GcMoveHookKind::None,
        GcRewriteHookKind::None,
        GcFinalizeHookKind::NativeHandle,
    )),
    Some(gc_type_info_entry(
        GC_TYPE_NATIVE_POD_VIEW,
        "native_pod_view",
        GcAllocationPolicy::Arena,
        true,
        GcRewriteDescriptorKind::NativePodView,
        GcLayoutSlotKind::None,
        false,
        GcExternalBytePolicy::None,
        GcLargeObjectPolicy::OldArenaWhenOverThreshold,
        false,
        GcMoveHookKind::None,
        GcRewriteHookKind::None,
        GcFinalizeHookKind::None,
    )),
    Some(gc_type_info_entry(
        GC_TYPE_DATE_CELL,
        "date",
        GcAllocationPolicy::Arena,
        true,
        GcRewriteDescriptorKind::MetaOnly,
        GcLayoutSlotKind::None,
        // Movable (#6186, 2026-07-09 GC audit). Directly analogous to
        // `GC_TYPE_PROMISE` above: a pointer-free arena object with
        // a traced metadata edge. The `movable` flag only gates
        // OLD-PAGE DEFRAG (`gc_type_is_movable`, oldgen.rs) — the nursery
        // copied-minor already evacuates eden objects regardless of it — so a
        // promoted, long-lived Date could pin its old-gen page from defrag.
        // Safety: old-page defrag runs only inside MOVING collections (the
        // precise pump-boundary safepoint, where the JS stack is unwound so no
        // un-shadow-rooted Date f64 local is live), the SAME collections at
        // which the nursery already evacuates Dates. Any live reference is a
        // traced heap slot the copy pass rewrites; a Date pointer parked in an
        // un-shadow-rooted local only exists mid-frame, never at a safepoint —
        // the identical invariant that already makes movable Promises sound.
        true,
        GcExternalBytePolicy::None,
        GcLargeObjectPolicy::NotApplicable,
        // pointer_free = FALSE since #6759 phase 1. The cell used to be one
        // raw `f64` and nothing else; it now also carries a `meta` edge, so
        // the collector must scan it. `validate_gc_type_info` enforces this
        // pairing — a pointer-free type may not expose a rewrite descriptor —
        // and caught the flag when it was left at `true`.
        false,
        // Own properties are child edges, requiring no move hook.
        GcMoveHookKind::None,
        GcRewriteHookKind::None,
        GcFinalizeHookKind::None,
    )),
    Some(gc_type_info_entry(
        GC_TYPE_TEMPORAL,
        "temporal",
        GcAllocationPolicy::Arena,
        true,
        GcRewriteDescriptorKind::MetaOnly,
        GcLayoutSlotKind::None,
        // Movable (#6186, completing the Date change from #6214 — the
        // non-movable rationale above it was disproven there): `movable`
        // only gates OLD-PAGE DEFRAG, which runs solely inside moving
        // collections at stack-unwound safepoints; the nursery copied-minor
        // already relocates Temporal cells. The embedded `temporal_rs` value
        // survives memcpy (its owned allocations live on the Rust heap and
        // move by value); from-space bulk resets skip per-object finalizers,
        // so a moved cell is never double-dropped — `TemporalCleanup` fires
        // once, wherever the cell finally dies.
        true,
        GcExternalBytePolicy::None,
        GcLargeObjectPolicy::NotApplicable,
        // Native payload: the embedded `temporal_rs` value is plain integers +
        // `'static` calendar data, never a JSValue. Any Rust-heap it owns is
        // released by the TemporalCleanup finalize hook, not GC tracing.
        false,
        // Own properties are child edges, requiring no move hook.
        GcMoveHookKind::None,
        GcRewriteHookKind::None,
        GcFinalizeHookKind::TemporalCleanup,
    )),
    Some(gc_type_info_entry(
        GC_TYPE_OBJECT_META,
        "object_meta",
        GcAllocationPolicy::Arena,
        true,
        GcRewriteDescriptorKind::ObjectMeta,
        GcLayoutSlotKind::ObjectMeta,
        // Movable: the owner's `meta` header slot is a raw-pointer child
        // edge (visited in the Object rewrite descriptor), so evacuation
        // rewrites it like any other reference — no address-keyed side
        // state exists for this type at all.
        true,
        GcExternalBytePolicy::None,
        GcLargeObjectPolicy::NotApplicable,
        // Holds the traced `prototype` NaN-box slot.
        false,
        GcMoveHookKind::None,
        GcRewriteHookKind::None,
        GcFinalizeHookKind::None,
    )),
    Some(gc_type_info_entry(
        GC_TYPE_REGEXP,
        "regexp_data",
        GcAllocationPolicy::ArenaOrMalloc,
        true,
        GcRewriteDescriptorKind::RegExp,
        GcLayoutSlotKind::RegExpFields,
        true,
        GcExternalBytePolicy::InlinePayload,
        GcLargeObjectPolicy::MallocTracked,
        false,
        GcMoveHookKind::None,
        GcRewriteHookKind::None,
        GcFinalizeHookKind::None,
    )),
    Some(gc_type_info_entry(
        GC_TYPE_REGEX_PROGRAM,
        "regex_program",
        GcAllocationPolicy::Arena,
        true,
        GcRewriteDescriptorKind::RegexProgram,
        GcLayoutSlotKind::None,
        true,
        GcExternalBytePolicy::InlinePayload,
        GcLargeObjectPolicy::OldArenaWhenOverThreshold,
        false,
        GcMoveHookKind::None,
        GcRewriteHookKind::None,
        GcFinalizeHookKind::None,
    )),
    Some(gc_type_info_entry(
        GC_TYPE_REGEX_CACHE,
        "regex_cache",
        GcAllocationPolicy::Arena,
        true,
        GcRewriteDescriptorKind::Leaf,
        GcLayoutSlotKind::None,
        true,
        GcExternalBytePolicy::InlinePayload,
        GcLargeObjectPolicy::OldArenaWhenOverThreshold,
        true,
        GcMoveHookKind::None,
        GcRewriteHookKind::None,
        GcFinalizeHookKind::None,
    )),
    Some(gc_type_info_entry(
        GC_TYPE_BOX,
        "box",
        GcAllocationPolicy::Arena,
        true,
        GcRewriteDescriptorKind::Box,
        GcLayoutSlotKind::None,
        true,
        GcExternalBytePolicy::InlinePayload,
        GcLargeObjectPolicy::OldArenaWhenOverThreshold,
        false,
        GcMoveHookKind::None,
        GcRewriteHookKind::None,
        GcFinalizeHookKind::None,
    )),
    Some(gc_type_info_entry(
        GC_TYPE_I32_BOX,
        "i32_box",
        GcAllocationPolicy::Arena,
        true,
        GcRewriteDescriptorKind::Leaf,
        GcLayoutSlotKind::None,
        true,
        GcExternalBytePolicy::InlinePayload,
        GcLargeObjectPolicy::OldArenaWhenOverThreshold,
        true,
        GcMoveHookKind::None,
        GcRewriteHookKind::None,
        GcFinalizeHookKind::None,
    )),
    Some(gc_type_info_entry(
        GC_TYPE_BOOL_BOX,
        "bool_box",
        GcAllocationPolicy::Arena,
        true,
        GcRewriteDescriptorKind::Leaf,
        GcLayoutSlotKind::None,
        true,
        GcExternalBytePolicy::InlinePayload,
        GcLargeObjectPolicy::OldArenaWhenOverThreshold,
        true,
        GcMoveHookKind::None,
        GcRewriteHookKind::None,
        GcFinalizeHookKind::None,
    )),
    // Indexed by type id: this entry must stay at position GC_TYPE_SCOPE.
    Some(gc_type_info_entry(
        GC_TYPE_SCOPE,
        "scope",
        GcAllocationPolicy::Arena,
        true,
        GcRewriteDescriptorKind::Scope,
        GcLayoutSlotKind::None,
        true,
        GcExternalBytePolicy::InlinePayload,
        GcLargeObjectPolicy::OldArenaWhenOverThreshold,
        false,
        GcMoveHookKind::None,
        GcRewriteHookKind::None,
        GcFinalizeHookKind::None,
    )),
    // Indexed by type id: the buffer-family flavors, GC_TYPE_BUFFER_UINT8ARRAY
    // through GC_TYPE_BUFFER_CRYPTO_KEY, in order.
    Some(buffer_family_type_info(
        GC_TYPE_BUFFER_UINT8ARRAY,
        "buffer_uint8array",
    )),
    Some(buffer_family_type_info(
        GC_TYPE_BUFFER_ARRAY_BUFFER,
        "buffer_array_buffer",
    )),
    Some(buffer_family_type_info(
        GC_TYPE_BUFFER_SHARED_ARRAY_BUFFER,
        "buffer_shared_array_buffer",
    )),
    Some(buffer_family_type_info(
        GC_TYPE_BUFFER_DATA_VIEW,
        "buffer_data_view",
    )),
    Some(buffer_family_type_info(
        GC_TYPE_BUFFER_SECRET_KEY,
        "buffer_secret_key",
    )),
    Some(buffer_family_type_info(
        GC_TYPE_BUFFER_CRYPTO_KEY,
        "buffer_crypto_key",
    )),
    Some(gc_type_info_entry(
        GC_TYPE_SYMBOL,
        "symbol",
        GcAllocationPolicy::Arena,
        true,
        GcRewriteDescriptorKind::Symbol,
        GcLayoutSlotKind::None,
        true,
        GcExternalBytePolicy::InlinePayload,
        GcLargeObjectPolicy::OldArenaWhenOverThreshold,
        false,
        GcMoveHookKind::None,
        GcRewriteHookKind::None,
        GcFinalizeHookKind::None,
    )),
    Some(gc_type_info_entry(
        GC_TYPE_WEAK_STORAGE,
        "weak_storage",
        GcAllocationPolicy::Arena,
        true,
        GcRewriteDescriptorKind::WeakStorage,
        GcLayoutSlotKind::None,
        true,
        GcExternalBytePolicy::InlinePayload,
        GcLargeObjectPolicy::OldArenaWhenOverThreshold,
        false,
        GcMoveHookKind::None,
        GcRewriteHookKind::WeakStorageIndex,
        GcFinalizeHookKind::None,
    )),
    Some(gc_type_info_entry(
        GC_TYPE_ASYNC_SNAPSHOT,
        "async_snapshot",
        GcAllocationPolicy::Arena,
        true,
        GcRewriteDescriptorKind::AsyncSnapshot,
        GcLayoutSlotKind::None,
        true,
        GcExternalBytePolicy::None,
        GcLargeObjectPolicy::NotApplicable,
        false,
        GcMoveHookKind::OwnedAsyncSnapshot,
        GcRewriteHookKind::None,
        GcFinalizeHookKind::AsyncSnapshot,
    )),
    Some(gc_type_info_entry(
        GC_TYPE_ASYNC_RESOURCE_STATE,
        "async_resource_state",
        GcAllocationPolicy::Arena,
        true,
        GcRewriteDescriptorKind::AsyncResourceState,
        GcLayoutSlotKind::None,
        false,
        GcExternalBytePolicy::None,
        GcLargeObjectPolicy::NotApplicable,
        false,
        GcMoveHookKind::None,
        GcRewriteHookKind::None,
        GcFinalizeHookKind::AsyncResourceState,
    )),
    Some(gc_type_info_entry(
        GC_TYPE_OVERFLOW,
        "object_overflow",
        GcAllocationPolicy::Arena,
        true,
        GcRewriteDescriptorKind::Overflow,
        GcLayoutSlotKind::None,
        true,
        GcExternalBytePolicy::None,
        GcLargeObjectPolicy::NotApplicable,
        false,
        GcMoveHookKind::None,
        GcRewriteHookKind::None,
        GcFinalizeHookKind::None,
    )),
    Some(gc_type_info_entry(
        GC_TYPE_SET_STORE,
        "set_store",
        GcAllocationPolicy::Arena,
        true,
        GcRewriteDescriptorKind::SetStore,
        GcLayoutSlotKind::None,
        true,
        GcExternalBytePolicy::SideAllocation,
        GcLargeObjectPolicy::OldArenaWhenOverThreshold,
        false,
        GcMoveHookKind::OwnedSetStore,
        GcRewriteHookKind::None,
        GcFinalizeHookKind::OwnedSetStore,
    )),
    Some(gc_type_info_entry(
        GC_TYPE_MAP_STORE,
        "map_store",
        GcAllocationPolicy::Arena,
        true,
        GcRewriteDescriptorKind::MapStore,
        GcLayoutSlotKind::None,
        true,
        GcExternalBytePolicy::SideAllocation,
        GcLargeObjectPolicy::OldArenaWhenOverThreshold,
        false,
        GcMoveHookKind::OwnedMapStore,
        GcRewriteHookKind::OwnedMapIndex,
        GcFinalizeHookKind::OwnedMapStore,
    )),
    Some(gc_type_info_entry(
        GC_TYPE_INTERP_FUNCTION,
        "interpreted_function",
        GcAllocationPolicy::Malloc,
        false,
        GcRewriteDescriptorKind::Leaf,
        GcLayoutSlotKind::None,
        false,
        GcExternalBytePolicy::None,
        GcLargeObjectPolicy::MallocTracked,
        true,
        GcMoveHookKind::None,
        GcRewriteHookKind::None,
        GcFinalizeHookKind::InterpFunction,
    )),
    Some(gc_type_info_entry(
        GC_TYPE_PROXY,
        "proxy",
        GcAllocationPolicy::Arena,
        true,
        GcRewriteDescriptorKind::Proxy,
        GcLayoutSlotKind::None,
        false,
        GcExternalBytePolicy::None,
        GcLargeObjectPolicy::NotApplicable,
        false,
        GcMoveHookKind::None,
        GcRewriteHookKind::None,
        GcFinalizeHookKind::None,
    )),
];

const INERT_TYPE_INFO: GcTypeInfo = gc_type_info_entry(
    0,
    "unused",
    GcAllocationPolicy::Arena,
    false,
    GcRewriteDescriptorKind::Leaf,
    GcLayoutSlotKind::None,
    false,
    GcExternalBytePolicy::None,
    GcLargeObjectPolicy::NotApplicable,
    true,
    GcMoveHookKind::None,
    GcRewriteHookKind::None,
    GcFinalizeHookKind::None,
);

// Direct descriptor projection must preserve unknown-kind semantics for
// EVERY unused row, including any future catalog changes.
const _: () = {
    let rows = byte_type_infos();
    let mut kind = 0;
    while kind < rows.len() {
        assert!(
            rows[kind].type_id != 0
                || matches!(
                    rows[kind].rewrite_descriptor_kind,
                    GcRewriteDescriptorKind::Leaf
                )
        );
        kind += 1;
    }
};

const fn byte_type_infos() -> [GcTypeInfo; MALLOC_KIND_BUCKET_COUNT] {
    // Unused ids have fully initialized inert fields. A layout note can read
    // its field directly from the sole descriptor authority; admission still
    // rejects the zero type id. No separate layout-kind cache is needed.
    let mut result = [INERT_TYPE_INFO; MALLOC_KIND_BUCKET_COUNT];
    let mut i = 0;
    while i < CORE_TYPE_INFOS.len() {
        if let Some(info) = CORE_TYPE_INFOS[i] {
            if !is_byte_family_type(info.type_id) {
                result[info.type_id as usize] = info;
            }
        }
        i += 1;
    }
    i = 0x40;
    while i <= 0x7f {
        if is_byte_family_type(i as u8) {
            result[i] = buffer_family_type_info(i as u8, "bytes");
        }
        i += 1;
    }
    result
}

pub(super) static GC_TYPE_INFO_BY_ID: [GcTypeInfo; MALLOC_KIND_BUCKET_COUNT] = byte_type_infos();

/// One `GcTypeInfo` for every buffer-family flavor: the flavors differ only in
/// the brand the type byte carries, never in layout, tracing or lifetime.
const fn buffer_family_type_info(type_id: u8, name: &'static str) -> GcTypeInfo {
    gc_type_info_entry(
        type_id,
        name,
        GcAllocationPolicy::RawOrLargeOldArena,
        true,
        GcRewriteDescriptorKind::Buffer,
        GcLayoutSlotKind::None,
        false,
        GcExternalBytePolicy::InlinePayload,
        GcLargeObjectPolicy::OldArenaWhenOverThreshold,
        false,
        GcMoveHookKind::None,
        GcRewriteHookKind::None,
        GcFinalizeHookKind::ByteStore,
    )
}

/// One bit per type id that has a `GcTypeInfo`, derived at compile time from
/// the same descriptor table tracing uses. The type ids are no longer one
/// dense run (core kinds, then the byte-family block at 0x40), so header
/// admission tests a constant bit instead of loading a descriptor. 32 bytes
/// cover every `u8`, so the index needs no bounds check.
static GC_TYPE_KNOWN_BITS: [u8; 32] = {
    let infos = byte_type_infos();
    let mut bits = [0u8; 32];
    let mut kind = 0usize;
    while kind < infos.len() {
        if infos[kind].type_id != 0 {
            bits[kind >> 3] |= 1 << (kind & 7);
        }
        kind += 1;
    }
    bits
};

/// Header admission uses the same metadata as tracing: a type id is known
/// exactly when it has a descriptor.
#[inline(always)]
pub(crate) fn gc_type_is_known(obj_type: u8) -> bool {
    #[cfg(test)]
    if obj_type == 10 && crate::buffer::bytes::b4_sabotage("retired_type_admission") {
        return true;
    }
    GC_TYPE_KNOWN_BITS[(obj_type >> 3) as usize] >> (obj_type & 7) & 1 != 0
}

#[inline]
pub(crate) fn gc_type_info(obj_type: u8) -> Option<&'static GcTypeInfo> {
    #[cfg(test)]
    if obj_type == GC_TYPE_ARRAY
        && header_admission_tests::DENSE_DESCRIPTOR_FAULT.with(std::cell::Cell::get)
    {
        return Some(&GC_TYPE_INFO_BY_ID[GC_TYPE_OBJECT as usize]);
    }
    GC_TYPE_INFO_BY_ID
        .get(obj_type as usize)
        .filter(|info| info.type_id != 0)
}

pub(crate) fn gc_type_infos() -> impl Iterator<Item = &'static GcTypeInfo> {
    GC_TYPE_INFO_BY_ID.iter().filter(|info| info.type_id != 0)
}

#[inline]
pub(crate) fn gc_type_is_arena_walkable(obj_type: u8) -> bool {
    gc_type_info(obj_type).is_some_and(|info| info.arena_walkable)
}

#[inline(always)]
pub(crate) fn gc_type_rewrite_descriptor_kind(obj_type: u8) -> GcRewriteDescriptorKind {
    #[cfg(test)]
    if obj_type == GC_TYPE_ARRAY
        && header_admission_tests::DENSE_DESCRIPTOR_FAULT.with(std::cell::Cell::get)
    {
        return GC_TYPE_INFO_BY_ID[GC_TYPE_OBJECT as usize].rewrite_descriptor_kind;
    }
    // Unused rows are canonical inert descriptors with Leaf rewrite kind,
    // exactly the unknown-kind answer. Project the field from the one table;
    // repeating type admission changes no answer and costs every child trace.
    // Header/pointer admission remains gc_type_is_known's independent job.
    GC_TYPE_INFO_BY_ID
        .get(obj_type as usize)
        .map_or(GcRewriteDescriptorKind::Leaf, |info| {
            info.rewrite_descriptor_kind
        })
}

#[inline]
pub(crate) fn gc_type_layout_slot_kind(obj_type: u8) -> GcLayoutSlotKind {
    #[cfg(test)]
    if obj_type == 10 && header_admission_tests::INERT_LAYOUT_FAULT.with(std::cell::Cell::get) {
        return GcLayoutSlotKind::ArrayElements;
    }
    GC_TYPE_INFO_BY_ID
        .get(obj_type as usize)
        .map_or(GcLayoutSlotKind::None, |info| info.layout_slot_kind)
}

#[inline]
pub(crate) fn gc_type_is_movable(obj_type: u8) -> bool {
    gc_type_info(obj_type).is_some_and(|info| info.movable)
}

// #854: part of GC type-metadata verification contract (exercised by gc/tests)
#[allow(dead_code)]
#[inline]
pub(crate) fn gc_type_external_byte_policy(obj_type: u8) -> GcExternalBytePolicy {
    gc_type_info(obj_type).map_or(GcExternalBytePolicy::None, |info| info.external_byte_policy)
}

// #854: part of GC type-metadata verification contract (exercised by gc/tests)
#[allow(dead_code)]
#[inline]
pub(crate) fn gc_type_large_object_policy(obj_type: u8) -> GcLargeObjectPolicy {
    gc_type_info(obj_type).map_or(GcLargeObjectPolicy::NotApplicable, |info| {
        info.large_object_policy
    })
}

// #854: part of GC type-metadata verification contract (exercised by gc/tests)
#[allow(dead_code)]
#[inline]
pub(crate) fn gc_type_is_pointer_free(obj_type: u8) -> bool {
    gc_type_info(obj_type).is_none_or(|info| info.pointer_free)
}

#[inline]
pub(crate) fn gc_type_rewrite_hook_kind(obj_type: u8) -> GcRewriteHookKind {
    gc_type_info(obj_type).map_or(GcRewriteHookKind::None, |info| info.rewrite_hook_kind)
}

/// Run the post-rewrite hook for `obj_type` after a GC pass changed one or
/// more of the object's reference slots. Shared by every rewrite call site
/// (remembered-set dirty-slot scan, copying field scan, verify/force-evacuate
/// rewrites) so a new hook kind only needs wiring here.
pub(crate) fn run_gc_rewrite_hook(obj_type: u8, user_ptr: usize) {
    match gc_type_rewrite_hook_kind(obj_type) {
        GcRewriteHookKind::None => {}
        GcRewriteHookKind::WeakStorageIndex => unsafe {
            (*(user_ptr as *mut crate::weakref::storage::WeakStorage)).rebuild();
        },
        GcRewriteHookKind::OwnedMapIndex => unsafe {
            crate::map::rebuild_owned_map_ptr_index(user_ptr as *mut u8);
        },
        GcRewriteHookKind::OwnedMapEntries => unsafe {
            crate::map::refresh_entries_alias(user_ptr as *mut crate::map::MapHeader);
        },
        GcRewriteHookKind::OwnedSetElements => unsafe {
            crate::set::refresh_elements_alias(user_ptr as *mut crate::set::SetHeader);
        },
    }
}

pub(crate) fn gc_type_after_payload_move(obj_type: u8, old_user: usize, new_user: usize) {
    match gc_type_info(obj_type).map_or(GcMoveHookKind::None, |info| info.move_hook_kind) {
        GcMoveHookKind::None => {}
        GcMoveHookKind::MapForeachStack => {
            crate::map::map_header_moved_for_gc(old_user, new_user);
        }
        GcMoveHookKind::SetForeachStack => {
            crate::set::set_header_moved_for_gc(old_user, new_user);
        }
        GcMoveHookKind::OwnedMapStore => unsafe {
            crate::map::owned_store_moved_for_gc(old_user, new_user);
        },
        GcMoveHookKind::OwnedSetStore => unsafe {
            crate::set::owned_store_moved_for_gc(old_user, new_user);
        },
        GcMoveHookKind::OwnedAsyncSnapshot => unsafe {
            crate::async_context::snapshot_holder_moved_for_gc(old_user, new_user);
        },
        GcMoveHookKind::ErrorSideTables => {
            crate::node_submodules::diagnostics_gc::error_side_tables_owner_moved(
                old_user, new_user,
            );
        }
        GcMoveHookKind::LazyArrayTape => {
            crate::json_tape_store::owner_moved(old_user, new_user);
        }
    }
}

#[cfg(test)]
pub(crate) fn gc_type_clear_dead_payload_side_tables(obj_type: u8, user_ptr: usize) {
    match gc_type_info(obj_type).map_or(GcMoveHookKind::None, |info| info.move_hook_kind) {
        GcMoveHookKind::ErrorSideTables => {
            crate::node_submodules::diagnostics_gc::error_side_tables_clear_dead(user_ptr);
        }
        GcMoveHookKind::LazyArrayTape => {
            // The tape is released by `GcFinalizeHookKind::LazyArrayTape` and,
            // for a header that dies in a copying minor's from-space, by
            // `finalize_dead_copied_minor_from_space_lazy_tapes`. Releasing it
            // a third time here would be sound (the release is idempotent) but
            // would hide which pass actually owns the reclaim.
        }
        GcMoveHookKind::None
        | GcMoveHookKind::MapForeachStack
        | GcMoveHookKind::SetForeachStack
        | GcMoveHookKind::OwnedMapStore
        | GcMoveHookKind::OwnedSetStore
        | GcMoveHookKind::OwnedAsyncSnapshot => {}
    }
}

pub(crate) unsafe fn gc_type_finalize_unmarked_payload(obj_type: u8, user_ptr: *mut u8) {
    match gc_type_info(obj_type).map_or(GcFinalizeHookKind::None, |info| info.finalize_hook_kind) {
        GcFinalizeHookKind::None => {}
        GcFinalizeHookKind::InterpFunction => {
            // Only the evaluator allocates this kind. Feature-stripped builds
            // cannot contain one and need no dependency on the evaluator.
            #[cfg(feature = "dyn-eval")]
            crate::dyn_eval::function_cell::finalize(user_ptr);
        }
        GcFinalizeHookKind::OwnedMapStore => crate::map::finalize_owned_store(user_ptr, true),
        GcFinalizeHookKind::OwnedSetStore => {
            crate::set::finalize_owned_store(user_ptr, true);
        }
        GcFinalizeHookKind::AsyncSnapshot => {
            crate::async_context::finalize_snapshot_holder(
                user_ptr as *mut crate::async_context::AsyncSnapshotHolder,
            );
        }
        GcFinalizeHookKind::AsyncResourceState => {
            crate::async_hook_slots::resource_state::finalize(user_ptr.cast(), true);
        }
        GcFinalizeHookKind::PromiseCleanup => {
            let promise = user_ptr as *mut crate::promise::Promise;
            crate::async_hook_slots::enqueue_gc_destroy((*promise).async_id);
            crate::promise::clear_promise_context_for_gc(promise);
        }

        GcFinalizeHookKind::NativeHandle => {
            crate::native_handle::finalize_native_handle_for_gc(
                user_ptr as *mut crate::native_handle::NativeHandleHeader,
            );
        }

        GcFinalizeHookKind::TemporalCleanup => {
            crate::temporal::finalize_temporal_cell_for_gc(
                user_ptr as *mut crate::temporal::TemporalCell,
            );
        }
        GcFinalizeHookKind::ErrorSideTables => {
            crate::node_submodules::diagnostics_gc::error_side_tables_clear_dead(user_ptr as usize);
        }

        GcFinalizeHookKind::LazyArrayTape => {
            crate::json_tape_store::release(user_ptr as usize);
        }
        GcFinalizeHookKind::ByteStore => {
            crate::buffer::finalize_collected_dead_buffer(user_ptr as usize);
        }
    }
}

#[cfg(any(perry_diagnostics, test))]
#[inline]
pub(super) fn gc_type_name(obj_type: u8) -> &'static str {
    gc_type_info(obj_type).map_or("unknown", |info| info.name)
}

// #854: part of GC type-metadata verification contract (exercised by gc/tests)
#[allow(dead_code)]
pub(crate) fn validate_gc_type_info(info: &GcTypeInfo) -> Result<(), &'static str> {
    let descriptor_is_leaf = info.rewrite_descriptor_kind == GcRewriteDescriptorKind::Leaf;
    if info.pointer_free {
        if !descriptor_is_leaf {
            return Err("pointer-free GC type exposes a rewrite descriptor");
        }
        if info.layout_slot_kind != GcLayoutSlotKind::None {
            return Err("pointer-free GC type exposes pointer slots");
        }
        return Ok(());
    }

    if descriptor_is_leaf {
        return Err("pointerful GC type lacks rewrite descriptor metadata");
    }

    match info.rewrite_descriptor_kind {
        GcRewriteDescriptorKind::Array => {
            if info.layout_slot_kind != GcLayoutSlotKind::ArrayElements {
                return Err("array rewrite descriptor must expose array element slots");
            }
        }
        GcRewriteDescriptorKind::Object => {
            if info.layout_slot_kind != GcLayoutSlotKind::ObjectFields {
                return Err("object rewrite descriptor must expose object field slots");
            }
        }
        GcRewriteDescriptorKind::RegExp => {
            if info.layout_slot_kind != GcLayoutSlotKind::RegExpFields {
                return Err("regexp rewrite descriptor must expose regexp fields");
            }
        }
        GcRewriteDescriptorKind::Closure => {
            if info.layout_slot_kind != GcLayoutSlotKind::ClosureCaptures {
                return Err("closure rewrite descriptor must expose closure capture slots");
            }
        }
        GcRewriteDescriptorKind::WeakStorage
        | GcRewriteDescriptorKind::Overflow
        | GcRewriteDescriptorKind::Box
        | GcRewriteDescriptorKind::Scope
        | GcRewriteDescriptorKind::Buffer
        | GcRewriteDescriptorKind::MetaOnly
        | GcRewriteDescriptorKind::RegexProgram
        | GcRewriteDescriptorKind::NativeHandle
        | GcRewriteDescriptorKind::Promise
        | GcRewriteDescriptorKind::AsyncSnapshot
        | GcRewriteDescriptorKind::AsyncResourceState
        | GcRewriteDescriptorKind::Symbol
        | GcRewriteDescriptorKind::Error
        | GcRewriteDescriptorKind::Map
        | GcRewriteDescriptorKind::MapStore
        | GcRewriteDescriptorKind::LazyArray
        | GcRewriteDescriptorKind::Set
        | GcRewriteDescriptorKind::SetStore
        | GcRewriteDescriptorKind::Proxy => {
            if info.layout_slot_kind != GcLayoutSlotKind::None {
                return Err(
                    "external-backed rewrite descriptor must not expose payload layout slots",
                );
            }
        }
        GcRewriteDescriptorKind::ObjectMeta => {
            // Meta records expose every child edge (prototype, spill buffer,
            // and private-evaluation brand) to marking and rewriting.
            if info.layout_slot_kind != GcLayoutSlotKind::ObjectMeta {
                return Err("object-meta descriptor must expose its child edges to marking");
            }
        }
        GcRewriteDescriptorKind::NativePodView => {
            if info.layout_slot_kind != GcLayoutSlotKind::None {
                return Err("native view rewrite descriptor must use fixed slots only");
            }
        }
        GcRewriteDescriptorKind::Leaf => unreachable!("leaf handled above"),
    }

    Ok(())
}

// #854: part of GC type-metadata verification contract (exercised by gc/tests)
#[allow(dead_code)]
pub(crate) fn validate_gc_type_metadata() -> Result<(), String> {
    for info in gc_type_infos() {
        validate_gc_type_info(info)
            .map_err(|reason| format!("invalid GC metadata for {}: {}", info.name, reason))?;
    }
    Ok(())
}

// Flag constants
pub const GC_FLAG_MARKED: u8 = 0x01;
pub const GC_FLAG_ARENA: u8 = 0x02;
pub const GC_FLAG_PINNED: u8 = 0x04;
/// Set on a keys-array that was handed out by `shape_cache_insert`.
/// `js_object_set_field_by_name` reads this bit to decide whether it
/// must clone before mutating (shared arrays can't be mutated in
/// place; fresh arrays allocated in the `keys.is_null()` branch can).
/// Without the bit the clone fires on every property added to every
/// fresh object literal — a 20-property row object allocates 19
/// throwaway keys_array clones per row.
/// In old generation this also pins MOVEMENT (I3), including old-page defrag
/// and idle compaction. It does not pin liveness: full GC still sweeps a dead
/// backing. Canonical key lists are born old with this bit; owned key lists
/// remain movable. Key strings and the attributes child retain normal tracing.
/// This bit belongs only to private key/attribute array storage: its words
/// never name foreign proxy, Fetch or pool handles. The collector may use that
/// source-word proof; the referenced children's descriptors remain authoritative.
pub const GC_FLAG_SHAPE_SHARED: u8 = 0x08;
/// Set on strings that live in the intern table. Prevents in-place
/// mutation and allows `js_object_set_field_by_name` to skip the
/// FNV-1a hash (pointer identity is sufficient for interned strings).
pub const GC_FLAG_INTERNED: u8 = 0x10;
/// Gen-GC Phase C4: object has survived at least PROMOTION_AGE
/// minor GCs and is now logically tenured — minor GC trace skips
/// recursion into its fields, exactly like an OLD_ARENA-allocated
/// object. Stored on the GcHeader so the per-object check is one
/// byte load + one bit-and. Non-moving generational: tenured
/// objects stay physically in nursery (no copying / forwarding-
/// pointer machinery), but the trace pretends they're old-gen.
/// True compacting evacuation lands in Phase C4b.
pub const GC_FLAG_TENURED: u8 = 0x20;
/// Gen-GC Phase C4: object has survived at least one minor GC.
/// The non-copying minor path still uses this as its one-bit
/// pre-tenure state; the copied-nursery path stores its exact
/// short age in `_reserved` so loop-carried transients get one
/// extra survivor cycle before old-gen promotion.
pub const GC_FLAG_HAS_SURVIVED: u8 = 0x40;
/// Object's user payload begins with a forwarding address. The new
/// address is stored in the **user-payload's first 8 bytes**
/// (immediately after the GcHeader). Walkers that encounter a
/// FORWARDED header read the forwarding address and follow it;
/// ref-rewrite passes update every NaN-boxed pointer they observe to
/// the forwarded address.
///
/// Two runtime paths use the same bit and payload layout:
/// - GC evacuation/copying stubs are short-lived. Evacuation keeps an
///   explicit list of original nursery headers and clears this bit
///   after owned references have been rewritten/verified, so sweep can
///   reclaim the original slot. Copying nursery stubs disappear when
///   from-space is reset.
/// - Array-growth stubs are intentionally retained. `clean_arr_ptr`
///   follows those chains for stale array references that the runtime
///   cannot rewrite.
///
/// Conservative-stack scans STILL get the old (now-stale) address;
/// objects that might be conservatively referenced are pinned out of
/// the evacuation set via `GC_FLAG_PINNED` to avoid corrupting reads
/// from those words.
///
/// This is the last bit in the u8 gc_flags. Adding more flags
/// requires extending GcHeader (currently 8 bytes total — extending
/// breaks ABI everywhere; deferred until/unless a future phase
/// genuinely needs more bits).
pub const GC_FLAG_FORWARDED: u8 = 0x80;
const _: () = assert!(GC_FLAG_FORWARDED == crate::codegen_abi::GC_FLAG_FORWARDED);
const _: () = assert!(GC_HEADER_SIZE == crate::codegen_abi::GC_HEADER_SIZE);

/// Read the forwarding address embedded in a forwarded object's user
/// payload. Caller must verify `gc_flags & GC_FLAG_FORWARDED` is set;
/// reading otherwise returns garbage. The forwarded address is the
/// **user pointer** of the new location — i.e. what the allocating
/// path returned for the new copy. Callers that need the new GcHeader
/// subtract `GC_HEADER_SIZE` themselves.
///
/// # Safety
/// `header` must point to a valid GcHeader whose user payload is
/// at least 8 bytes (every Perry object's payload is — strings
/// have at least the StringHeader, arrays have ArrayHeader, etc.).
#[inline]
pub unsafe fn forwarding_address(header: *const GcHeader) -> *mut u8 {
    debug_assert!(
        (*header).gc_flags & GC_FLAG_FORWARDED != 0,
        "forwarding_address called on non-forwarded header"
    );
    let user_ptr = (header as *const u8).add(GC_HEADER_SIZE) as *const *mut u8;
    *user_ptr
}

/// Install a forwarding address in an object's user payload and set
/// `GC_FLAG_FORWARDED` on its header. The first 8 bytes of the user
/// payload become the forwarding pointer (the new user address).
/// Subsequent reads via `forwarding_address` recover the new location.
///
/// GC evacuation must later clear this bit only for the originals it
/// just moved. Array growth uses the same representation but leaves the
/// stub retained so stale array references can continue to resolve via
/// `clean_arr_ptr`.
///
/// # Safety
/// As `forwarding_address`. The user payload must be at least 8
/// bytes; this is true for every Perry GC type today.
#[inline]
pub unsafe fn set_forwarding_address(header: *mut GcHeader, new_user_addr: *mut u8) {
    let user_ptr = (header as *mut u8).add(GC_HEADER_SIZE) as *mut *mut u8;
    *user_ptr = new_user_addr;
    (*header).gc_flags |= GC_FLAG_FORWARDED;
}

/// Per-instance builtin constructability. Kind-disjoint from byte-owner
/// and array bit 7; copying age/layout updates preserve this bit.
pub(crate) const GC_CLOSURE_NON_CONSTRUCTABLE: u16 = 0x80;

// Object flags stored in GcHeader._reserved (u16) for Object.freeze/seal/preventExtensions
pub const OBJ_FLAG_FROZEN: u16 = 0x01;
pub const OBJ_FLAG_SEALED: u16 = 0x02;
pub const OBJ_FLAG_NO_EXTEND: u16 = 0x04;
// #1175: object was created with a null prototype (Object.create(null) /
// querystring.parse). `Object.getPrototypeOf` returns null for these.
// Bit 6; bits 3..4 are survival age, bit 5 is callability, and 14..15
// are layout state. See the complete map below before allocating a bit.
pub const OBJ_FLAG_NULL_PROTO: u16 = 0x40;
/// I1 (`gc::never_move`): this `GC_TYPE_OBJECT` never moves once it is in
/// the old generation. The copying minor promotes it at its next survival,
/// and old-page defragmentation and idle compaction never select it. A
/// per-site memo may name only an object that carries this bit and is old
/// (`never_move::stable_memo_referent`), which is what lets the memo hold a
/// plain address with no root, registry or rewrite. Bit 7 is free for
/// objects (`OBJ_FLAG_RESERVED_BIT_MAP_SEE_DOC`); arrays and byte owners give
/// it other meanings, so every reader checks `obj_type` first. Set-only.
pub(crate) const OBJ_FLAG_NEVER_MOVE: u16 = 0x80;
/// I1 companion (`gc::never_move::note_memo_named`): a site memo stores
/// this object's address. If a class relink displaces it from a class
/// identity, the retirement pins it, so the memo keeps reading this object.
/// Bit 13 shares generic layout initialization's ALL_POINTERS bit. The
/// object field iterator selects by shape; memo readers require OBJECT.
/// Layout initialization must finish before this set-only fact is published.
pub(crate) const OBJ_FLAG_MEMO_NAMED: u16 = 0x2000;
/// The GC_TYPE_BUFFER payload contains a native data pointer after BufferHeader.
/// Bit 7 is kind-disjoint from object/array numeric-layout proofs. Generic age
/// and layout transitions preserve it; no Buffer reader interprets those proofs.
pub(crate) const GC_BUFFER_FOREIGN_DATA: u16 = 0x80;
/// A Buffer-family view contains a resolved native data pointer at +8.
/// Kind-disjoint from the Object typed-array-prototype bit. The view's
/// backing edge is traced and the derived pointer is refreshed on rewrite.
// Bit 10 has no array meaning: an array's element descriptors (accessors or
// custom attrs on an index key, a non-writable `length`) are a fact of its
// named layout (`array::array_has_element_descriptors`, object model step 7).
/// #9064: this `GC_TYPE_OBJECT` has O(1)-delete tombstones whose ShapeId is
/// deliberately stable. A cached slot is authoritative only after its value
/// is checked against `TAG_HOLE`; the marker means the cached key was deleted
/// and the access must take the ordinary lookup path.
///
/// Bit 10; only meaningful for `GC_TYPE_OBJECT`.
pub const OBJ_FLAG_STABLE_TOMBSTONES: u16 = 0x400;
// #5054: a property/accessor descriptor (or builtin attrs) has been installed
// on this specific object — the dynamic-write fast path must take the full
// descriptor-aware OrdinarySet walk. Bit 11; only meaningful for
// `GC_TYPE_OBJECT`. Set-only (clearing a descriptor leaves it set; the slow
// path is always correct). #7480 reuses bit 11 for `GC_TYPE_ARRAY` as
// `GC_ARRAY_ELEMENT_SHAPE`; the two are disjoint by `obj_type`.
pub const OBJ_FLAG_HAS_DESCRIPTORS: u16 = 0x800;
// #2145: this object is a per-kind `<TypedArrayCtor>.prototype` whose
// `[[Prototype]]` is the shared `%TypedArray%.prototype` intrinsic.
// `Object.getPrototypeOf(Int8Array.prototype)` returns the cached
// `TYPED_ARRAY_INTRINSIC_PROTO_PTR` (a single object shared across all
// 11 typed-array kinds) when this bit is set.
pub const OBJ_FLAG_TYPED_ARRAY_PROTO: u16 = 0x100;
/// Array payload is stored as canonical raw `f64` values, not NaN-boxed
/// `JSValue` slots. This is only meaningful for `GC_TYPE_ARRAY`; object
/// flags share the same `_reserved` word but never inspect this bit.
pub(crate) const GC_ARRAY_RAW_F64_LAYOUT: u16 = 0x80;
/// #10593: a per-array flag, read as "THIS array's
/// `[[Prototype]]` was retargeted". Arrays are never meta-capable, so every
/// `Object.setPrototypeOf(arr, p)` records in its property bag and sets
/// this flag; it rides growth and every GC relocation for the reasons given
/// above, and it is set-only, so a later reset to `Array.prototype` leaves the
/// array (conservatively) on the slow path.
///
/// This is what the inline element guards consult per receiver instead of the
/// process-wide `PERRY_ARRAY_INDEX_FAST_PATH_INVALIDATED` byte, which used to
/// be flipped by the first retargeted array and then stood the index fast path
/// — and with it the cheap element-store barrier — down for EVERY array in the
/// program (a 33x whole-program cliff from one line of setup). The byte now
/// only carries the genuinely global facts: an index on `Array.prototype` or
/// `Object.prototype`, a retargeted `Array.prototype`, or a retargeted lazy
/// JSON array (whose materialized storage is a separate allocation that does
/// not carry this bit).
pub(crate) const GC_ARRAY_CUSTOM_PROTO: u16 = 0x40;
/// Array was synthesized for a function's `arguments` binding. This is only
/// meaningful for `GC_TYPE_ARRAY`; it lets `util.types.isArgumentsObject`
/// distinguish Perry's internal `arguments` arrays from user rest arrays.
pub(crate) const GC_ARRAY_ARGUMENTS_OBJECT: u16 = 0x200;
/// #10166 (brief 4): this `GC_TYPE_ARRAY` carries its own non-index ("named")
/// properties WITH the allocation: the FIRST physical element slot (`arr + 1`)
/// is reserved for a NaN-boxed pointer to a pairs array and `capacity` excludes
/// it, so `array_front_offset(arr) >= 1` for every flagged array (see
/// `array/named_props.rs`). The collector emits that slot as a fixed child edge
/// in `gc::layout_slot_visit`'s Array arm, `js_array_grow` re-reserves it in the
/// replacement allocation, and `storage::shift_dense`'s empty-queue reset keeps
/// it out of `capacity`. Set-only for an allocation (the reserve cannot be
/// returned without moving every element); the pairs array's length says
/// whether any property is live.
///
/// Bit 8 is shared with `OBJ_FLAG_TYPED_ARRAY_PROTO`, which is meaningful only
/// for `GC_TYPE_OBJECT`, the same disjointness bits 9/10/11/12 already rely on.
/// Every reader of that flag must require `obj_type == GC_TYPE_OBJECT` first.
/// Two prototype readers in `object/object_ops/prototype.rs` did not, and
/// reported the TypedArray prototype for arrays carrying named properties;
/// `array::named_props_tests::named_property_arrays_keep_the_array_prototype`
/// pins that. The readers that only use the bit to DECLINE an object fast path
/// (`proxy/put_value.rs`, `field_set_by_name*`, `delete_rest.rs`,
/// `read_stub.rs`, `then_probe.rs`) are type-guarded as well.
pub(crate) const GC_ARRAY_KEY_ATTRS: u16 = 0x100;
/// #8098: this `GC_TYPE_OBJECT` allocation is an ORDINARY plain object. It has
/// no class, but it also carries none of the per-object `[[Set]]` semantics a
/// class-less receiver may otherwise have — a `URL`'s `pathname`/`search`/…
/// slots are live views whose setters rebuild `href`, `Object.prototype` is the
/// realm intrinsic, and native-module receivers dispatch. Only a runtime birth
/// site that has established the receiver is ordinary may set this; it is what
/// admits `JSON.parse` output to the object-write fast paths, whose generated
/// hit paths re-test this exact bit on every store, so a ShapeId shared with an
/// unmarked population can never carry one population's cached slot into
/// another's.
///
/// Bit 9 — only meaningful for `GC_TYPE_OBJECT`, disjoint from the array-only
/// `GC_ARRAY_ARGUMENTS_OBJECT` by `obj_type` (its sole reader goes through
/// `array::header::array_gc_header`, which refuses any header that is not
/// `GC_TYPE_ARRAY`), the same sharing bits 11 and 12 already use. The value
/// MUST match `PLAIN_ORDINARY_OBJ_FLAG` in
/// `perry-codegen/src/expr/proxy_reflect.rs`, which emits it as a literal.
pub const OBJ_FLAG_PLAIN_ORDINARY: u16 = perry_abi::OBJ_FLAG_PLAIN_ORDINARY;
/// This OBJECT allocation has an Arguments private tail. Its class id stays
/// zero: the identity does not invent a constructor or private class fields.
/// Bit 12 is disjoint from object initialization state (14..15). Array
/// numeric-hole layout uses bit 12 only after its array kind guard.
pub(crate) const OBJ_FLAG_ARGUMENTS: u16 = 0x1000;
/// # `GcHeader::_reserved` bit map by GC kind
///
/// Read this before spending a bit. It is the only place both namespaces are
/// written down together, and the reason it exists is that they are not:
/// `OBJ_FLAG_*` lives here, `GC_LAYOUT_*` live
/// in `gc/layout.rs`, and a comment in this file used to claim bits 12..13
/// were "the last free bits" while `gc/layout.rs` already owned 12, 13, 14
/// and 15.
///
/// | bit | OBJECT | PROXY | other uses |
/// |---|---|---|---|
/// | 0..2 | FROZEN / SEALED / NO_EXTEND | no proxy-specific writer | integrity flags |
/// | 3..4 | survival age | survival age | GC_COPY_SURVIVAL_AGE_MASK, all kinds |
/// | 5 | GC_OBJECT_CALLABLE | GC_OBJECT_CALLABLE | unused |
/// | 6 | NULL_PROTO | unused | array: CUSTOM_PROTO |
/// | 7 | NEVER_MOVE | unused | array: RAW_F64_LAYOUT; closure: NON_CONSTRUCTABLE; byte owner: FOREIGN_DATA; byte view: LENGTH_TRACKING |
/// | 8 | TYPED_ARRAY_PROTO | unused | array: KEY_ATTRS; byte owner: RESIZABLE |
/// | 9 | PLAIN_ORDINARY | unused | array: ARGUMENTS_OBJECT |
/// | 10 | STABLE_TOMBSTONES | unused | byte pin count |
/// | 11 | HAS_DESCRIPTORS | unused | array: ELEMENT_SHAPE; byte pin count |
/// | 12 | ARGUMENTS | unused | array: RAW_F64_HOLES; byte pin count |
/// | 13 | MEMO_NAMED / generic ALL_POINTERS initialization | unused | array/closure: ALL_POINTERS; byte pin count |
/// | 14..15 | GC_LAYOUT_STATE_MASK initialization | unused | array/closure layout state |
///
/// OBJECT has ObjectFields layout: generic layout initialization writes
/// 13..15 even though its field iterator later selects exclusively by ShapeId.
/// These bits are not available for an independent object fact. MEMO_NAMED
/// is published after initialization and its readers require OBJECT.
///
/// Byte families, TypedArray and NativeArena owner only: byte access uses
/// the active byte-pin count in 9..13, 14 is detached state, and 15
/// preserves a permanent pin. These kinds have no dynamic layout-slot state.
/// No header or payload layout grows.
pub const OBJ_FLAG_RESERVED_BIT_MAP_SEE_DOC: () = ();

/// All independent OBJECT header meanings. Callability must never alias one.
pub(crate) const GC_OBJECT_CALLABLE_IS_DISJOINT: bool = GC_OBJECT_CALLABLE
    & (super::layout::GC_LAYOUT_STATE_MASK
        | super::layout::GC_LAYOUT_ALL_POINTERS
        | super::layout::GC_COPY_SURVIVAL_AGE_MASK
        | OBJ_FLAG_FROZEN
        | OBJ_FLAG_SEALED
        | OBJ_FLAG_NO_EXTEND
        | OBJ_FLAG_NULL_PROTO
        | OBJ_FLAG_NEVER_MOVE
        | OBJ_FLAG_TYPED_ARRAY_PROTO
        | OBJ_FLAG_PLAIN_ORDINARY
        | OBJ_FLAG_STABLE_TOMBSTONES
        | OBJ_FLAG_HAS_DESCRIPTORS
        | OBJ_FLAG_ARGUMENTS
        | OBJ_FLAG_MEMO_NAMED)
    == 0;
const _: () = assert!(GC_OBJECT_CALLABLE_IS_DISJOINT);

/// #6011: every element slot in `[0, length)` holds either canonical raw-f64
/// number bits or `TAG_HOLE` — the hole-tolerant sibling of
/// `GC_ARRAY_RAW_F64_LAYOUT`. Set when `new Array(n)` hole-initializes a
/// user-facing array (and by the range-loop guard after a hole-seeing verify
/// pass); cleared alongside the dense flag by `clear_array_numeric_layout`,
/// which every non-numeric store path already funnels through. Lets the
/// packed-f64 range-loop guard skip its whole-array verify walk. Bit 12 —
/// bits 12..13 were the last free `_reserved` bits (see the bit map above).
/// Only meaningful for `GC_TYPE_ARRAY`.
pub(crate) const GC_ARRAY_RAW_F64_HOLES: u16 = 0x1000;
/// #7480: every element slot in `[0, length)` holds a `POINTER_TAG`-boxed
/// `GC_TYPE_OBJECT` whose `class_id` is the one recorded in the array's
/// element-shape record — the *pointer* sibling of the two raw-f64 bits
/// above, and the shared prerequisite of both element-shape routes.
///
/// This is the O(1) half of the proof and it exists for the same reason the
/// raw-f64 bit does: it rides in `_reserved`, which the copying collector
/// copies verbatim, so the invariant survives a move without a side-table
/// walk. The *shape id* itself cannot fit here, so it lives in the
/// address-keyed record `array::element_shape` maintains, moved on
/// relocation by `layout_transfer`. The bit is
/// the authority: a fresh allocation's `_reserved` is zero, so a stale
/// record left behind at a recycled address is never consulted.
///
/// Bit 11 — shared with `OBJ_FLAG_HAS_DESCRIPTORS`, which is only
/// meaningful for `GC_TYPE_OBJECT`, exactly as `GC_ARRAY_RAW_F64_HOLES`
/// (bit 12) is free on ordinary objects. Only meaningful for
/// `GC_TYPE_ARRAY`; every accessor goes through `array::element_shape`,
/// which checks `obj_type` first.
pub(crate) const GC_ARRAY_ELEMENT_SHAPE: u16 = 0x800;

pub(super) const POINTER_TAG: u64 = 0x7FFD_0000_0000_0000;
pub(super) const STRING_TAG: u64 = 0x7FFF_0000_0000_0000;
pub(super) const BIGINT_TAG: u64 = 0x7FFA_0000_0000_0000;
pub(super) const POINTER_MASK: u64 = 0x0000_FFFF_FFFF_FFFF;
pub(super) const TAG_MASK: u64 = 0xFFFF_0000_0000_0000;

#[cfg(test)]
mod buffer_family_type_tests;

#[cfg(test)]
mod header_admission_tests {
    // Only the independent descriptor witness selects this test fault. Do
    // not query the environment in each runtime lookup during GC sweeping.
    std::thread_local! {
        pub(super) static DENSE_DESCRIPTOR_FAULT: std::cell::Cell<bool> = const {
            std::cell::Cell::new(false)
        };
        pub(super) static INERT_LAYOUT_FAULT: std::cell::Cell<bool> = const {
            std::cell::Cell::new(false)
        };
    }

    #[test]
    fn sparse_header_admission_agrees_with_all_type_descriptors() {
        DENSE_DESCRIPTOR_FAULT.set(crate::buffer::bytes::b4_sabotage("dense_core_descriptor"));
        INERT_LAYOUT_FAULT.set(crate::buffer::bytes::b4_sabotage("inert_layout_descriptor"));
        for kind in 0..=u8::MAX {
            assert_eq!(
                super::gc_type_info(kind),
                super::GC_TYPE_INFO_BY_ID
                    .get(kind as usize)
                    .filter(|info| info.type_id != 0),
                "descriptor lookup must preserve the authoritative metadata for {kind:#x}"
            );
            assert_eq!(
                super::gc_type_layout_slot_kind(kind),
                super::GC_TYPE_INFO_BY_ID
                    .get(kind as usize)
                    .filter(|info| info.type_id != 0)
                    .map_or(super::GcLayoutSlotKind::None, |info| info.layout_slot_kind),
                "unused ids must have inert layout fields for {kind:#x}"
            );
            assert_eq!(
                super::gc_type_is_known(kind),
                super::gc_type_info(kind).is_some(),
                "header kind {kind:#x}"
            );
            // Exhaust the whole type-byte domain against the declarative
            // brand/role contract, independently of the masked-range form.
            let brand = kind & 0x1f;
            let bytes = kind & 0xc0 == 0x40 && brand <= 18;
            assert_eq!(super::is_byte_family_type(kind), bytes);
            assert_eq!(super::is_byte_view_type(kind), bytes && kind & 0x20 != 0);
            assert_eq!(super::is_typed_array_type(kind), bytes && brand <= 11);
            assert_eq!(
                super::is_buffer_family_type(kind),
                bytes && (brand == 0 || brand >= 12)
            );
            assert_eq!(
                super::is_uint8array_buffer_type(kind),
                bytes && matches!(brand, 0 | 16 | 17)
            );
        }
    }
    #[test]
    fn changing_dense_core_descriptor_turns_metadata_agreement_red() {
        let child = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "gc::types::header_admission_tests::sparse_header_admission_agrees_with_all_type_descriptors", "--nocapture"])
            .env("PERRY_B4_SABOTAGE", "dense_core_descriptor").output().unwrap();
        assert!(String::from_utf8_lossy(&child.stdout).contains("running 1 test"));
        assert!(
            !child.status.success(),
            "metadata substitution must be detected"
        );
    }
    #[test]
    fn changing_inert_layout_turns_layout_agreement_red() {
        let child = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "gc::types::header_admission_tests::sparse_header_admission_agrees_with_all_type_descriptors", "--nocapture"])
            .env("PERRY_B4_SABOTAGE", "inert_layout_descriptor").output().unwrap();
        assert!(String::from_utf8_lossy(&child.stdout).contains("running 1 test"));
        assert!(
            !child.status.success(),
            "an active retired layout must be detected"
        );
    }
    #[test]
    fn admitting_a_retired_kind_turns_header_admission_red() {
        let child = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "gc::types::header_admission_tests::sparse_header_admission_agrees_with_all_type_descriptors", "--nocapture"])
            .env("PERRY_B4_SABOTAGE", "retired_type_admission").output().unwrap();
        assert!(String::from_utf8_lossy(&child.stdout).contains("running 1 test"));
        assert!(!child.status.success(), "retired kind must be rejected");
    }
}
