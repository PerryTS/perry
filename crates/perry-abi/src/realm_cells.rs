//! Shared physical layouts of realm cells emitted by the compiler.
//! Runtime births and image emission address these same fields.

/// Image symbols use the upper half of the identity space; dynamic births
/// draw only from the lower half. Identity does not depend on the link address.
pub const IMAGE_SYMBOL_ID_BIT: u64 = 1 << 63;

/// GC kind of an object-owned metadata record.
pub const GC_TYPE_OBJECT_META: u8 = 19;

/// #6759 Phase B: per-object metadata record, reached from
/// `ObjectHeader::meta` in two dependent loads (no side-table probe).
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
    /// The boxed Number/String/Boolean/BigInt/Symbol primitive, NaN-boxed.
    /// This private child slot replaces the address-keyed payload table.
    /// Reuse a retired descriptor word so emitted metadata offsets stay fixed.
    pub boxed_primitive: u64,
    /// Private Reflect metadata record, a traced NaN-boxed object edge.
    pub reflect_metadata: u64,
    /// Object state and compact scalar proof payloads. Bit 0 records native
    /// fallback after an ordinary undefined read; shape minting projects that
    /// fact into OrdinaryNativeAlias. Bits 3..4 are unused. Bits 1..2 and
    /// 8..63 hold the packed Array-subclass numeric-prefix proof payload
    /// (verified bound and unproven twin ShapeId). The address-reuse-safe
    /// authority for that proof is the ShapeId kind.
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
    /// Lexical ClassDefinitionEvaluation of a shared evaluation prototype,
    /// or the per-object brand of an exceptional constructor-returned/native
    /// receiver. Ordinary class births name their class object from their
    /// shape and allocate no metadata solely for this word. This remains a
    /// traced edge without consuming a user field slot.
    pub private_evaluation_brand: u64,
    /// On a function's prototype object: the birth word of the instances
    /// `new F()` makes with this prototype, as a fact on the holder (the
    /// record the instances are born with). Low 32 bits: the agent-local
    /// LayoutWord; high 32 bits: the constructor layout descriptor's identity
    /// (`function_family_birth::birth_tag`). Every use validates it
    /// as main validates its descriptor cell: the record is still present
    /// (rows are never reused, so a present row is the record published)
    /// and names this prototype identity and serial; a miss re-interns and
    /// rewrites it (`function_family_birth::allocate_layout`). A scalar, not
    /// traced. Zero elsewhere. (`instance_birth` is the keyless
    /// synthetic-class birth of a function WITHOUT a constructor layout.)
    pub function_layout_birth: u64,
    /// Native pointer to this receiver's per-thread `ObjectHotTables`.
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
    /// Scalar identity of a movable Set key; zero means unassigned.
    pub key_identity: u64,
    /// Private owner-family state bag, a traced raw array edge. Promises use
    /// three combinator/reaction slots; ordinary property holders use three
    /// VM state slots. Node-API metadata, when present on a property holder,
    /// uses a fourth slot without interpreting the first three. Other owner
    /// families have separate cells and record layouts.
    pub owned_state: u64,
    /// Object-owned ordered overflow table (raw GC_TYPE_OVERFLOW pointer,
    /// zero until needed). Its entries carry values, attributes and fixed-slot
    /// references; the collector traces keys and table-owned values directly.
    pub overflow: u64,
    /// #10868 lever (iv): a stable identity for a prototype or shared class
    /// evaluation holder, assigned once and never changed. Assigning a class
    /// identity alone does not mark it as a prototype. 0 = none assigned.
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
    /// #11506: an `arguments` object's exotic state, NaN-boxed; 0 on every
    /// other object. `TAG_TRUE` marks an unmapped object with the restricted
    /// `callee`, `TAG_FALSE` a sloppy one with no parameter alias, and a
    /// `POINTER_TAG`-boxed `GC_TYPE_ARRAY` a sloppy MAPPED one, whose element
    /// `i` is the box parameter `i` aliases (see `object/arguments.rs`).
    ///
    /// It replaces `ARGUMENTS_OBJECTS`, a thread-local table keyed by the
    /// owner's ADDRESS that cost a hash insert per call, a rekey per move and
    /// a dead-owner prune per collection. The mapped array is a traced child
    /// edge exactly like `expando`: it lives, moves and dies with this record.
    ///
    /// Placed before `native_state` for the same reason as `proto_serial`.
    pub arguments: u64,
    /// #10507: the birth record of the objects `new F()` creates while this
    /// object is `F.prototype` — 0 until the first such construction. The low
    /// half is the class id those objects carry (the minting function's
    /// synthetic class); the high half is their birth ShapeId (no keys, this
    /// object's serial as `proto_id`), the one the class-default link leaves.
    /// Both are facts of THIS object, so a construction reaches them from
    /// `F.prototype` with two loads. Cleared when that class's registered
    /// prototype moves to another object.
    ///
    /// POD, never a heap edge.
    ///
    /// Placed before `native_state` for the same reason as `proto_serial`.
    pub instance_birth: u64,
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
    /// A native Set uses this word for its lazily allocated lookup index.
    /// SET_REGISTRY owns that native allocation; moving the metadata only
    /// copies the pointer and does not transfer ownership.
    ///
    /// A heap class object uses this word for a unique scalar private-name
    /// storage identity. It survives evacuation and never retains another
    /// object. Class objects cannot also be native decoders or Sets.
    ///
    /// Native payload families and WeakMap/WeakSet store their object-owned
    /// GC cell here as a POINTER_TAG word, and a plain runtime stream its
    /// state record (`node_stream::state_record`). The GC descriptor traces and
    /// rewrites that edge. Other families store untagged POD and no edge.
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
const _: () = assert!(core::mem::offset_of!(ObjectMeta, spill) == 32);
const _: () = assert!(core::mem::offset_of!(ObjectMeta, function_layout_birth) == 48);
const _: () = assert!(core::mem::offset_of!(ObjectMeta, array_tail_object_hot) == 56);
// The Array-subclass elements store: codegen's inline `elem.*` tiers load
// `ObjectHeader.meta` then this word (perry-codegen `expr/index_get` and
// `property_get/composed_ics.rs`). Keep in lock-step.
const _: () = assert!(core::mem::offset_of!(ObjectMeta, elements) == 96);
// #10868 step 2.5 stage 1. Not addressed by index from emitted code, so this
// pin is documentation of where it landed rather than an ABI contract — the
// contract it must not break is the `native_state`-is-last assertion below,
// which is what forbids inserting it ahead of one of the offsets above.
const _: () = assert!(core::mem::offset_of!(ObjectMeta, overflow) == 120);
// #340/#341: `native_state` must stay the LAST word. The offsets above are
// addressed by index from emitted code, so a field inserted mid-struct moves
// them silently; this pins the append instead of trusting the comment.
const _: () = assert!(
    core::mem::offset_of!(ObjectMeta, native_state) + 8 == core::mem::size_of::<ObjectMeta>()
);

/// Symbol object header. Allocated in the arena (or native storage for registered
/// symbols that need to outlive GC cycles).
#[repr(C)]
pub struct SymbolHeader<S> {
    /// Magic number for type discrimination. Always SYMBOL_MAGIC.
    pub magic: u32,
    /// Whether this symbol is in the global registry (Symbol.for). Registered
    /// symbols have their description used as the registry key.
    /// Bit 0 is Symbol.for membership; bit 1 is persistent native storage.
    pub registered: u32,
    /// Description string pointer, or null for `Symbol()` with no argument.
    pub description: *mut S,
    /// Unique id (monotonic counter). Two symbols with the same description
    /// still compare as different unless created via Symbol.for.
    pub id: u64,
}

const _: () = assert!(core::mem::offset_of!(SymbolHeader<u8>, description) == 8);
const _: () = assert!(core::mem::offset_of!(SymbolHeader<u8>, id) == 16);
const _: () = assert!(core::mem::size_of::<SymbolHeader<u8>>() == 24);

/// The getter closure word.
pub const ACCESSOR_PAIR_GET: usize = 0;
/// The setter closure word.
pub const ACCESSOR_PAIR_SET: usize = 1;
/// The class getter's compiled entry word.
pub const ACCESSOR_PAIR_RAW_GET: usize = 2;
/// The class setter's compiled entry word.
pub const ACCESSOR_PAIR_RAW_SET: usize = 3;
/// Words in a pair.
pub const ACCESSOR_PAIR_LEN: usize = 4;

/// The collector's fact after a closure acquires its first non-capture child
/// (its props bag). Other state and semantic bits retain their birth values.
pub const fn closure_reserved_with_props(reserved: u16) -> u16 {
    let state_mask = crate::GC_LAYOUT_POINTER_FREE | crate::GC_LAYOUT_SIDE_MASK;
    if reserved & state_mask == crate::GC_LAYOUT_POINTER_FREE {
        reserved & !crate::GC_LAYOUT_POINTER_FREE
    } else {
        reserved
    }
}
