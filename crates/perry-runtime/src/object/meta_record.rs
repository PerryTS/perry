//! The per-object metadata record (`ObjectMeta`) and the offsets emitted code
//! addresses it by.
//!
//! Split out of `object/mod.rs` for the 2,000-line gate, which the record's
//! sixteenth word (#10868 step 2.5 stage 1's `dictionary_keys`) took past the
//! cap. The struct and its `offset_of!` pins are one unit — the pins exist
//! precisely because `perry-codegen` reaches words 4, 6, 7 and 12 by index —
//! so they move together or the contract is split from the thing it
//! constrains.
//!
//! Moving it also makes `object/mod.rs` a file one lane at a time no longer
//! has to share: the record and the transition cache were the two regions in
//! it owned by different lanes of the One Path campaign.

/// #6759 Phase B: per-object metadata record, reached from
/// [`ObjectHeader::meta`] in two dependent loads (no side-table probe).
///
/// GC-arena allocated (`GC_TYPE_OBJECT_META`). Its header slot is a traced +
/// rewritten child edge (the record is reachable ONLY through its owner),
/// so liveness, evacuation, and death all ride the ordinary GC — no manual
/// free paths, no owner registry, and no stale-address hazard: the record
/// dies with (and only with) its owner.
///
/// Only the authoritative `GC_TYPE_OBJECT` kind has this layout. RegExp uses
/// its own GC kind and slot descriptor, so no ObjectHeader consumer needs to
/// inspect its native payload to disambiguate the two.
///
/// The shipped Phase B record holds the custom `[[Prototype]]`, the Phase C2
/// per-key descriptor summaries, object flags, and owned spill storage. The
/// RFC also sketched an exotic-kind tag here, but Date/RegExp/Error/Promise/
/// Map/Set/Temporal have distinct cell layouts rather than an `ObjectHeader`;
/// representing their kind here first requires header unification. Their
/// expando payloads therefore remain in the per-thread `RuntimeState` with GC
/// rekey/prune defenses instead of being described as the next incremental
/// `ObjectMeta` migration.
#[repr(C)]
pub struct ObjectMeta {
    /// Custom `[[Prototype]]` recorded by a user-facing operation or runtime
    /// prototype wiring: the NaN-boxed proto bits,
    /// `crate::value::TAG_NULL` for an explicit null prototype, or 0 when
    /// unset (fall back to default prototype resolution).
    pub prototype: u64,
    /// #6759 Phase C2: Bloom summary of the string keys with a customized
    /// property descriptor (non-default writable/enumerable/configurable)
    /// installed on THIS object — bit `key_bytes_hash(key) & 63` per key.
    /// Monotonic (descriptor removal never clears a bit — another key may
    /// share it; a spurious bit just costs one table probe). A clear bit is
    /// authoritative: no `property_descriptors` entry `(owner, key)` can
    /// exist for a key whose bit is clear, so the hot paths skip the
    /// side-table probe (and its per-call `String` build) entirely. POD —
    /// the GC trace arm visits the record's three child edges explicitly.
    pub attr_key_bits: u64,
    /// Same summary for accessor descriptors (`get`/`set` installs) — the
    /// `accessor_descriptors` table twin of `attr_key_bits`.
    pub accessor_key_bits: u64,
    /// Object-only state and compact scalar proof payloads. Bit 0 records
    /// prototype-semantic divergence (including runtime wiring); bit 3 records
    /// that a user-facing operation chose the prototype. Keeping those signals
    /// separate prevents internal wiring from masquerading as
    /// `Object.setPrototypeOf`. #8690 reserves bits 1..2 and 8..63 for the
    /// packed Array-subclass numeric-prefix proof (kind, verified bound, and
    /// ShapeId);
    /// its address-reuse-safe authority is a type-specific GcHeader bit.
    /// In particular, GcHeader bit 12 is `GC_OBJ_TYPED_LAYOUT_INTACT`, so
    /// using that word for prototype divergence made every typed-layout
    /// object appear to have a custom prototype.
    pub flags: u64,
    /// #6812: object-owned overflow storage — a `GC_TYPE_ARRAY` buffer
    /// (`*mut ArrayHeader` bits, 0 = none) holding the NaN-boxed values of
    /// properties whose field index is at or past the inline alloc_limit,
    /// indexed by ABSOLUTE field index (the inline region's entries stay
    /// hole/undefined, mirroring the retired side-table Vec's fillers).
    /// A traced child edge exactly like `prototype`: the buffer lives and
    /// moves with this record, which lives and moves with its owner — no
    /// pointer-keyed side state, no owner re-keying on evacuation, no
    /// per-object finalization.
    pub spill: u64,
    /// Fresh ClassDefinitionEvaluation identity for instances constructed
    /// from a heap class object. This is object metadata rather than an own
    /// property: private branding must not consume a user field slot, alter
    /// the ShapeId/key order, or become visible to enumeration.
    pub private_evaluation_brand: u64,
    /// Exact class-declared named-prefix identity for an Array-subclass
    /// receiver. Numeric tail mutations change the ordinary ShapeId on every
    /// push/pop even though the named slots before that tail remain fixed.
    /// Property-read PICs may use this nonzero scalar as a second identity
    /// only after `array_subclass_named_prefix_token` has proved the current
    /// keys against the class's registered allocation keys. Generic shape or
    /// semantic transitions clear it; the exact learned numeric-tail
    /// transition is the only publisher that deliberately preserves it.
    pub array_subclass_named_prefix_token: u64,
    /// Native pointer to this receiver's per-thread [`ObjectHotTables`].
    /// Array-subclass tail transitions are agent-local: their ShapeIds and
    /// rooted key arrays belong to the same thread that owns the object. Once
    /// a transition is learned, caching that stable heap allocation here lets
    /// every later push/pop reach the full historical shape lattice without a
    /// Darwin TLS/TSD lookup first.
    ///
    /// This is NOT a managed-heap edge and the ObjectMeta slot visitors must
    /// deliberately ignore it. Perry workers deep-copy values into independent
    /// arenas rather than sharing ObjectHeaders, so an object cannot carry the
    /// pointer into another agent. The RuntimeState allocation outlives every
    /// object in that thread.
    pub array_tail_object_hot: u64,
    /// Move-stable, receiver-local cache of the Array-subclass dense layout.
    /// `array_subclass_dense_key` is `(class_id << 32) | ShapeId`; the two
    /// payload words use the same packing as `array::subclass`'s global
    /// collision cache. They contain scalar slot indices only, never managed
    /// pointers. A generic semantic/structural mutation publishes a new
    /// ShapeId before it becomes observable, so a stale payload misses by key
    /// without a pointer-side-table invalidation walk. Exact learned numeric
    /// tail transitions update these words directly.
    pub array_subclass_dense_key: u64,
    pub array_subclass_dense_slots: u64,
    pub array_subclass_dense_bounds: u64,
    /// #6759 phase 1: named own properties for a cell that has no
    /// `keys_array`/inline-slot layout of its own — a NaN-boxed pointer to an
    /// ordinary object used as the property bag, or 0 when the owner has none.
    ///
    /// An `ErrorHeader` (and every other exotic cell) cannot store named
    /// properties inline, which is why they lived in `ERROR_USER_PROPS`, keyed
    /// by the owner's ADDRESS and needing four GC hooks of their own —
    /// rekey-on-evacuation, finalize, dead-sweep and a root scanner — plus the
    /// long-standing bug that a recycled address inherited the previous
    /// tenant's properties.
    ///
    /// Hanging the bag off the metadata record instead makes it an ordinary
    /// child edge: it moves with its owner, dies with its owner, and needs no
    /// address bookkeeping at all.
    pub expando: u64,
    /// Elements backing store of a `class X extends Array` instance: a
    /// `GC_TYPE_ARRAY` (`*mut ArrayHeader` bits, 0 = none) holding the
    /// instance's indexed elements and `length`, exactly as a plain Array
    /// does — so `push`/`pop`/`obj[i]` are element operations instead of
    /// property-shape transitions (`array/subclass_elements.rs`). A traced
    /// child edge exactly like `spill`: lives and moves with this record.
    /// Installed by `js_array_subclass_init` under
    /// `array_subclass_elements_enabled()`; never present otherwise.
    pub elements: u64,
    /// #10287 exact identity for the overwhelmingly common case of an object
    /// carrying descriptors for exactly ONE key. `descriptor_key_count` is 0
    /// (none recorded), 1 (`descriptor_key_hash` is the full
    /// `key_bytes_hash` of that single key) or 2 (more than one distinct key
    /// — consult the Bloom summaries and then the tables).
    ///
    /// The 64-bit Bloom above answers "maybe" for about one key in 64, and a
    /// maybe costs far more than a table probe: the store it rejects takes
    /// the slow path, which appends to a PRIVATE keys array and drops the
    /// receiver off the shared transition chain for the rest of its life.
    /// zod installs exactly one descriptor per schema (`_zod`), so a single
    /// full-width compare here answers every store on those objects exactly,
    /// with no table probe and no string rebuild.
    ///
    /// Maintained by the same writer as the Bloom bits
    /// (`note_meta_descriptor_key`), so it inherits that function's
    /// invariant: every descriptor-table insert for a meta-capable owner
    /// records its key here first.
    pub descriptor_key_hash: u64,
    /// Distinct descriptor-key count, saturating at 2. See
    /// [`ObjectMeta::descriptor_key_hash`].
    pub descriptor_key_count: u64,
    /// #10868 step 2.5 stage 1: the ordered own-key list of a DICTIONARY-MODE
    /// receiver — a `GC_TYPE_ARRAY` (`*mut ArrayHeader` bits, 0 = none)
    /// private to this object, holding the same NaN-boxed key strings (and
    /// `TAG_HOLE` tombstones) an ordinary receiver keeps in its shape's keys
    /// array. Values are NOT moved by the mode: the key at position `i` still
    /// reads inline slot `i` below the live bound and `spill` at or above it.
    ///
    /// A traced child edge exactly like `spill` (#6812): it lives and moves
    /// with this record, which lives and moves with its owner — no
    /// address-keyed side state, no owner re-keying on evacuation.
    ///
    /// Nonzero means "this receiver's keys are here" only in combination with
    /// a shape that publishes none; `dictionary::is_dictionary` is the single
    /// spelling of the test. See `object/dictionary.rs`.
    pub dictionary_keys: u64,
    /// #10868 lever (iv): a stable identity for an object that is some other
    /// object's prototype, assigned once by `mark_object_as_prototype` and
    /// never changed. 0 = none assigned.
    ///
    /// Why a serial and not the prototype's address or ShapeId: the address
    /// moves under the collector (and keying on it is an address-keyed derived
    /// structure); the ShapeId is shared by distinct prototypes (unsound — §3
    /// needs the receiver's shape to determine its prototype) AND changes
    /// whenever the prototype gains a key (so two receivers diverging to the
    /// same prototype before and after that would fork). The receiver's
    /// generation must key on the prototype's IDENTITY, not its STATE.
    ///
    /// Placed before `native_state` so every codegen-addressed offset above is
    /// unchanged and `native_state` stays the last word (#340/#341).
    pub proto_serial: u64,
    /// #340/#341 honest tags: packed state for a runtime class whose instances
    /// are ORDINARY objects rather than small registry handles
    /// (`TextEncoder` / `TextDecoder` today; the other twelve families follow).
    ///
    /// The layout is private to the owning family — `text.rs` packs
    /// `(present | encoding index | fatal | ignoreBOM)` here — and `0` means
    /// "this object has no native state". It lives in the meta record rather
    /// than an inline slot for the same reason as
    /// [`ObjectMeta::private_evaluation_brand`]: it must not consume a user
    /// field slot, alter the ShapeId or key order, or become visible to
    /// enumeration. An inline slot would also be handed to the first user
    /// expando (`decoder.mine = 1`) by the slot allocator and overwritten.
    ///
    /// POD. Never a managed-heap edge — the GC trace arm visits this record's
    /// child edges explicitly and this word is not one of them, exactly like
    /// `array_tail_object_hot`.
    ///
    /// LAST FIELD ON PURPOSE: this record carries `offset_of!` assertions for
    /// the words codegen and the spill lanes address by index, so a new field
    /// may only be appended.
    pub native_state: u64,
}


// #6812 spill lanes: the versioned write-loop emitter
// (perry-codegen/src/stmt/loops.rs) addresses `meta.spill` at word 4 of the
// ObjectMeta record and buffer elements one word past the ArrayHeader. Keep
// codegen and these structs in lock-step.
const _: () = assert!(std::mem::offset_of!(ObjectMeta, spill) == 32);
const _: () = assert!(std::mem::offset_of!(ObjectMeta, array_subclass_named_prefix_token) == 48);
const _: () = assert!(std::mem::offset_of!(ObjectMeta, array_tail_object_hot) == 56);
// The Array-subclass elements store: codegen's inline `elem.*` tiers load
// `ObjectHeader.meta` then this word (perry-codegen `expr/index_get` and
// `property_get/composed_ics.rs`). Keep in lock-step.
const _: () = assert!(std::mem::offset_of!(ObjectMeta, elements) == 96);
// #10868 step 2.5 stage 1. Not addressed by index from emitted code, so this
// pin is documentation of where it landed rather than an ABI contract — the
// contract it must not break is the `native_state`-is-last assertion below,
// which is what forbids inserting it ahead of one of the offsets above.
const _: () = assert!(std::mem::offset_of!(ObjectMeta, dictionary_keys) == 120);
// #340/#341: `native_state` must stay the LAST word. The offsets above are
// addressed by index from emitted code, so a field inserted mid-struct moves
// them silently; this pins the append instead of trusting the comment.
const _: () = assert!(
    std::mem::offset_of!(ObjectMeta, native_state) + 8 == std::mem::size_of::<ObjectMeta>()
);
