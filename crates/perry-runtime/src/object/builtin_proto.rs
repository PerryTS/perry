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

use super::ObjectHeader;

/// Number of [`BuiltinProto`] families.
pub const FAMILIES: usize = crate::codegen_abi::BUILTIN_PROTO_FAMILIES;

pub use perry_abi::BuiltinProto;

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
        super::MetaObj::runtime_born(obj),
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

/// The meta family field of a sealed RUNTIME prototype (one the runtime
/// builds, such as `%URL.prototype%` or a timer prototype; not a
/// [`BuiltinProto`]). A write marks only that prototype.
pub(crate) const RUNTIME_FAMILY: usize = super::builtin_namespace::GLOBAL_FAMILY + 1;
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
        super::MetaObj::runtime_born(proto),
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
        for (family, name) in BuiltinProto::ALL {
            let proto = intrinsic_prototype_addr(family, global, name);
            if proto == 0 {
                continue;
            }
            let Some(header) = crate::value::addr_class::try_read_gc_header(proto) else {
                continue;
            };
            match header.obj_type {
                crate::gc::GC_TYPE_OBJECT => {
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
                crate::gc::GC_TYPE_ARRAY => {
                    let bag = crate::array::array_property_bag_ensure(
                        proto as *mut crate::array::ArrayHeader,
                    );
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
pub(crate) unsafe fn note_write(obj: *const ObjectHeader, site: &'static str) {
    if obj.is_null() {
        return;
    }
    let meta = (*obj).meta;
    if meta.is_null() {
        return;
    }
    let flags = (*meta).flags;
    if flags & super::meta_flags::OBJECT_META_FLAG_SEALED_INTRINSIC != 0 {
        note_sealed_write(obj, flags, None, site);
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
/// global (`builtin_namespace`). `field` is the inline or spill slot a value
/// store is about to write, when known.
#[cold]
#[inline(never)]
#[optimize(size)]
unsafe fn note_sealed_write(
    obj: *const ObjectHeader,
    flags: u64,
    field: Option<usize>,
    site: &'static str,
) {
    let family = sealed_family(flags);
    if family < FAMILIES {
        note_family_write(family, site);
    } else if family == RUNTIME_FAMILY {
        if !super::own_override::in_builtin_definition() {
            super::shadow::set_bit(obj.cast_mut());
        }
    } else {
        super::builtin_namespace::note_write(obj as *mut ObjectHeader, family, field, site);
    }
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
    let meta = (*obj).meta;
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
    let flags = (*meta).flags;
    crate::dyn_eval_hooks::note_proto_write();
    if flags & super::meta_flags::OBJECT_META_FLAG_SEALED_INTRINSIC != 0 {
        note_sealed_write(obj, flags, Some(field_index), site);
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
pub(crate) fn note_write_addr(addr: usize, site: &'static str) {
    if addr == 0 || !crate::value::addr_class::is_above_handle_band(addr) {
        return;
    }
    let Some(header) = (unsafe { crate::value::addr_class::try_read_gc_header(addr) }) else {
        return;
    };
    match header.obj_type {
        crate::gc::GC_TYPE_OBJECT => unsafe { note_write(addr as *const ObjectHeader, site) },
        crate::gc::GC_TYPE_ARRAY => note_array_prototype_write(addr, site),
        crate::gc::GC_TYPE_CLOSURE => unsafe {
            note_write(
                (*(addr as *const crate::closure::ClosureHeader)).props,
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
pub(crate) fn note_array_prototype_write(arr: usize, site: &'static str) {
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
            note_write(crate::array::array_property_bag(live), site);
        }
    }
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
    if v.is_number() || v.is_int32() {
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
        crate::gc::GC_TYPE_ARRAY | crate::gc::GC_TYPE_LAZY_ARRAY
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
        crate::gc::GC_TYPE_ARRAY | crate::gc::GC_TYPE_LAZY_ARRAY => BuiltinProto::Array,
        crate::gc::GC_TYPE_MAP => BuiltinProto::Map,
        crate::gc::GC_TYPE_SET => BuiltinProto::Set,
        crate::gc::GC_TYPE_DATE_CELL => BuiltinProto::Date,
        crate::gc::GC_TYPE_CLOSURE => BuiltinProto::Function,
        _ => return None,
    };
    Some(ReceiverFact::Builtin(
        family,
        family == BuiltinProto::Array && header._reserved & crate::gc::GC_ARRAY_CUSTOM_PROTO != 0,
    ))
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
/// `m.foo()` on a relinked `Map.prototype`, or the TypeError for a missing
/// method — instead of the old silent `undefined`. A builtin closure found
/// here is called as a value; one level only, so a builtin body that
/// dispatches the same name by name again answers `undefined` as before
/// rather than recursing.
#[optimize(size)]
pub(crate) unsafe fn call_unknown_collection_method(recv: f64, name: &[u8], args: &[f64]) -> f64 {
    thread_local! {
        static ACTIVE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    }
    if ACTIVE.with(|a| a.get()) {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    struct Reset;
    impl Drop for Reset {
        fn drop(&mut self) {
            ACTIVE.with(|a| a.set(false));
        }
    }
    ACTIVE.with(|a| a.set(true));
    let _reset = Reset;
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
