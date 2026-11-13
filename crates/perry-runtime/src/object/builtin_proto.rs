//! The "builtin prototype unmodified" proof (#11915): `Pristine(P)` of the
//! typed-entry contracts (decision 129, §4.1 / slice 5).
//!
//! # The fact
//!
//! Object model step 3 (`OBJECT_MODEL_2026-10-13.md` §3.6): the fact is the
//! **shadow bit** of each family's intrinsic prototype, bit 30 of its layout
//! word (`object::layout_desc`). It is set once the realm's intrinsic
//! prototype of family `k` has been written,
//! defined, deleted or relinked after the runtime sealed it at startup. It is
//! set-only: restoring a method never reopens the fact.
//!
//! So `Pristine(k)` is "the layout word of agent slot
//! `AGENT_PTR_INTRINSIC_PROTO + k` and `%Object.prototype%` are below
//! `UNSHADOWED_WORD_END`": read the objects' own words and combine their
//! shadow bits in one compare. A write marks only the object it writes;
//! no family invalidation, per-site state or second copy is maintained.
//! Each agent's realm carries its own bits. A receiver of a builtin kind
//! resolves its builtin methods through its family's prototype and then
//! `%Object.prototype%`; the receiver's own keys and `[[Prototype]]` edge are
//! the receiver's own proof (shape or header). A site that has proven the
//! receiver's KIND and has `Pristine(kind)` knows the method it would read is
//! the builtin it is about to call.
//!
//! # Who reads it
//!
//! * emitted code: `perry-codegen`'s `builtin_proto::emit_patched_test`,
//!   folded into the branch a site already has;
//! * the runtime: [`pristine`] / [`receiver_pristine`], and [`redirect_call`]
//!   — the ONE shared generic path a failed proof takes.
//!
//! # Who writes it
//!
//! [`seal_realm`] gives each sealed prototype's meta record its family
//! (`OBJECT_META_FLAG_SEALED_INTRINSIC` and the family bits of
//! `ObjectMeta::flags`) and publishes it into its agent slot once the realm
//! is populated. Every mutation funnel then calls [`note_write`] (one meta load,
//! null-checked) or [`note_array_prototype_write`]: the slot and spill stores,
//! the shape publication funnel (key add, delete, descriptor and attribute
//! changes, `[[Prototype]]` relinks), the descriptor, delete and
//! `[[Prototype]]` funnels, and the Array-prototype named-property set and
//! delete. Nothing is paid until a sealed prototype is written.
//!
//! [`seal_runtime_prototype`] seals a prototype the runtime builds (URL,
//! timers) under family [`RUNTIME_FAMILY`]: a write sets that prototype's own
//! shadow bit and nothing else. [`name_runtime_prototype`] only gives one its
//! reserved identity.
//!
//! The runtime's own definitions (`own_override::as_builtin_definition`, the
//! lazy populations) do not count: they install the builtin itself.
//!
//! # Locked built-ins (DEC 250)
//!
//! A member the image gives a built-in prototype, constructor or namespace is
//! never written by the program: replacing, deleting or redefining it, or
//! relinking the object, is a `TypeError` at the store; adding a member works
//! and is modelled as before. Every funnel reports WHAT it is about to do
//! ([`Mutation`]); [`note_sealed_write`] decides once, for the sealed
//! families [`locked_family`] names (the `BuiltinProto` families only; not
//! namespace holders or constructor statics, `console`, the realm global or a
//! runtime prototype; and not a prototype's `constructor`: owner ruling
//! 2026-11-03). "Existing" is a position below the image
//! key count the meta record keeps ([`image_key_count`]): the shape is the
//! authority, there is no list of names.

use super::ObjectHeader;

/// Number of [`BuiltinProto`] families.
pub const FAMILIES: usize = crate::codegen_abi::BUILTIN_PROTO_FAMILIES;

use perry_abi::lock::{refuses, LockHolder, LockKey, LockMutation};
pub use perry_abi::BuiltinProto;

/// What a mutation funnel is about to do to the object it reports (DEC 250).
/// Each funnel knows its kind by construction: a value store writes an
/// existing storage slot, a key-add transition appends a key that is not own,
/// the delete, descriptor and `[[Prototype]]` funnels name their key or edge.
#[derive(Clone, Copy)]
pub(crate) enum Mutation<'a> {
    /// A value store into the existing own storage slot `slot` (inline or
    /// spill index, as the store funnels address it).
    Replace(usize),
    /// `delete obj[key]`.
    Delete(Key<'a>),
    /// `Object.defineProperty(obj, key, …)`: a data or accessor definition, an
    /// attribute change, or clearing one.
    Redefine(Key<'a>),
    /// `Object.setPrototypeOf(obj, …)` / `obj.__proto__ = …`.
    Relink,
    /// A key-add transition appending `key`: never an own key of the image.
    Add(Key<'a>),
    /// A report BEFORE a structural change whose own funnel already decided,
    /// or a stamp that does not carry its key: the layout is still in place,
    /// so the image key count can be recorded; nothing is decided.
    Restructure,
    /// A shape stamp about to publish the layout word `published` without
    /// naming the key it changes (the slow key-add path): the layout is still
    /// in place, so comparing the published key list with the own one tells
    /// an add from a restructure.
    Stamp(u32),
    /// A report AFTER a change (`shape publication`, `overflow key change`,
    /// `function own state`): the modelling runs; nothing is decided or
    /// recorded, the layout may already differ from the image.
    Publication,
}

/// A property key as the funnels hold it.
#[derive(Clone, Copy)]
pub(crate) enum Key<'a> {
    /// A property string or a symbol header.
    Str(*const crate::StringHeader),
    /// The bytes of a property string.
    Bytes(&'a [u8]),
    /// A dynamic key (string, symbol or number), as the funnel received it.
    Value(f64),
}

/// The write a locked built-in refuses: the member (as `.name` or
/// `[Symbol(description)]`) and how it was about to be written.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Violation {
    Replace(String),
    Delete(String),
    Redefine(String),
    Relink,
    /// A key that is not own on an add-locked prototype (DEC 250 (i)).
    Add(String),
    /// An array-index key on `%Array.prototype%` or `%Object.prototype%`.
    AddIndex,
}

/// `Pristine(family)`: neither `family`'s prototype nor `%Object.prototype%`
/// has been modified: the shadow bit of this agent's intrinsic prototype.
#[inline(always)]
pub fn pristine(family: BuiltinProto) -> bool {
    // SAFETY: the slot names a live intrinsic of this agent or the static
    // stand-in; the word read is the header's `+4`.
    unsafe {
        let own = (*crate::agent_ptrs::intrinsic_proto(family as usize)).parent_class_id;
        let parent = if family == BuiltinProto::Object {
            0
        } else {
            (*crate::agent_ptrs::intrinsic_proto(BuiltinProto::Object as usize)).parent_class_id
        };
        own < super::layout_desc::UNSHADOWED_WORD_END
            && parent < super::layout_desc::UNSHADOWED_WORD_END
    }
}

/// `family`'s intrinsic prototype itself has not been written since the realm
/// sealed it: its own shadow bit, one load. Unlike [`pristine`] this does not
/// cover `%Object.prototype%`, so it proves only keys that prototype defines
/// itself (a write to it, including a delete, sets the bit).
#[inline(always)]
pub fn prototype_unwritten(family: BuiltinProto) -> bool {
    // SAFETY: as in `pristine`.
    unsafe {
        (*crate::agent_ptrs::intrinsic_proto(family as usize)).parent_class_id
            < super::layout_desc::UNSHADOWED_WORD_END
    }
}

/// `Pristine` for a borrowed call (`Array.prototype.m.call(x, …)`), which
/// also reads `Function.prototype.call`.
#[inline(always)]
pub fn pristine_borrowed(family: BuiltinProto) -> bool {
    pristine(family) && pristine(BuiltinProto::Function)
}

/// What a direct-call site has proven about its receiver: its KIND, so its
/// methods are `family`'s builtins while [`pristine`]`(family)` holds.
/// Checking it is one load and one compare: Leaf, NoCollect, NoThrow.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PristineReceiverFact {
    Intrinsic(BuiltinProto),
}

impl PristineReceiverFact {
    /// Does the fact hold now? A hit may take the direct body; a miss must
    /// take the one generic operation.
    #[inline(always)]
    pub fn holds(self) -> bool {
        match self {
            PristineReceiverFact::Intrinsic(family) => pristine(family),
        }
    }
}

/// An Array took its own `[[Prototype]]` (`GC_ARRAY_CUSTOM_PROTO`, the
/// receiver's own proof). No family is withdrawn — every other array still
/// reads `%Array.prototype%`.
pub(crate) fn note_array_custom_proto() {
    super::own_override::note_builtin_proto_modified();
}

/// Seal one prototype object: from now on a mutation of it withdraws
/// `Pristine(family)`. The object is also marked as a prototype, so its shape lineage is
/// private (`proto_validity::ensure_meta_for_mark`) and every structural
/// change of it reaches [`super::proto_validity::note_object_shape_stamped`].
///
/// # Safety
/// `obj` is a live `ObjectHeader` (a GC_TYPE_OBJECT), or null.
#[inline(never)]
pub(crate) unsafe fn seal_object(obj: *mut ObjectHeader, family: BuiltinProto) {
    // A host family is sealed as a runtime prototype (`seal_built_host_intrinsic`).
    debug_assert!(family.realm_intrinsic());
    seal_object_family(obj, family as usize);
}

/// [`seal_object`] for any intrinsic family: a prototype family, a builtin
/// namespace (`builtin_namespace`), or the realm global.
///
/// # Safety
/// `obj` is a live `ObjectHeader` (a GC_TYPE_OBJECT), or null.
#[inline(never)]
pub(crate) unsafe fn seal_object_family(obj: *mut ObjectHeader, family: usize) {
    if obj.is_null() {
        return;
    }
    let _ = super::proto_validity::mark_object_as_prototype(obj as usize);
    // Marking may have moved nothing (startup runs with collection
    // suppressed by the caller) but it did ensure a meta record.
    let meta = super::object_meta_ensure(
        crate::meta_obj_runtime_born!(obj),
        super::MetaReason::BuiltinSeal,
    );
    if meta.is_null() {
        return;
    }
    // GC_STORE_AUDIT(POINTER_FREE): scalar flag bits, never a reference.
    (*meta).flags |= super::meta_flags::OBJECT_META_FLAG_SEALED_INTRINSIC
        | (((family as u64) & super::meta_flags::OBJECT_META_SEALED_FAMILY_MASK)
            << super::meta_flags::OBJECT_META_SEALED_FAMILY_SHIFT);
}

/// Seal one family's intrinsic the moment its builder completes it, before
/// any program code can reach it. `%Object.prototype%` is built on first use
/// (`ensure_object_intrinsics`), often long before the realm global is
/// populated or in a program that never populates it; until it is sealed, its
/// intrinsic slot names the stand-in, which proves nothing, so every default
/// store would walk its keys. [`seal_realm`] later seals the same object again
/// (idempotent).
pub(crate) fn seal_built_intrinsic(family: BuiltinProto, proto: *mut ObjectHeader) {
    if proto.is_null() {
        return;
    }
    let _no_move = crate::gc::GcSuppressScope::new();
    super::own_override::as_builtin_definition(|| unsafe {
        trace_seal(family, (*proto).parent_class_id);
        seal_object(proto, family);
        crate::agent_ptrs::publish_intrinsic_proto(family as usize, proto);
    });
}

/// Seal a host family's prototype (`Buffer.prototype`) when the member install
/// reached only through its feature group completes it
/// (`typedarray_hook_slots::buffer_prototype_members`), and publish it in the
/// family's agent slot. Its meta family is [`RUNTIME_FAMILY`]: it is a
/// prototype the runtime builds, outside DEC 250's lock, and a write marks
/// only that prototype, its own shadow bit, which is the family's
/// [`prototype_unwritten`]. So the sealed meta family field holds a realm
/// family only below [`BuiltinProto::REALM_INTRINSICS`].
#[inline(never)]
pub(crate) fn seal_built_host_intrinsic(family: BuiltinProto, proto: *mut ObjectHeader) {
    debug_assert!(!family.realm_intrinsic());
    if proto.is_null() {
        return;
    }
    let _no_move = crate::gc::GcSuppressScope::new();
    super::own_override::as_builtin_definition(|| unsafe {
        trace_seal(family, (*proto).parent_class_id);
        seal_object_family(proto, RUNTIME_FAMILY);
        crate::agent_ptrs::publish_intrinsic_proto(family as usize, proto);
    });
}

/// The meta family field of a sealed RUNTIME prototype (one the runtime
/// builds, such as `%URL.prototype%` or a timer prototype; not a
/// [`BuiltinProto`]; also the host family `Buffer`'s prototype,
/// `seal_built_host_intrinsic`). A write marks only that prototype. It follows
/// the locked families below, so DEC 250's lock set past the global is one
/// contiguous range (`ITERATOR_FAMILY..=REGEXP_STRING_ITERATOR_FAMILY`).
pub(crate) const RUNTIME_FAMILY: usize = REGEXP_STRING_ITERATOR_FAMILY + 1;
/// The meta family field of `%IteratorPrototype%` and of
/// `%ArrayIteratorPrototype%`: built-in prototypes that are not a
/// [`BuiltinProto`] family, sealed by `iterator_prototypes` (DEC 250 (i)).
pub(crate) const ITERATOR_FAMILY: usize = super::builtin_namespace::GLOBAL_FAMILY + 1;
pub(crate) const ARRAY_ITERATOR_FAMILY: usize = super::builtin_namespace::GLOBAL_FAMILY + 2;
/// The meta family field of `%GeneratorPrototype%` and of
/// `%AsyncGeneratorPrototype%` (DEC 250 as narrowed 2026-11-03): their image
/// members (`next`, `return`, `throw`, `@@toStringTag`; not `constructor`)
/// are locked, adds stay open. Sealed by `seal_generator_prototype` when
/// `build_generator_tower` completes them.
pub(crate) const GENERATOR_FAMILY: usize = super::builtin_namespace::GLOBAL_FAMILY + 3;
pub(crate) const ASYNC_GENERATOR_FAMILY: usize = super::builtin_namespace::GLOBAL_FAMILY + 4;
/// The meta family fields of `%MapIteratorPrototype%`,
/// `%SetIteratorPrototype%`, `%StringIteratorPrototype%` and
/// `%RegExpStringIteratorPrototype%` (DEC 250 as narrowed 2026-11-03, like
/// the generator prototypes): their image members (`next`,
/// `@@toStringTag`) are locked, adds stay open. Sealed by
/// `seal_iterator_prototypes` with the shared and array ones.
pub(crate) const MAP_ITERATOR_FAMILY: usize = super::builtin_namespace::GLOBAL_FAMILY + 5;
pub(crate) const SET_ITERATOR_FAMILY: usize = super::builtin_namespace::GLOBAL_FAMILY + 6;
pub(crate) const STRING_ITERATOR_FAMILY: usize = super::builtin_namespace::GLOBAL_FAMILY + 7;
pub(crate) const REGEXP_STRING_ITERATOR_FAMILY: usize = super::builtin_namespace::GLOBAL_FAMILY + 8;
const _: () = assert!(
    REGEXP_STRING_ITERATOR_FAMILY as u64 <= super::meta_flags::OBJECT_META_SEALED_FAMILY_MASK,
    "the meta family field holds every sealed family"
);

/// Does the realm seal the prototype the constructor loop
/// (`global_this::populate`) births for `declaration`? A family prototype is
/// sealed by [`seal_realm`] before any program code can reach it, so it is
/// born on the born-prototype lineage (`shapes::stamp_born_prototype_root`)
/// and that seal's mark mints no shape. Every other one keeps the ordinary
/// lineage until something marks it (a runtime prototype with a declared
/// class or layout word is restamped onto that word's lineage first).
pub(crate) fn realm_seals_constructor_prototype(
    declaration: &crate::object::global_this_tables::BuiltinConstructorDeclaration,
) -> bool {
    BuiltinProto::ALL
        .iter()
        .any(|(family, ctor)| family.realm_intrinsic() && *ctor == declaration.name)
}

/// Seal the built-in iterator prototypes the moment `iterator_prototypes`
/// completes them, before any program code can reach them: `shared`
/// (`%IteratorPrototype%`), then the families in `ITERATOR_FAMILIES` order.
///
/// # Safety
/// Every one is a live, fully built ordinary object (or null).
#[inline(never)]
#[optimize(size)]
pub(crate) unsafe fn seal_iterator_prototypes(
    shared: *mut ObjectHeader,
    families: [*mut ObjectHeader; 5],
) {
    let _no_move = crate::gc::GcSuppressScope::new();
    super::own_override::as_builtin_definition(|| unsafe {
        seal_object_family(shared, ITERATOR_FAMILY);
        for (proto, family) in families.into_iter().zip(ITERATOR_FAMILIES) {
            seal_object_family(proto, family);
        }
    });
}

/// The family of each built-in iterator prototype `seal_iterator_prototypes`
/// seals past `%IteratorPrototype%`: Array, Map, Set, String, RegExp String.
const ITERATOR_FAMILIES: [usize; 5] = [
    ARRAY_ITERATOR_FAMILY,
    MAP_ITERATOR_FAMILY,
    SET_ITERATOR_FAMILY,
    STRING_ITERATOR_FAMILY,
    REGEXP_STRING_ITERATOR_FAMILY,
];

/// Seal `%GeneratorPrototype%` (or `%AsyncGeneratorPrototype%`) the moment
/// its tower is built, before any program code can reach it.
///
/// # Safety
/// `proto` is the live, fully built generator prototype (or null).
#[inline(never)]
#[optimize(size)]
pub(crate) unsafe fn seal_generator_prototype(proto: *mut ObjectHeader, is_async: bool) {
    let _no_move = crate::gc::GcSuppressScope::new();
    let family = if is_async {
        ASYNC_GENERATOR_FAMILY
    } else {
        GENERATOR_FAMILY
    };
    super::own_override::as_builtin_definition(|| unsafe { seal_object_family(proto, family) });
}

/// DEC 250 (i): the prototypes on which the program adds no key: compiled code
/// relies on keys being absent there (own-key misses, "a plain object is not
/// iterable", `IteratorClose` finding no `return`).
#[inline(always)]
pub(crate) fn add_locked(family: usize) -> bool {
    lock_holder(family).is_some_and(LockHolder::add_locked)
}

/// The locked holder (`perry_abi::lock`) sealed family `family` is, if the
/// lock covers it ([`locked_family`]).
#[inline(always)]
pub(crate) fn lock_holder(family: usize) -> Option<LockHolder> {
    // A host family (`Buffer`, `REALM_INTRINSICS..FIRST`) is not locked:
    // the program patches it as in node, which only sets its shadow bit.
    if family < BuiltinProto::REALM_INTRINSICS {
        BuiltinProto::from_index(family).map(LockHolder::Proto)
    } else if family == ITERATOR_FAMILY {
        Some(LockHolder::Iterator)
    } else if family == ARRAY_ITERATOR_FAMILY {
        Some(LockHolder::ArrayIterator)
    } else if family == GENERATOR_FAMILY {
        Some(LockHolder::Generator)
    } else if family == ASYNC_GENERATOR_FAMILY {
        Some(LockHolder::AsyncGenerator)
    } else if family == MAP_ITERATOR_FAMILY {
        Some(LockHolder::MapIterator)
    } else if family == SET_ITERATOR_FAMILY {
        Some(LockHolder::SetIterator)
    } else if family == STRING_ITERATOR_FAMILY {
        Some(LockHolder::StringIterator)
    } else if family == REGEXP_STRING_ITERATOR_FAMILY {
        Some(LockHolder::RegExpStringIterator)
    } else {
        None
    }
}

/// DEC 250 (i): an array-index store reached `%Array.prototype%` or
/// `%Object.prototype%` (`family`), which never carry an index key: always an
/// add. The runtime's own definitions pass.
#[cold]
#[inline(never)]
#[optimize(size)]
pub(crate) fn refuse_index_add(family: BuiltinProto) {
    if refuses(LockHolder::Proto(family), LockKey::Index, LockMutation::Add) && index_add_refused()
    {
        throw_locked(family as usize, &Violation::AddIndex);
    }
}

/// DEC 250 (i): an index add on `%Array.prototype%` / `%Object.prototype%` is
/// refused, unless it is the runtime's own definition. The decision only: the
/// store funnels throw on it ([`refuse_index_add`]), `Reflect.set` and
/// `Reflect.defineProperty` report it as `false` ([`lock_refuses_write`]).
#[inline]
fn index_add_refused() -> bool {
    !super::own_override::in_builtin_definition()
}
const _: () = assert!(
    RUNTIME_FAMILY as u64 <= super::meta_flags::OBJECT_META_SEALED_FAMILY_MASK,
    "the meta family field holds every sealed family"
);

/// Seal a runtime prototype the moment its builder completes it, before any
/// program code can reach it (§3.6: builtin prototypes are families with
/// runtime-defined layouts). It takes its reserved intrinsic `serial`, the
/// identity a layout descriptor names through its prototype family, and
/// writes that identity's word, which every shape of the identity reads as its
/// [[Prototype]]: a layout-born object needs no link. From now on every
/// mutation of the prototype sets its own shadow bit, so an unshadowed word
/// proves it still holds exactly the runtime's definitions and [[Prototype]].
///
/// # Safety
/// `proto` is a live, fully built ordinary object with no serial yet.
#[inline(never)]
#[optimize(size)]
pub(crate) unsafe fn seal_runtime_prototype(proto: *mut ObjectHeader, serial: u64) {
    let _no_move = crate::gc::GcSuppressScope::new();
    super::proto_validity::assign_intrinsic_prototype_serial(proto as usize, serial);
    let Some(meta) = name_runtime_prototype(proto, serial) else {
        return;
    };
    // GC_STORE_AUDIT(POINTER_FREE): scalar flag bits, never a reference.
    (*meta).flags |= super::meta_flags::OBJECT_META_FLAG_SEALED_INTRINSIC
        | ((RUNTIME_FAMILY as u64) << super::meta_flags::OBJECT_META_SEALED_FAMILY_SHIFT);
}

/// Give a runtime prototype its reserved intrinsic `serial` as its identity
/// and write the identity's word, without marking it or changing its shape:
/// a prototype born on its own declared layout keeps that layout (the
/// stream-state prototypes). Its layout word then proves nothing about
/// writes; only [`seal_runtime_prototype`] adds that. Returns its meta record.
///
/// # Safety
/// `proto` is a live ordinary object whose serial is 0 or `serial`.
#[inline(never)]
#[optimize(size)]
pub(crate) unsafe fn name_runtime_prototype(
    proto: *mut ObjectHeader,
    serial: u64,
) -> Option<*mut super::ObjectMeta> {
    let _no_move = crate::gc::GcSuppressScope::new();
    let meta = super::object_meta_ensure(
        crate::meta_obj_runtime_born!(proto),
        super::MetaReason::RuntimePrototypeName,
    );
    if meta.is_null() {
        return None;
    }
    debug_assert!((*meta).proto_serial == 0 || (*meta).proto_serial == serial);
    // GC_STORE_AUDIT(POINTER_FREE): a scalar serial, never a reference.
    (*meta).proto_serial = serial;
    super::shapes::write_identity_word(
        serial,
        crate::value::js_nanbox_pointer(proto as i64).to_bits(),
    );
    Some(meta)
}

/// Seal every family's intrinsic prototype of THIS thread's realm. Called once
/// at the end of `populate_global_this_builtins`, after every runtime-owned
/// install, link and the prototype-address priming.
#[optimize(size)]
pub(crate) fn seal_realm(global: *mut ObjectHeader) {
    if global.is_null() {
        return;
    }
    let _no_move = crate::gc::GcSuppressScope::new();
    super::own_override::as_builtin_definition(|| unsafe {
        // A host family (`Buffer`) is sealed when its prototype is built
        // (`seal_built_intrinsic`); reading it here would build it.
        for (family, name) in BuiltinProto::REALM {
            let proto = intrinsic_prototype_addr(family, global, name);
            if proto == 0 {
                continue;
            }
            let Some(header) = crate::value::addr_class::try_read_gc_header(proto) else {
                continue;
            };
            match header.obj_type {
                crate::gc_object_type_pat!() => {
                    trace_seal(family, (*(proto as *const ObjectHeader)).parent_class_id);
                    seal_object(proto as *mut ObjectHeader, family);
                    crate::agent_ptrs::publish_intrinsic_proto(
                        family as usize,
                        proto as *const ObjectHeader,
                    );
                }
                // `Array.prototype` is an Array exotic: its named members live
                // in its property bag, an ordinary object. Materialise the bag
                // now so every later named write reaches a sealed object, and
                // recognise the array itself by its memoized address
                // ([`note_array_prototype_write`]).
                crate::gc_array_type_pat!() => {
                    // DEC 250 (i): it never holds an element, so it keeps no
                    // element room: every element it could gain must then
                    // grow it, and `js_array_grow` refuses that. Its length is
                    // 0 (nothing wrote it before the seal); the allocation's
                    // size lives in its GC header, not in `capacity`.
                    let arr = proto as *mut crate::array::ArrayHeader;
                    debug_assert_eq!((*arr).length, 0);
                    (*arr).capacity = 0;
                    let bag = crate::array::array_property_bag_ensure(arr);
                    seal_object(bag, family);
                    if !bag.is_null() {
                        trace_seal(family, (*bag).parent_class_id);
                        crate::agent_ptrs::publish_intrinsic_proto(family as usize, bag);
                    }
                }
                _ => {}
            }
        }
        super::builtin_namespace::seal_namespaces(global);
    });
}

/// The realm's intrinsic prototype of `family`: the memoized intrinsic cells
/// for Object (the complete intrinsic, never the shape-only sentinel), Array
/// and Function; `globalThis[name].prototype` for the rest.
#[inline(never)]
#[optimize(size)]
unsafe fn intrinsic_prototype_addr(
    family: BuiltinProto,
    global: *mut ObjectHeader,
    name: &str,
) -> usize {
    match family {
        BuiltinProto::Object => crate::array::object_prototype_addr(),
        BuiltinProto::Array => crate::array::array_prototype_addr(),
        BuiltinProto::Function => crate::array::function_prototype_addr(),
        _ => constructor_prototype_addr(global, name),
    }
}

/// `globalThis[name].prototype` while the realm still names its intrinsics.
#[inline(never)]
#[optimize(size)]
unsafe fn constructor_prototype_addr(global: *mut ObjectHeader, name: &str) -> usize {
    let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
    let ctor = super::js_object_get_field_by_name_f64(global, key);
    let ctor_bits = ctor.to_bits();
    if ctor_bits & !crate::value::POINTER_MASK != crate::value::POINTER_TAG {
        return 0;
    }
    let pkey = crate::string::js_string_from_bytes(b"prototype".as_ptr(), 9);
    let proto = super::js_object_get_field_by_name_f64(
        (ctor_bits & crate::value::POINTER_MASK) as usize as *const ObjectHeader,
        pkey,
    );
    let bits = proto.to_bits();
    if bits & !crate::value::POINTER_MASK != crate::value::POINTER_TAG {
        return 0;
    }
    (bits & crate::value::POINTER_MASK) as usize
}

/// The invalidation hook for an ordinary object about to be mutated. One
/// load of the object's meta pointer; a sealed prototype is the only object
/// whose record carries a family.
///
/// # Safety
/// `obj` is a live `ObjectHeader`, or null.
#[inline(always)]
pub(crate) unsafe fn note_write(
    obj: *const ObjectHeader,
    mutation: Mutation<'_>,
    site: &'static str,
) {
    if obj.is_null() {
        return;
    }
    // A1_UNPROVEN: record-first: caller intrinsic-proto membership (kind-A/B question for S0f).
    let meta = crate::meta_read!(crate::meta_obj_runtime_born!(obj as *mut _));
    if meta.is_null() {
        return;
    }
    if (*meta).flags & super::meta_flags::OBJECT_META_FLAG_SEALED_INTRINSIC != 0 {
        note_sealed_write(obj, meta, mutation, site);
    }
}

/// The family a sealed intrinsic's meta flags record (see
/// `OBJECT_META_FLAG_SEALED_INTRINSIC`).
#[inline(always)]
fn sealed_family(flags: u64) -> usize {
    ((flags >> super::meta_flags::OBJECT_META_SEALED_FAMILY_SHIFT)
        & super::meta_flags::OBJECT_META_SEALED_FAMILY_MASK) as usize
}

/// Is the sealed intrinsic whose meta flags are `flags` a builtin prototype
/// born on a complete static layout (`runtime_layouts::builtin_prototypes`):
/// `%Object.prototype%`, `%Function.prototype%` or `%Array.prototype%`'s
/// holder? Its own keys are then exactly its list and overflow, like an
/// ordinary object's. A compare against a fixed set of families.
#[inline(always)]
pub(crate) fn on_static_layout(flags: u64) -> bool {
    const OBJECT: usize = BuiltinProto::Object as usize;
    const FUNCTION: usize = BuiltinProto::Function as usize;
    const ARRAY: usize = BuiltinProto::Array as usize;
    matches!(sealed_family(flags), OBJECT | FUNCTION | ARRAY)
}

/// A write reached a sealed intrinsic: a prototype family's own bit, a
/// runtime prototype's own shadow bit, or a namespace holder or the realm
/// global (`builtin_namespace`). DEC 250 decides first: on a locked family
/// the program's write of an image member throws before anything changes;
/// what the decision admits is then modelled as before (the shadow bit, the
/// array-methods epoch, the namespace withdraw).
#[cold]
#[inline(never)]
#[optimize(size)]
unsafe fn note_sealed_write(
    obj: *const ObjectHeader,
    meta: *mut super::ObjectMeta,
    mutation: Mutation<'_>,
    site: &'static str,
) {
    let family = sealed_family((*meta).flags);
    let definition = super::own_override::in_builtin_definition();
    if locked_family(family) && !definition {
        if let Some(violation) = locked_violation(obj, meta, mutation) {
            throw_locked(family, &violation);
        }
    }
    // The realm families; a host family (`Buffer`) is sealed as a runtime
    // prototype (`seal_built_host_intrinsic`).
    if family < BuiltinProto::REALM_INTRINSICS {
        // A runtime definition that ADDS a key to an add-locked prototype
        // after its seal changes what an absent-key answer (a pristine-guarded
        // miss, the lock's "never gains a key") already said: it withdraws
        // like the program's write would. Any other definition (the image's
        // own population, a value refresh) is the builtin itself.
        if definition && add_locked(family) && adds_key(obj, mutation) {
            withdraw_family(family, site);
        } else {
            note_family_write(family, site);
        }
    } else if family == RUNTIME_FAMILY {
        if !super::own_override::in_builtin_definition() {
            super::shadow::set_bit(obj.cast_mut());
        }
    } else if (GENERATOR_FAMILY..=REGEXP_STRING_ITERATOR_FAMILY).contains(&family) {
        // The generator and the Map/Set/String/RegExp String iterator
        // prototypes: locked above (image members only); an add is the
        // program's own member and nothing in the runtime answers from its
        // absence.
    } else if family == ITERATOR_FAMILY || family == ARRAY_ITERATOR_FAMILY {
        // Locked above (adds included); nothing else models them, so a
        // definition add after the seal has no answer to withdraw: it aborts.
        if definition {
            if let Some(message) = iterator_definition_add(family, obj, mutation) {
                abort_runtime_bug(&message);
            }
        }
    } else {
        let field = match mutation {
            Mutation::Replace(slot) => Some(slot),
            _ => None,
        };
        super::builtin_namespace::note_write(obj as *mut ObjectHeader, family, field, site);
    }
}

/// DEC 250's lock set: the sealed families whose image members the program
/// never writes. A pure function of the family the meta record already
/// carries: the `BuiltinProto` families; not the namespace holders and
/// constructor statics (owner ruling 2026-11-03), not `console`, not the realm
/// global, not a runtime prototype (URL, timers, …).
#[inline(always)]
pub(crate) fn locked_family(family: usize) -> bool {
    lock_holder(family).is_some()
}

/// The image member `mutation` would write on the locked intrinsic `obj`, if
/// any. Reached by pre-mutation reports only (a `Publication` decides
/// nothing): the first one records the image key count, so it must still see
/// the image layout.
unsafe fn locked_violation(
    obj: *const ObjectHeader,
    meta: *mut super::ObjectMeta,
    mutation: Mutation<'_>,
) -> Option<Violation> {
    if matches!(mutation, Mutation::Publication) {
        return None;
    }
    let keys = own_keys(obj);
    let image = image_key_count(meta, keys);
    let Some(holder) = lock_holder(sealed_family((*meta).flags)) else {
        return None;
    };
    // The one decision (`perry_abi::lock::refuses`) with the key's class: an
    // image position is `Image`, a key that is not own `Absent`, and
    // `constructor` `Constructor` in either case (owner ruling 2026-11-03).
    // The key text is built only when the class alone would be refused.
    let decide = |class: LockKey,
                  m: LockMutation,
                  text: &dyn Fn() -> String,
                  wrap: fn(String) -> Violation| {
        if !refuses(holder, class, m) {
            return None;
        }
        let k = text();
        let class = if k == ".constructor" {
            LockKey::Constructor
        } else {
            class
        };
        refuses(holder, class, m).then(|| wrap(k))
    };
    match mutation {
        Mutation::Relink => {
            refuses(holder, LockKey::Absent, LockMutation::Relink).then_some(Violation::Relink)
        }
        Mutation::Replace(slot) => {
            let pos = image_position_of_slot(keys, image, slot)?;
            decide(
                LockKey::Image,
                LockMutation::Replace,
                &|| key_display(keys, pos),
                Violation::Replace,
            )
        }
        Mutation::Delete(key) => {
            let pos = image_position_of_key(keys, image, key)?;
            decide(
                LockKey::Image,
                LockMutation::Delete,
                &|| key_display(keys, pos),
                Violation::Delete,
            )
        }
        Mutation::Redefine(key) => match image_position_of_key(keys, image, key) {
            Some(pos) => decide(
                LockKey::Image,
                LockMutation::Redefine,
                &|| key_display(keys, pos),
                Violation::Redefine,
            ),
            // Defining a key that is not own adds it.
            None if image_position_of_key(keys, keys.count(), key).is_none() => decide(
                LockKey::Absent,
                LockMutation::Add,
                &|| key_text(key),
                Violation::Add,
            ),
            None => None,
        },
        Mutation::Add(key) => decide(
            LockKey::Absent,
            LockMutation::Add,
            &|| key_text(key),
            Violation::Add,
        ),
        Mutation::Stamp(published) if refuses(holder, LockKey::Absent, LockMutation::Add) => {
            let k = stamped_add(obj, keys, published)?;
            decide(
                LockKey::Absent,
                LockMutation::Add,
                &|| k.clone(),
                Violation::Add,
            )
        }
        Mutation::Stamp(_) | Mutation::Restructure | Mutation::Publication => None,
    }
}

/// The number of own keys the realm image gave `meta`'s object. Recorded at
/// the program's first mutation (the layout is still the image's: the
/// runtime's own installs returned before reaching this), read back after.
/// Image keys are never deleted, so they stay the prefix of every later key
/// list; a program-added key is a later position.
unsafe fn image_key_count(meta: *mut super::ObjectMeta, keys: super::ObjectKeys) -> u32 {
    use super::meta_flags::{
        OBJECT_META_IMAGE_KEYS_MASK as MASK, OBJECT_META_IMAGE_KEYS_SHIFT as SHIFT,
    };
    let recorded = ((*meta).flags >> SHIFT) & MASK;
    if recorded != 0 {
        return (recorded - 1) as u32;
    }
    let count = keys.count().min(MASK as u32 - 1);
    // GC_STORE_AUDIT(POINTER_FREE): a scalar count, never a reference.
    (*meta).flags |= (u64::from(count) + 1) << SHIFT;
    count
}

/// The object's own keys in position order: its overflow table when it has
/// one, else its layout's key list.
unsafe fn own_keys(obj: *const ObjectHeader) -> super::ObjectKeys {
    let overflow = super::overflow::keys_array(obj);
    if !overflow.is_null() {
        return super::ObjectKeys::owned(overflow);
    }
    let id = super::layout_desc::layout_id_of_word((*obj).parent_class_id);
    match super::shapes::shape_record_for_word(crate::codegen_abi::LayoutWord(id)) {
        Some(record) => super::object_keys_from_shape_record(obj, record),
        None => super::ObjectKeys::NONE,
    }
}

/// The image position whose storage slot is `slot`, if any. A position whose
/// entry is no key (a deleted key's tombstone in an overflow table keeps its
/// storage slot) is no member: a store into its slot re-adds the key.
unsafe fn image_position_of_slot(keys: super::ObjectKeys, image: u32, slot: usize) -> Option<u32> {
    if keys.is_null() {
        return None;
    }
    (0..image.min(keys.count())).find(|&pos| {
        super::key_attrs::key_storage_slot(keys.arr(), pos) as usize == slot
            && entry_key(keys, pos).is_some()
    })
}

/// Key `pos` of `keys` as a key value: a string key is its string-tagged
/// header, a symbol key its pointer-tagged symbol. Anything else (a deleted
/// key's tombstone reads as `undefined`) names no key.
unsafe fn entry_key(keys: super::ObjectKeys, pos: u32) -> Option<f64> {
    let value = keys.get(pos);
    let bits = value.bits();
    if value.is_string()
        || (value.is_pointer() && crate::symbol::js_is_symbol(f64::from_bits(bits)) != 0)
    {
        Some(f64::from_bits(bits))
    } else {
        None
    }
}

/// The image position of `key`, if `key` is an own key below the image count.
/// Nothing here allocates: the funnels report before the key may be coerced
/// (and the object moved).
unsafe fn image_position_of_key(keys: super::ObjectKeys, image: u32, key: Key<'_>) -> Option<u32> {
    if keys.is_null() || image == 0 {
        return None;
    }
    let count = keys.count();
    let pos = match key {
        Key::Str(ptr) => super::keys_find_property_slot_by_key_ptr(keys, count, ptr)?,
        Key::Bytes(bytes) => super::keys_find_property_slot_by_bytes(keys, count, bytes)?,
        Key::Value(value) => {
            // A string or symbol key is its header; a number is never an
            // image member (no intrinsic owns an index key).
            let is_symbol = crate::symbol::js_is_symbol(value) != 0;
            if !is_symbol && !crate::JSValue::from_bits(value.to_bits()).is_string() {
                return None;
            }
            if is_symbol {
                // A symbol key is stored as its tagged header in the key list.
                super::shaped_symbols::position_in(
                    keys,
                    (value.to_bits() & crate::value::POINTER_MASK) as usize,
                )?
            } else {
                let ptr = crate::value::js_nanbox_get_pointer(value) as *const crate::StringHeader;
                super::keys_find_property_slot_by_key_ptr(keys, count, ptr)?
            }
        }
    };
    (pos < image).then_some(pos)
}

/// The key a stamp to `published` appends to `obj`, if it appends one: the
/// published key list is longer than the own one. Nothing allocates a GC
/// object (the message text is Rust).
unsafe fn stamped_add(
    obj: *const ObjectHeader,
    keys: super::ObjectKeys,
    published: u32,
) -> Option<String> {
    let id = super::layout_desc::layout_id_of_word(published);
    let record = super::shapes::shape_record_for_word(crate::codegen_abi::LayoutWord(id))?;
    let next = super::object_keys_from_shape_record(obj, record);
    let had = if keys.is_null() { 0 } else { keys.count() };
    (!next.is_null() && next.count() > had).then(|| key_display(next, had))
}

/// A funnel key for a message: `.name`, `[Symbol(description)]` or `[n]`.
unsafe fn key_text(key: Key<'_>) -> String {
    let value = match key {
        Key::Bytes(bytes) => return format!(".{}", String::from_utf8_lossy(bytes)),
        Key::Str(ptr) if crate::symbol::may_be_symbol_header(ptr as *const u8) => {
            crate::value::js_nanbox_pointer(ptr as i64)
        }
        Key::Str(ptr) => crate::value::js_nanbox_string(ptr as i64),
        Key::Value(value) => value,
    };
    if crate::symbol::js_is_symbol(value) != 0 {
        let description = crate::symbol::symbol_description_text(
            crate::value::js_nanbox_get_pointer(value) as *const _,
        )
        .map(|text| String::from_utf8_lossy(&text).into_owned())
        .unwrap_or_default();
        return format!("[Symbol({description})]");
    }
    if crate::JSValue::from_bits(value.to_bits()).is_number() {
        return format!("[{value}]");
    }
    match super::property_ops::set::key_to_rust_string(value) {
        Some(name) => format!(".{name}"),
        None => String::from(".?"),
    }
}

/// Key `pos` of `keys` for a message: `.name`, or `[Symbol(description)]`.
unsafe fn key_display(keys: super::ObjectKeys, pos: u32) -> String {
    match entry_key(keys, pos) {
        Some(key) => key_text(Key::Value(key)),
        None => String::from(".?"),
    }
}

/// The name of the locked intrinsic of `family` as a program spells it.
fn locked_object_name(family: usize) -> String {
    if family < FAMILIES {
        format!("{}.prototype", BuiltinProto::ALL[family].1)
    } else if family == ITERATOR_FAMILY {
        String::from("Iterator.prototype")
    } else if family == ARRAY_ITERATOR_FAMILY {
        String::from("%ArrayIteratorPrototype%")
    } else if family == GENERATOR_FAMILY {
        String::from("%GeneratorPrototype%")
    } else if family == ASYNC_GENERATOR_FAMILY {
        String::from("%AsyncGeneratorPrototype%")
    } else if family == MAP_ITERATOR_FAMILY {
        String::from("%MapIteratorPrototype%")
    } else if family == SET_ITERATOR_FAMILY {
        String::from("%SetIteratorPrototype%")
    } else if family == STRING_ITERATOR_FAMILY {
        String::from("%StringIteratorPrototype%")
    } else if family == REGEXP_STRING_ITERATOR_FAMILY {
        String::from("%RegExpStringIteratorPrototype%")
    } else {
        crate::codegen_abi::BUILTIN_NAMESPACES[family - super::builtin_namespace::FIRST].to_string()
    }
}

/// DEC 250: the `TypeError` a program meets when it writes a built-in member
/// (CWN Q1: explanatory, so a polyfill or test shim learns what works).
#[cold]
#[inline(never)]
#[optimize(size)]
fn throw_locked(family: usize, violation: &Violation) -> ! {
    const WHY: &str = "built-in objects are read-only in a compiled program";
    // The hint names what still works: on an add-locked prototype nothing does.
    let add = if add_locked(family) {
        ""
    } else {
        " (adding a new member is allowed)"
    };
    let object = locked_object_name(family);
    let message = match violation {
        Violation::Replace(member) => {
            format!("Cannot replace built-in member {object}{member}: {WHY}{add}")
        }
        Violation::Delete(member) => {
            format!("Cannot delete built-in member {object}{member}: {WHY}{add}")
        }
        Violation::Redefine(member) => {
            format!("Cannot redefine built-in member {object}{member}: {WHY}{add}")
        }
        Violation::Relink => format!("Cannot change the prototype of built-in {object}: {WHY}"),
        Violation::Add(member) => format!("Cannot add member {object}{member}: {WHY}"),
        Violation::AddIndex if family == BuiltinProto::Array as usize => format!(
            "Cannot add an array index to built-in {object}: {WHY} (named members may be added)"
        ),
        Violation::AddIndex => format!("Cannot add an array index to built-in {object}: {WHY}"),
    };
    let text = crate::string::js_string_from_bytes(message.as_ptr(), message.len() as u32);
    crate::exception::throw_core(crate::error::CoreError::type_error(text))
}

/// The value-store hook (slot and spill stores): [`note_write`] for a sealed
/// builtin prototype.
///
/// Inline: one meta-pointer load and one flag test (a sealed builtin
/// prototype is always marked); everything else is out of line.
///
/// # Safety
/// `obj` is a live `ObjectHeader`.
#[inline(always)]
pub(crate) unsafe fn note_value_store(
    obj: *const ObjectHeader,
    field_index: usize,
    spill: bool,
    site: &'static str,
) {
    // A1_UNPROVEN: record-first: caller intrinsic-proto membership (kind-A/B question for S0f).
    let meta = crate::meta_read!(crate::meta_obj_runtime_born!(obj as *mut _));
    if meta.is_null() || (*meta).flags & super::OBJECT_META_FLAG_IS_PROTOTYPE == 0 {
        return;
    }
    note_prototype_value_store(obj, meta, field_index, spill, site);
}

#[cold]
#[inline(never)]
#[optimize(size)]
unsafe fn note_prototype_value_store(
    obj: *const ObjectHeader,
    meta: *const super::ObjectMeta,
    field_index: usize,
    spill: bool,
    site: &'static str,
) {
    crate::dyn_eval_hooks::note_proto_write();
    if (*meta).flags & super::meta_flags::OBJECT_META_FLAG_SEALED_INTRINSIC != 0 {
        // A value store writes an existing storage slot by construction.
        note_sealed_write(obj, meta.cast_mut(), Mutation::Replace(field_index), site);
    }
    // A method body replaced in place publishes no layout (§3.6).
    super::shadow::note_prototype_slot_store(obj as *mut ObjectHeader, field_index, spill);
}

/// [`note_write`] for an owner of unknown kind (descriptor, delete and
/// `[[Prototype]]` funnels take a bare address): an ordinary object answers
/// from its meta record, an Array from the realm's `%Array.prototype%`
/// address. Anything else is not a sealed prototype.
#[inline(never)]
#[optimize(size)]
pub(crate) fn note_write_addr(addr: usize, mutation: Mutation<'_>, site: &'static str) {
    if addr == 0 || !crate::value::addr_class::is_above_handle_band(addr) {
        return;
    }
    let Some(header) = (unsafe { crate::value::addr_class::try_read_gc_header(addr) }) else {
        return;
    };
    match header.obj_type {
        crate::gc_object_type_pat!() => unsafe {
            note_write(addr as *const ObjectHeader, mutation, site)
        },
        crate::gc_array_type_pat!() => note_array_prototype_write(addr, mutation, site),
        crate::gc::GC_TYPE_CLOSURE => unsafe {
            note_write(
                (*(addr as *const crate::closure::ClosureHeader)).props,
                mutation,
                site,
            );
        },
        _ => {}
    }
}

/// The hook for an Array exotic about to take a named-property write, delete
/// or `[[Prototype]]` relink: `%Array.prototype%`'s own named storage may not
/// be its (sealed) bag yet. Cold: arrays rarely take named properties.
#[inline(never)]
#[optimize(size)]
pub(crate) fn note_array_prototype_write(arr: usize, mutation: Mutation<'_>, site: &'static str) {
    if arr == 0 {
        return;
    }
    // `seal_realm` materialised `%Array.prototype%`'s named bag and sealed
    // it, so the bag's meta record names the family; an ordinary array's bag
    // (or none) is not sealed. The realm's memoized prototype address is not
    // consulted: it may be unresolved when the realm was reached another way.
    // SAFETY: `clean_arr_ptr` returns the live head of a grown array.
    unsafe {
        let live = crate::array::clean_arr_ptr(arr as *const crate::array::ArrayHeader);
        if !live.is_null() {
            note_write(crate::array::array_property_bag(live), mutation, site);
        }
    }
}

/// DEC 250: would the lock refuse the program's write of property `key` (a
/// property key: string, symbol or number) on `receiver`? The same decision
/// the store funnels take before they throw ([`note_sealed_write`],
/// [`refuse_index_add`]); here it is only answered, for `Reflect.set` and
/// `Reflect.defineProperty`, which report a refused `[[Set]]` /
/// `[[DefineOwnProperty]]` as `false` and never throw for it (ECMA-262
/// 28.1.12, 28.1.13). A write the lock admits goes on to the ordinary
/// operation, which still answers everything else (non-writable,
/// non-extensible, setters). Allocation-free: the operands stay where they are.
pub(crate) fn lock_refuses_write(receiver: f64, key: f64) -> bool {
    let bits = receiver.to_bits();
    if bits & !crate::value::POINTER_MASK != crate::value::POINTER_TAG {
        return false;
    }
    let addr = (bits & crate::value::POINTER_MASK) as usize;
    if addr == 0 || !crate::value::addr_class::is_above_handle_band(addr) {
        return false;
    }
    let Some(header) = (unsafe { crate::value::addr_class::try_read_gc_header(addr) }) else {
        return false;
    };
    // SAFETY: `addr` is a live tracked object of the matched type.
    unsafe {
        match header.obj_type {
            crate::gc_object_type_pat!() => {
                if property_key_is_index(key) && addr == crate::array::object_prototype_addr() {
                    return refuses(
                        LockHolder::Proto(BuiltinProto::Object),
                        LockKey::Index,
                        LockMutation::Add,
                    ) && index_add_refused();
                }
                sealed_write_refused(addr as *const ObjectHeader, key)
            }
            crate::gc_array_type_pat!() => {
                let live = crate::array::clean_arr_ptr(addr as *const crate::array::ArrayHeader);
                if live.is_null() {
                    return false;
                }
                if property_key_is_index(key)
                    && live as usize == crate::array::array_prototype_addr_if_resolved()
                {
                    return refuses(
                        LockHolder::Proto(BuiltinProto::Array),
                        LockKey::Index,
                        LockMutation::Add,
                    ) && index_add_refused();
                }
                sealed_write_refused(crate::array::array_property_bag(live), key)
            }
            crate::gc::GC_TYPE_CLOSURE => {
                sealed_write_refused((*(addr as *const crate::closure::ClosureHeader)).props, key)
            }
            _ => false,
        }
    }
}

/// [`note_sealed_write`]'s decision for a definition of `key` on `obj` (a
/// sealed intrinsic's holder, or any other object, which no lock covers).
///
/// # Safety
/// `obj` is a live `ObjectHeader`, or null.
unsafe fn sealed_write_refused(obj: *const ObjectHeader, key: f64) -> bool {
    if obj.is_null() || super::own_override::in_builtin_definition() {
        return false;
    }
    let meta = crate::meta_read!(crate::meta_obj_runtime_born!(obj as *mut _));
    if meta.is_null() || (*meta).flags & super::meta_flags::OBJECT_META_FLAG_SEALED_INTRINSIC == 0 {
        return false;
    }
    locked_family(sealed_family((*meta).flags))
        && locked_violation(obj, meta, Mutation::Redefine(Key::Value(key))).is_some()
}

/// Is the property key `key` an array index (a canonical decimal in
/// `0..=2^32-2`, as a string or as the number itself)?
fn property_key_is_index(key: f64) -> bool {
    let value = crate::JSValue::from_bits(key.to_bits());
    if value.is_number() {
        let n = value.as_number();
        return n >= 0.0 && n <= 4_294_967_294.0 && n.fract() == 0.0;
    }
    let mut sso = [0u8; crate::value::SHORT_STRING_MAX_LEN];
    // SAFETY: `value` is a live property key (string, symbol or number).
    unsafe { crate::string::js_string_key_bytes(value, &mut sso) }
        .is_some_and(|bytes| super::canonical_array_index_bytes(bytes).is_some())
}

/// Mark the written intrinsic's own shadow bit. A site's chain proof reads
/// the actual parent word; no write propagates state into other families.
#[cold]
#[inline(never)]
#[optimize(size)]
fn note_family_write(family: usize, site: &'static str) {
    if super::own_override::in_builtin_definition() {
        return;
    }
    withdraw_family(family, site);
}

/// Does `mutation` give the sealed `obj` a key it does not own?
unsafe fn adds_key(obj: *const ObjectHeader, mutation: Mutation<'_>) -> bool {
    added_key(obj, mutation).is_some()
}

/// The key `mutation` gives the sealed `obj` that it does not own, for a
/// message. A pure read of its own keys: it records no image key count (a
/// runtime definition must not fix the image prefix the program's first write
/// records).
unsafe fn added_key(obj: *const ObjectHeader, mutation: Mutation<'_>) -> Option<String> {
    match mutation {
        Mutation::Add(key) => Some(key_text(key)),
        Mutation::Redefine(key) => {
            let keys = own_keys(obj);
            let count = if keys.is_null() { 0 } else { keys.count() };
            image_position_of_key(keys, count, key)
                .is_none()
                .then(|| key_text(key))
        }
        Mutation::Stamp(published) => stamped_add(obj, own_keys(obj), published),
        _ => None,
    }
}

/// A runtime definition that adds a key to an iterator prototype after its
/// seal: no fact models those prototypes, so nothing could withdraw an answer
/// a compiled read already took. Such an install is a runtime bug, never a
/// program behaviour (DEC 250 Q2: the program cannot add there); the message
/// names the prototype and the key.
unsafe fn iterator_definition_add(
    family: usize,
    obj: *const ObjectHeader,
    mutation: Mutation<'_>,
) -> Option<String> {
    let key = added_key(obj, mutation)?;
    Some(format!(
        "perry runtime bug: a runtime definition adds {}{key} after the prototype was sealed",
        locked_object_name(family)
    ))
}

#[cold]
#[inline(never)]
fn abort_runtime_bug(message: &str) -> ! {
    eprintln!("{message}");
    std::process::abort()
}

/// Withdraw `family`: the written intrinsic's shadow bit and every derived
/// answer that keys on it (the invalidation funnel of [`note_family_write`]).
#[cold]
#[inline(never)]
#[optimize(size)]
fn withdraw_family(family: usize, site: &'static str) {
    crate::dyn_eval_hooks::note_proto_write();
    let set = |k: usize| -> bool {
        let p = crate::agent_ptrs::intrinsic_proto(k) as *mut ObjectHeader;
        // SAFETY: a live intrinsic of this agent, or the static stand-in
        // (never written: nothing writes an intrinsic before sealing).
        unsafe {
            if std::ptr::eq(
                p.cast::<u64>(),
                crate::agent_ptrs::PERRY_PRISTINE_PROTO.as_ptr(),
            ) {
                return false;
            }
            let fresh = !super::layout_desc::word_is_shadowed((*p).parent_class_id);
            super::shadow::set_bit(p);
            fresh
        }
    };
    let first = family < FAMILIES && set(family);
    super::own_override::note_builtin_proto_modified();
    if family == BuiltinProto::Array as usize || family == BuiltinProto::Object as usize {
        crate::array::note_array_methods_modified();
    }
    // A program that patches nothing must print nothing.
    if first {
        trace_withdrawal(family, site);
    }
}

/// `PERRY_BUILTIN_PROTO_TRACE=1`: the layout word each sealed intrinsic
/// starts with (it must be an ordinary layout, or its family is never
/// pristine).
#[cold]
fn trace_seal(family: BuiltinProto, word: u32) {
    // SAFETY: a NUL-terminated literal; `getenv` only reads the environment.
    if unsafe { libc::getenv(c"PERRY_BUILTIN_PROTO_TRACE".as_ptr()) }.is_null() {
        return;
    }
    eprintln!("[builtin-proto] sealed {family:?} word={word:#x}");
}

/// `PERRY_BUILTIN_PROTO_TRACE=1` names the funnel of each first withdrawal.
#[cold]
#[inline(never)]
#[optimize(size)]
fn trace_withdrawal(family: usize, site: &'static str) {
    // SAFETY: a NUL-terminated literal; `getenv` only reads the environment.
    if unsafe { libc::getenv(c"PERRY_BUILTIN_PROTO_TRACE".as_ptr()) }.is_null() {
        return;
    }
    let name = BuiltinProto::ALL.get(family).map_or("?", |(_, name)| name);
    for part in [
        "[builtin-proto] ",
        name,
        ".prototype withdrawn at ",
        site,
        "\n",
    ] {
        // SAFETY: writes a borrowed byte string to fd 2.
        unsafe { libc::write(2, part.as_ptr().cast(), part.len() as _) };
    }
}

/// The builtin family and receiver-owned custom-link fact, obtained from
/// one classification of the value and (for a heap receiver) its header.
enum ReceiverFact {
    Builtin(BuiltinProto, bool),
    Proxy,
}

#[inline]
fn receiver_fact(recv: f64) -> Option<ReceiverFact> {
    let bits = recv.to_bits();
    let v = crate::value::JSValue::from_bits(bits);
    if v.is_any_string() {
        return Some(ReceiverFact::Builtin(BuiltinProto::String, false));
    }
    if v.is_number() {
        return Some(ReceiverFact::Builtin(BuiltinProto::Number, false));
    }
    if v.is_bool() {
        return Some(ReceiverFact::Builtin(BuiltinProto::Boolean, false));
    }
    if v.is_bigint() {
        return Some(ReceiverFact::Builtin(BuiltinProto::BigInt, false));
    }
    let mut header = crate::value::addr_class::pointer_value_header(bits)?;
    if matches!(
        header.obj_type,
        crate::gc_array_type_pat!() | crate::gc::GC_TYPE_LAZY_ARRAY
    ) && header.gc_flags & crate::gc::GC_FLAG_FORWARDED != 0
    {
        let addr = (bits & crate::value::POINTER_MASK) as usize;
        let array = crate::array::clean_arr_ptr(addr as *const crate::array::ArrayHeader);
        if array.is_null() {
            return None;
        }
        // SAFETY: clean_arr_ptr returns a live GC-headed array.
        header = unsafe { crate::value::addr_class::honest_header(array as usize) };
    }
    let family = match header.obj_type {
        crate::gc::GC_TYPE_PROXY => return Some(ReceiverFact::Proxy),
        crate::gc_array_type_pat!() | crate::gc::GC_TYPE_LAZY_ARRAY => BuiltinProto::Array,
        crate::gc::GC_TYPE_MAP => BuiltinProto::Map,
        crate::gc::GC_TYPE_SET => BuiltinProto::Set,
        crate::gc::GC_TYPE_DATE_CELL => BuiltinProto::Date,
        crate::gc::GC_TYPE_CLOSURE => BuiltinProto::Function,
        _ => return None,
    };
    // The receiver's own link: an array's header bit; a Map, Set or Date
    // cell's meta record (its own `[[Prototype]]` or an own key lives there).
    let own = if family == BuiltinProto::Array {
        header._reserved & crate::gc::GC_ARRAY_CUSTOM_PROTO != 0
    } else {
        let addr = (bits & crate::value::POINTER_MASK) as usize;
        // SAFETY: `header` is this live cell's validated header.
        unsafe { super::cell_meta::builtin_cell_has_meta(addr, header.obj_type) }
    };
    Some(ReceiverFact::Builtin(family, own))
}

/// The intrinsic's shadow bit and the receiver's own prototype link are
/// the complete builtin proof; both use the same receiver classification.
#[inline]
pub(crate) fn receiver_pristine(recv: f64) -> bool {
    match receiver_fact(recv) {
        Some(ReceiverFact::Proxy | ReceiverFact::Builtin(_, true)) => false,
        Some(ReceiverFact::Builtin(family, false)) => pristine(family),
        None => true,
    }
}

/// Is `value` one of the runtime's own builtin method closures?
pub(crate) fn is_builtin_method(value: f64) -> bool {
    let bits = value.to_bits();
    if bits & !crate::value::POINTER_MASK != crate::value::POINTER_TAG {
        return false;
    }
    let ptr = (bits & crate::value::POINTER_MASK) as usize;
    if !crate::closure::is_closure_ptr(ptr) {
        return false;
    }
    crate::closure::closure_info(ptr as *const crate::closure::ClosureHeader)
        .is_some_and(|info| info.flags & crate::closure::FN_BUILTIN != 0)
}

/// THE shared generic path of a failed `Pristine` proof: ECMA-262's
/// `Call(Get(recv, name), recv, args)`.
///
/// * a value that is not one of the runtime's builtin method closures (a user
///   function, a bound function, a non-callable, `undefined` after a delete)
///   is called with `recv` as `this`, which throws the spec TypeError for a
///   non-callable — `Some(result)`;
/// * a builtin method closure answers `None`: the caller continues with its
///   own native implementation of `name`. A builtin is never re-entered as a
///   value from here, so a saved original (`orig.apply(this, args)` inside a
///   wrapper installed over it) cannot recurse back into this lookup.
///
/// # Safety
/// `args` holds `argc` NaN-boxed values (or is null with `argc == 0`).
pub(crate) unsafe fn redirect_call(
    recv: f64,
    name: &[u8],
    args: *const f64,
    argc: usize,
) -> Option<f64> {
    select_and_call(recv, recv, name, args, argc, false, None)
}

/// `Call(Get(recv, name), recv, args)` whatever the method is: the ordinary
/// call for a primitive receiver an Array site refused. Its `Get` may select
/// ANOTHER kind's builtin (`String.prototype.at` for a String), which must run
/// as itself, never as the Array body the site was folded to.
///
/// # Safety
/// `recv` and `args` are NaN-boxed values live at the call.
pub(crate) unsafe fn complete_call(recv: f64, name: &[u8], args: &[f64]) -> f64 {
    select_and_call(recv, recv, name, args.as_ptr(), args.len(), true, None)
        .expect("a complete call always calls")
}

/// `Call(Get(holder, name), this, args)` — the one body behind every generic
/// entry. With `call_builtin == false` a selected runtime builtin answers
/// `None` (the caller runs it natively) — and, given `refresh`, re-reads the
/// caller's receiver and arguments from their roots into it, since the `Get`
/// may have run a getter that collected; with `true` it is called as a value
/// (a borrowed call whose builtin must run on `this`).
#[inline(never)]
#[optimize(size)]
unsafe fn select_and_call(
    holder: f64,
    this: f64,
    name: &[u8],
    args: *const f64,
    argc: usize,
    call_builtin: bool,
    refresh: Option<(&mut f64, &mut [f64])>,
) -> Option<f64> {
    let scope = crate::gc::RuntimeHandleScope::new();
    let this_h = scope.root_nanbox_f64(this);
    let original: &[f64] = if argc > 0 && !args.is_null() {
        std::slice::from_raw_parts(args, argc)
    } else {
        &[]
    };
    let arg_handles = scope.root_nanbox_f64_slice(original);
    let selected = super::method_call::spec_get_value(holder, name);
    // A Proxy cannot run a dense builtin body: even the original builtin
    // must keep the receiver intact and perform its operations through traps.
    if !call_builtin
        && is_builtin_method(selected)
        && crate::proxy_hooks::proxy_is_proxy(this_h.get_nanbox_f64()) == 0
    {
        if let Some((recv, out)) = refresh {
            *recv = this_h.get_nanbox_f64();
            out.copy_from_slice(&crate::gc::RuntimeHandleScope::refreshed_nanbox_f64_slice(
                &arg_handles,
            ));
        }
        return None;
    }
    let selected_h = scope.root_nanbox_f64(selected);
    let values = crate::gc::RuntimeHandleScope::refreshed_nanbox_f64_slice(&arg_handles);
    if selected_h.get_nanbox_f64().to_bits() & !crate::value::POINTER_MASK
        != crate::value::POINTER_TAG
    {
        // undefined, null, a number, a boolean …: not callable.
        crate::closure::throw_not_callable();
    }
    Some(super::method_call::js_method_call_value(
        selected_h.get_nanbox_f64(),
        this_h.get_nanbox_f64(),
        values.as_ptr(),
        values.len(),
    ))
}

/// [`redirect_call`] for a checked entry that runs the builtin body itself
/// (`builtin_proto_entries.rs`): `Some(result)` when the method actually
/// there is not the runtime's builtin; `None` when it still is, with `recv`
/// and `args` re-read from their roots — the `Get` may run a getter, which
/// may collect.
///
/// # Safety
/// `recv` and `args` are NaN-boxed values.
#[cold]
#[inline(never)]
#[optimize(size)]
pub(crate) unsafe fn redirect_or_refresh(
    recv: &mut f64,
    name: &[u8],
    args: &mut [f64],
) -> Option<f64> {
    let original: Vec<f64> = args.to_vec();
    let this = *recv;
    select_and_call(
        this,
        this,
        name,
        original.as_ptr(),
        original.len(),
        false,
        Some((recv, args)),
    )
}

/// [`redirect_call`] for a spread call: `args_array` is the array holding
/// every argument (the `js_native_call_method_apply` shape).
///
/// # Safety
/// `args_array` is 0 or a live array.
pub(crate) unsafe fn redirect_call_array(recv: f64, name: &[u8], args_array: i64) -> Option<f64> {
    let arr = args_array as *const crate::array::ArrayHeader;
    let len = if arr.is_null() {
        0
    } else {
        crate::array::js_array_length(arr) as usize
    };
    let buf: Vec<f64> = (0..len)
        .map(|i| crate::array::js_array_get_f64(arr, i as u32))
        .collect();
    redirect_call(recv, name, buf.as_ptr(), buf.len())
}

/// Emitted code's generic arm for a borrowed call whose `Pristine` proof
/// failed: `Call(Get(holder, name), this, args)` where `holder` is the
/// family's intrinsic prototype as the program reads it NOW. Used by the
/// `Array.prototype.m.call(x, …)` / `Object.prototype.m.call(x, …)` lowerings.
/// Returns `None` when the selected value is still the builtin.
///
/// # Safety
/// As [`redirect_call`].
pub(crate) unsafe fn redirect_borrowed_call(
    family: BuiltinProto,
    this: f64,
    name: &[u8],
    args: *const f64,
    argc: usize,
) -> Option<f64> {
    let holder = borrowed_holder(family)?;
    select_and_call(holder, this, name, args, argc, false, None)
}

/// The intrinsic prototype a borrowed call reads its method from, NaN-boxed.
#[inline(never)]
fn borrowed_holder(family: BuiltinProto) -> Option<f64> {
    let holder = match family {
        BuiltinProto::Array => crate::array::array_prototype_addr(),
        BuiltinProto::Object => crate::array::object_prototype_addr(),
        _ => return None,
    };
    (holder != 0).then(|| crate::value::js_nanbox_pointer(holder as i64))
}

/// What a failed `Pristine` proof reads the method from, as emitted code
/// passes it to [`js_builtin_proto_generic_call`].
pub const GENERIC_RECEIVER: i32 = 0;
/// `%Array.prototype%`, read at the call (`Array.prototype.m.call(x, …)`).
pub const GENERIC_ARRAY_PROTOTYPE: i32 = 1;
/// `%Object.prototype%`, read at the call (`Object.prototype.m.call(x, …)`).
pub const GENERIC_OBJECT_PROTOTYPE: i32 = 2;

/// THE shared out-of-line generic arm of an emitted `Pristine` guard: one
/// call with the operands the site already evaluated. `mode` names where the
/// method is read ([`GENERIC_RECEIVER`] …); `method_id` is the static
/// dispatch id the universal dispatcher takes.
///
/// A selected value that is still the runtime's builtin runs that builtin:
/// for a receiver read, through the by-name natives (exactly the dispatcher
/// arm the site's own-override guard already uses); for a prototype read,
/// as a function value with `this` = the receiver (the borrowed call).
///
/// # Safety
/// `args` holds `argc` NaN-boxed values (or is null with `argc == 0`).
#[no_mangle]
pub unsafe extern "C-unwind" fn js_builtin_proto_generic_call(
    mode: i32,
    recv: f64,
    method_id: i64,
    args: *const f64,
    argc: usize,
) -> f64 {
    let mut scratch = [0u8; crate::value::SHORT_STRING_MAX_LEN];
    let Some(name_ref) = crate::string::perry_string_ref_from_dispatch_id(method_id, &mut scratch)
    else {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    };
    let name = std::slice::from_raw_parts(name_ref.ptr, name_ref.len);
    // The name may live in `scratch`; own it across the calls below.
    let name: Vec<u8> = name.to_vec();
    let family = match mode {
        GENERIC_ARRAY_PROTOTYPE => Some(BuiltinProto::Array),
        GENERIC_OBJECT_PROTOTYPE => Some(BuiltinProto::Object),
        _ => None,
    };
    let Some(family) = family else {
        if let Some(value) = redirect_call(recv, &name, args, argc) {
            return value;
        }
        return super::js_native_call_method(recv, name.as_ptr().cast(), name.len(), args, argc);
    };
    // The method actually there — still the builtin, too, run as the
    // borrowed call it is (`this` = the receiver).
    match borrowed_holder(family) {
        Some(holder) => select_and_call(holder, recv, &name, args, argc, true, None)
            .unwrap_or(f64::from_bits(crate::value::TAG_UNDEFINED)),
        None => f64::from_bits(crate::value::TAG_UNDEFINED),
    }
}

/// `Object.prototype.<m>.call(x, …)` entry points (#11915). The HIR folds the
/// borrowed call to the builtin's runtime body; these keep that body for a
/// pristine `%Object.prototype%` and `Function.prototype.call`, and otherwise
/// perform the real `Call(Get(%Object.prototype%, m), x, args)`. Same ABI as
/// the bodies (every operand a NaN-boxed double), so the fold costs no
/// emitted code at the call site.
///
/// The `Pristine` check of an `Object.prototype.<m>.call(this, …args)` entry:
/// `Some(result)` when `%Object.prototype%` or `Function.prototype.call` was
/// modified and the method actually there is not the builtin.
#[inline]
fn borrowed_object_redirect(name: &[u8], this: f64, args: &[f64]) -> Option<f64> {
    if pristine_borrowed(BuiltinProto::Object) {
        return None;
    }
    // SAFETY: `args` holds exactly `args.len()` values.
    unsafe { redirect_borrowed_call(BuiltinProto::Object, this, name, args.as_ptr(), args.len()) }
}

#[no_mangle]
pub extern "C-unwind" fn js_borrowed_object_to_string(this: f64) -> f64 {
    if let Some(v) = borrowed_object_redirect(b"toString", this, &[]) {
        return v;
    }
    // SAFETY: a NaN-boxed operand, the body's contract.
    unsafe { super::js_object_to_string(this) }
}

#[no_mangle]
pub extern "C-unwind" fn js_borrowed_object_has_own(this: f64, key: f64) -> f64 {
    if let Some(v) = borrowed_object_redirect(b"hasOwnProperty", this, &[key]) {
        return v;
    }
    super::js_object_has_own(this, key)
}

#[no_mangle]
pub extern "C-unwind" fn js_borrowed_object_property_is_enumerable(this: f64, key: f64) -> f64 {
    if let Some(v) = borrowed_object_redirect(b"propertyIsEnumerable", this, &[key]) {
        return v;
    }
    super::js_object_property_is_enumerable(this, key)
}

#[no_mangle]
pub extern "C-unwind" fn js_borrowed_object_define_getter(this: f64, key: f64, getter: f64) -> f64 {
    if let Some(v) = borrowed_object_redirect(b"__defineGetter__", this, &[key, getter]) {
        return v;
    }
    super::js_object_define_getter(this, key, getter)
}

#[no_mangle]
pub extern "C-unwind" fn js_borrowed_object_define_setter(this: f64, key: f64, setter: f64) -> f64 {
    if let Some(v) = borrowed_object_redirect(b"__defineSetter__", this, &[key, setter]) {
        return v;
    }
    super::js_object_define_setter(this, key, setter)
}

#[no_mangle]
pub extern "C-unwind" fn js_borrowed_object_lookup_getter(this: f64, key: f64) -> f64 {
    if let Some(v) = borrowed_object_redirect(b"__lookupGetter__", this, &[key]) {
        return v;
    }
    super::js_object_lookup_getter(this, key)
}

#[no_mangle]
pub extern "C-unwind" fn js_borrowed_object_lookup_setter(this: f64, key: f64) -> f64 {
    if let Some(v) = borrowed_object_redirect(b"__lookupSetter__", this, &[key]) {
        return v;
    }
    super::js_object_lookup_setter(this, key)
}

/// `arr.pop()` as compiled code calls it (#11915): the builtin body while
/// `%Array.prototype%` is pristine, else the method actually selected. The
/// body itself (`js_array_pop_f64`) stays the runtime's, so a saved original
/// called from a wrapper over it cannot re-enter this lookup.
#[no_mangle]
pub extern "C" fn js_array_pop_f64_site(arr: *mut crate::array::ArrayHeader) -> f64 {
    let mut recv = crate::value::js_nanbox_pointer(arr as i64);
    if !receiver_pristine(recv) {
        // The same receiver proof and generic call as every checked entry;
        // a Proxy is never a plain ArrayHeader, even in a pristine realm.
        if let Some(value) = unsafe { redirect_or_refresh(&mut recv, b"pop", &mut []) } {
            return value;
        }
    }
    crate::array::js_array_pop_f64(
        (recv.to_bits() & crate::value::POINTER_MASK) as *mut crate::array::ArrayHeader,
    )
}

/// A Map/Set method name the by-name natives do not implement (#11915): the
/// spec call, `Call(Get(recv, name), recv, args)` — `m.isPrototypeOf(x)`,
/// `m.toLocaleString()` (whose body invokes `toString`, an ordinary nested
/// call here), `m.foo()` on a relinked `Map.prototype`, or the TypeError for
/// a missing method.
///
/// A method reached here that dispatches by name back into this lookup can
/// form a cycle of runtime frames only, which no compiled prologue's stack
/// check sees; the same check at this entry bounds it, so a runaway
/// recursion through this path throws node's `RangeError` instead of running
/// into the guard page. No state of its own: the agent's stack limit is the
/// one every prologue reads.
#[optimize(size)]
pub(crate) unsafe fn call_unknown_collection_method(recv: f64, name: &[u8], args: &[f64]) -> f64 {
    crate::stack_guard::check_native_stack();
    select_and_call(recv, recv, name, args.as_ptr(), args.len(), true, None)
        .unwrap_or(f64::from_bits(crate::value::TAG_UNDEFINED))
}

/// [`js_builtin_proto_generic_call`] for a receiver read with its arguments
/// in registers: the generic arm of a guarded site then needs no argument
/// buffer, only the call (#11915 inline-size rule).
#[no_mangle]
pub unsafe extern "C-unwind" fn js_builtin_proto_generic_call0(recv: f64, method_id: i64) -> f64 {
    js_builtin_proto_generic_call(GENERIC_RECEIVER, recv, method_id, std::ptr::null(), 0)
}

/// See [`js_builtin_proto_generic_call0`].
#[no_mangle]
pub unsafe extern "C-unwind" fn js_builtin_proto_generic_call1(
    recv: f64,
    method_id: i64,
    a0: f64,
) -> f64 {
    let args = [a0];
    js_builtin_proto_generic_call(GENERIC_RECEIVER, recv, method_id, args.as_ptr(), 1)
}

/// See [`js_builtin_proto_generic_call0`].
#[no_mangle]
pub unsafe extern "C-unwind" fn js_builtin_proto_generic_call2(
    recv: f64,
    method_id: i64,
    a0: f64,
    a1: f64,
) -> f64 {
    let args = [a0, a1];
    js_builtin_proto_generic_call(GENERIC_RECEIVER, recv, method_id, args.as_ptr(), 2)
}

/// See [`js_builtin_proto_generic_call0`].
#[no_mangle]
pub unsafe extern "C-unwind" fn js_builtin_proto_generic_call3(
    recv: f64,
    method_id: i64,
    a0: f64,
    a1: f64,
    a2: f64,
) -> f64 {
    let args = [a0, a1, a2];
    js_builtin_proto_generic_call(GENERIC_RECEIVER, recv, method_id, args.as_ptr(), 3)
}

/// `Get(recv, "size")`: the miss of a Map / Set `size` read whose receiver
/// failed the brand test (codegen `collection_receiver::lower_collection_size`):
/// an own or inherited `size` (a getter runs with `recv` as `this`),
/// `undefined`, or node's TypeError for null / undefined.
#[no_mangle]
pub extern "C-unwind" fn js_collection_size_get(recv: f64) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let recv = scope.root_nanbox_f64(recv);
    // The interned key may allocate on first use; the receiver is rooted.
    let key = crate::string::canonical_key(b"size");
    crate::value::js_dyn_index_get(
        recv.get_nanbox_f64(),
        crate::value::js_nanbox_string(key as i64),
    )
}

/// Contract builds: a site that took the direct builtin path must
/// have held `Pristine(family)`. Codegen emits a call to this only under
/// `PERRY_CHECK_CONTRACTS`; release code never references it. Defined in
/// every build (contract builds link the plain runtime, without
/// `gc-instruments`).
#[no_mangle]
pub extern "C" fn js_builtin_proto_contract_check(family: u64) {
    if let Some(f) = BuiltinProto::from_index(family as usize) {
        if !pristine(f) {
            eprintln!("[contract] Pristine({f:?}) violated: a direct builtin call ran on a modified prototype");
            std::process::abort();
        }
    }
}

#[cfg(test)]
#[path = "builtin_proto_tests.rs"]
mod tests;
