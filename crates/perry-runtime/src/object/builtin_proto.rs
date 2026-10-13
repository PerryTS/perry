//! The "builtin prototype unmodified" proof (#11915): `Pristine(P)` of the
//! typed-entry contracts (decision 129, §4.1 / slice 5).
//!
//! # The fact
//!
//! [`PERRY_BUILTIN_PROTO_PATCHED`] is ONE process-global, immortal byte
//! array, one byte per [`BuiltinProto`] family. Byte `k` becomes nonzero once
//! the realm's intrinsic prototype of family `k` — or `%Object.prototype%`,
//! which every family inherits from — has been written, defined, deleted or
//! relinked after the runtime sealed it at startup. It is set-only: restoring
//! a method never reopens the fact.
//!
//! So `Pristine(k)` is `PERRY_BUILTIN_PROTO_PATCHED[k] == 0`: one byte load
//! and compare (`cmpb $0, sym+k(%rip)`), with no per-site state — the bytes
//! are monotone, so a site has no epoch to remember. A receiver of a builtin
//! kind resolves its builtin methods through its family's prototype and then
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
//! `ObjectMeta::flags`) once the realm is
//! populated. Every mutation funnel then calls [`note_write`] (one meta load,
//! null-checked) or [`note_array_prototype_write`]: the slot and spill stores,
//! the shape publication funnel (key add, delete, descriptor and attribute
//! changes, `[[Prototype]]` relinks), the descriptor, delete and
//! `[[Prototype]]` funnels, and the Array-prototype named-property set and
//! delete. Nothing is paid until a sealed prototype is written.
//!
//! The runtime's own definitions (`own_override::as_builtin_definition`, the
//! lazy populations) do not count: they install the builtin itself.

use std::sync::atomic::{AtomicU8, Ordering};

use super::ObjectHeader;

/// Number of [`BuiltinProto`] families (bytes of the fact).
pub const FAMILIES: usize = 16;

/// The byte after the families: nonzero once ANY family was withdrawn — the
/// runtime dispatcher's one-load early exit.
const ANY_PATCHED: usize = FAMILIES;

per_test_global! {
    // #10944: tests run in one process; a test that patches a prototype must
    // not make a sibling's pristine assertion fail. Outside a test build this
    // is the plain exported static, byte for byte.
    /// Byte `k` nonzero: family `k`'s intrinsic prototype, or
    /// `%Object.prototype%`, was modified after sealing. Read by emitted code
    /// as `@PERRY_BUILTIN_PROTO_PATCHED`.
    #[cfg_attr(not(test), export_name = "PERRY_BUILTIN_PROTO_PATCHED")]
    pub static PERRY_BUILTIN_PROTO_PATCHED: [AtomicU8; FAMILIES + 1] =
        [const { AtomicU8::new(0) }; FAMILIES + 1];
}

/// #11915: the heap-handle bound of the emitted inline `charCodeAt`: 4096
/// (the handle band) while `%String.prototype%` and `%Object.prototype%` are
/// unmodified, `u64::MAX` once either is. The inline heap arm admits a handle
/// only at or above it, so this one value — a mirror of the String byte, set
/// with it — is how that arm proves `Pristine(String)` with the compare it
/// already makes. Set-only.
#[no_mangle]
pub static PERRY_STRING_INLINE_MIN_HANDLE: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(4096);

/// A family of intrinsic prototypes whose methods compiled code calls by
/// name. The discriminant is the family's byte in
/// [`PERRY_BUILTIN_PROTO_PATCHED`]; `perry-codegen` mirrors these values
/// (`builtin_proto.rs`), and a test pins the two lists together.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum BuiltinProto {
    Object = 0,
    Function = 1,
    Array = 2,
    String = 3,
    Number = 4,
    Boolean = 5,
    Symbol = 6,
    BigInt = 7,
    Map = 8,
    Set = 9,
    Date = 10,
    RegExp = 11,
    Promise = 12,
    Error = 13,
    WeakMap = 14,
    WeakSet = 15,
}

impl BuiltinProto {
    /// Every family, with the global constructor whose `prototype` it is.
    pub const ALL: [(BuiltinProto, &'static str); FAMILIES] = [
        (BuiltinProto::Object, "Object"),
        (BuiltinProto::Function, "Function"),
        (BuiltinProto::Array, "Array"),
        (BuiltinProto::String, "String"),
        (BuiltinProto::Number, "Number"),
        (BuiltinProto::Boolean, "Boolean"),
        (BuiltinProto::Symbol, "Symbol"),
        (BuiltinProto::BigInt, "BigInt"),
        (BuiltinProto::Map, "Map"),
        (BuiltinProto::Set, "Set"),
        (BuiltinProto::Date, "Date"),
        (BuiltinProto::RegExp, "RegExp"),
        (BuiltinProto::Promise, "Promise"),
        (BuiltinProto::Error, "Error"),
        (BuiltinProto::WeakMap, "WeakMap"),
        (BuiltinProto::WeakSet, "WeakSet"),
    ];

    fn from_index(i: usize) -> Option<BuiltinProto> {
        Self::ALL.get(i).map(|(f, _)| *f)
    }
}

/// `Pristine(family)`: neither `family`'s prototype nor `%Object.prototype%`
/// has been modified. One relaxed byte load.
#[inline(always)]
pub fn pristine(family: BuiltinProto) -> bool {
    PERRY_BUILTIN_PROTO_PATCHED[family as usize].load(Ordering::Relaxed) == 0
}

/// `Pristine` for a borrowed call (`Array.prototype.m.call(x, …)`), which
/// also reads `Function.prototype.call`.
#[inline(always)]
pub fn pristine_borrowed(family: BuiltinProto) -> bool {
    pristine(family) && pristine(BuiltinProto::Function)
}

/// What a direct-call memo has proven about its receiver — the runtime side
/// of the ONE pristine-receiver guard (`perry-codegen`'s
/// `builtin_proto::PristineReceiverFact` emits the same checks inline).
///
/// * `Intrinsic(family)`: the receiver's KIND is proven; its methods are
///   `family`'s builtins while [`pristine`]`(family)` holds.
/// * `Shape`: a site / holder memo proved, at `PERRY_PROTO_VALIDITY ==
///   proof_epoch`, that a receiver with word `proof_word` (`class_id |
///   ShapeId << 32`, [`receiver_word`]) has no own method, accessor or
///   interceptor for the name and resolves it to the memo's pinned inherited
///   method. The receiver's `[[Prototype]]` is part of its ShapeId; a marked
///   prototype's structural changes and in-place method replacements
///   ([`note_value_store`]) advance the epoch.
///
/// Checking either is loads and compares only: Leaf, NoCollect, NoThrow.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PristineReceiverFact {
    Intrinsic(BuiltinProto),
    Shape {
        receiver_word: u64,
        proof_word: u64,
        proof_epoch: u64,
    },
}

impl PristineReceiverFact {
    /// Does the fact hold now? A hit may take the direct body; a miss must
    /// take the one generic operation.
    #[inline(always)]
    pub fn holds(self) -> bool {
        match self {
            PristineReceiverFact::Intrinsic(family) => pristine(family),
            PristineReceiverFact::Shape {
                receiver_word,
                proof_word,
                proof_epoch,
            } => {
                receiver_word == proof_word
                    && super::proto_validity::proto_validity() == proof_epoch
            }
        }
    }
}

/// The word a [`PristineReceiverFact::Shape`] proof is keyed on: an ordinary
/// object's first header word, `class_id | ShapeId << 32`.
///
/// # Safety
/// `obj` is a live `ObjectHeader`.
#[inline(always)]
pub unsafe fn receiver_word(obj: *const ObjectHeader) -> u64 {
    u64::from((*obj).class_id) | (u64::from((*obj).parent_class_id) << 32)
}

/// The current `PERRY_PROTO_VALIDITY`, for a memo recording its proof.
#[inline(always)]
pub fn proof_epoch() -> u64 {
    super::proto_validity::proto_validity()
}

/// Is every family pristine and no Array re-parented? (The runtime
/// dispatcher's one-load early exit.)
#[inline(always)]
fn all_pristine() -> bool {
    PERRY_BUILTIN_PROTO_PATCHED[ANY_PATCHED].load(Ordering::Relaxed) == 0
}

/// An Array took its own `[[Prototype]]` (`GC_ARRAY_CUSTOM_PROTO`): the
/// dispatcher's early exit can no longer answer without reading the receiver.
/// No family is withdrawn — every other array still reads `%Array.prototype%`.
pub(crate) fn note_array_custom_proto() {
    PERRY_BUILTIN_PROTO_PATCHED[ANY_PATCHED].store(1, Ordering::Relaxed);
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
    if obj.is_null() {
        return;
    }
    let _ = super::proto_validity::mark_object_as_prototype(obj as usize);
    // Marking may have moved nothing (startup runs with collection
    // suppressed by the caller) but it did ensure a meta record.
    let meta = super::object_meta_ensure(obj);
    if meta.is_null() {
        return;
    }
    // GC_STORE_AUDIT(POINTER_FREE): scalar flag bits, never a reference.
    (*meta).flags |= super::meta_flags::OBJECT_META_FLAG_SEALED_INTRINSIC
        | ((family as u64) << super::meta_flags::OBJECT_META_SEALED_FAMILY_SHIFT);
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
                crate::gc::GC_TYPE_OBJECT => seal_object(proto as *mut ObjectHeader, family),
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
                }
                _ => {}
            }
        }
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
        note_family_write(sealed_family(flags), site);
    }
}

/// The family a sealed prototype's meta flags record (see
/// `OBJECT_META_FLAG_SEALED_INTRINSIC`).
#[inline(always)]
fn sealed_family(flags: u64) -> usize {
    ((flags >> super::meta_flags::OBJECT_META_SEALED_FAMILY_SHIFT) & 0xF) as usize
}

/// Where a value store's previous value lives, for [`note_value_store`]'s
/// out-of-line half.
#[derive(Clone, Copy)]
pub(crate) enum StoredSlot {
    /// An inline field slot.
    Inline(*const u64),
    /// A spill slot of `obj` at absolute field index `index`.
    Spill { obj: usize, index: usize },
}

/// The value-store hook (slot and spill stores): [`note_write`] for a sealed
/// builtin prototype, and — for ANY object used as a prototype — the
/// method-identity half of `PERRY_PROTO_VALIDITY`: replacing a function value
/// held by a prototype (or putting one there) in place, on the same shape,
/// advances the epoch, so a memo proven by `(ShapeId, PERRY_PROTO_VALIDITY)`
/// ([`PristineReceiverFact::Shape`]) can never call a replaced method. A data
/// overwrite that involves no function does not move it.
///
/// Inline: one meta-pointer load and one flag test (a sealed builtin
/// prototype is always marked); everything else is out of line.
///
/// # Safety
/// `obj` is a live `ObjectHeader`; `slot` names the slot about to be written,
/// still holding its previous value.
#[inline(always)]
pub(crate) unsafe fn note_value_store(
    obj: *const ObjectHeader,
    slot: StoredSlot,
    new_bits: u64,
    site: &'static str,
) {
    let meta = (*obj).meta;
    if meta.is_null() || (*meta).flags & super::OBJECT_META_FLAG_IS_PROTOTYPE == 0 {
        return;
    }
    note_prototype_value_store(meta, slot, new_bits, site);
}

#[cold]
#[inline(never)]
#[optimize(size)]
unsafe fn note_prototype_value_store(
    meta: *const super::ObjectMeta,
    slot: StoredSlot,
    new_bits: u64,
    site: &'static str,
) {
    let flags = (*meta).flags;
    if flags & super::meta_flags::OBJECT_META_FLAG_SEALED_INTRINSIC != 0 {
        note_family_write(sealed_family(flags), site);
    }
    let old_bits = match slot {
        StoredSlot::Inline(ptr) => Some(*ptr),
        StoredSlot::Spill { obj, index } => super::spill::spill_get(obj, index),
    };
    if old_bits == Some(new_bits) {
        return;
    }
    let callable = |bits: u64| {
        bits & !crate::value::POINTER_MASK == crate::value::POINTER_TAG
            && crate::closure::is_closure_ptr((bits & crate::value::POINTER_MASK) as usize)
    };
    if old_bits.is_some_and(callable) || callable(new_bits) {
        super::proto_validity::bump_proto_validity();
    }
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
    let proto = crate::array::array_prototype_addr_if_resolved();
    if proto == 0 {
        return;
    }
    // A grown array leaves a forwarding alias; compare the live head.
    let live = crate::array::clean_arr_ptr(arr as *const crate::array::ArrayHeader) as usize;
    if proto == arr || proto == live {
        note_family_write(BuiltinProto::Array as usize, site);
    }
}

/// Withdraw `Pristine(family)` — every family's for `%Object.prototype%`,
/// which all of them inherit from. Set-only.
#[cold]
#[inline(never)]
#[optimize(size)]
fn note_family_write(family: usize, site: &'static str) {
    if super::own_override::in_builtin_definition() {
        return;
    }
    let first = if family == BuiltinProto::Object as usize {
        PERRY_BUILTIN_PROTO_PATCHED[..FAMILIES]
            .iter()
            .fold(false, |any, b| b.swap(1, Ordering::Relaxed) == 0 || any)
    } else if family < FAMILIES {
        PERRY_BUILTIN_PROTO_PATCHED[family].swap(1, Ordering::Relaxed) == 0
    } else {
        false
    };
    PERRY_BUILTIN_PROTO_PATCHED[ANY_PATCHED].store(1, Ordering::Relaxed);
    super::own_override::note_builtin_proto_modified();
    if family == BuiltinProto::Array as usize || family == BuiltinProto::Object as usize {
        crate::array::note_array_methods_modified();
    }
    if family == BuiltinProto::String as usize || family == BuiltinProto::Object as usize {
        PERRY_STRING_INLINE_MIN_HANDLE.store(u64::MAX, Ordering::Relaxed);
    }
    // A program that patches nothing must print nothing.
    if first {
        trace_withdrawal(family, site);
    }
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

/// The family whose prototype a by-name builtin dispatch on `recv` answers
/// for, or `None` for a receiver whose methods are always read through its
/// own shape and real `[[Prototype]]` chain (ordinary objects, class
/// instances).
pub(crate) fn receiver_family(recv: f64) -> Option<BuiltinProto> {
    let v = crate::value::JSValue::from_bits(recv.to_bits());
    if v.is_any_string() {
        return Some(BuiltinProto::String);
    }
    if v.is_number() || v.is_int32() {
        return Some(BuiltinProto::Number);
    }
    if v.is_bool() {
        return Some(BuiltinProto::Boolean);
    }
    if v.is_bigint() {
        return Some(BuiltinProto::BigInt);
    }
    if !v.is_pointer() {
        return None;
    }
    let addr = (recv.to_bits() & crate::value::POINTER_MASK) as usize;
    if !crate::value::addr_class::is_above_handle_band(addr) {
        return None;
    }
    let header = unsafe { crate::value::addr_class::try_read_gc_header(addr) }?;
    match header.obj_type {
        crate::gc::GC_TYPE_ARRAY | crate::gc::GC_TYPE_LAZY_ARRAY => Some(BuiltinProto::Array),
        crate::gc::GC_TYPE_MAP => Some(BuiltinProto::Map),
        crate::gc::GC_TYPE_SET => Some(BuiltinProto::Set),
        crate::gc::GC_TYPE_DATE_CELL => Some(BuiltinProto::Date),
        crate::gc::GC_TYPE_CLOSURE => Some(BuiltinProto::Function),
        _ => None,
    }
}

/// `Pristine(family(recv))`, for the runtime's own by-name dispatch. `true`
/// for a receiver that has no by-name builtin family.
///
/// An Array whose own `[[Prototype]]` was retargeted (`GC_ARRAY_CUSTOM_PROTO`)
/// is never pristine: its methods are not `%Array.prototype%`'s.
#[inline]
pub(crate) fn receiver_pristine(recv: f64) -> bool {
    all_pristine() || receiver_pristine_slow(recv)
}

/// [`receiver_pristine`] once anything was modified.
#[cold]
#[inline(never)]
#[optimize(size)]
fn receiver_pristine_slow(recv: f64) -> bool {
    match receiver_family(recv) {
        Some(BuiltinProto::Array) if array_with_custom_proto(recv) => false,
        Some(family) => pristine(family),
        None => true,
    }
}

/// A plain Array receiver whose header records a custom `[[Prototype]]`.
#[inline]
fn array_with_custom_proto(recv: f64) -> bool {
    let bits = recv.to_bits();
    if bits & !crate::value::POINTER_MASK != crate::value::POINTER_TAG {
        return false;
    }
    let addr = (bits & crate::value::POINTER_MASK) as usize;
    if !crate::value::addr_class::is_above_handle_band(addr) {
        return false;
    }
    match unsafe { crate::value::addr_class::try_read_gc_header(addr) } {
        Some(h) => {
            h.obj_type == crate::gc::GC_TYPE_ARRAY
                && h._reserved & crate::gc::GC_ARRAY_CUSTOM_PROTO != 0
        }
        None => false,
    }
}

/// Is `value` one of the runtime's own builtin method closures?
fn is_builtin_method(value: f64) -> bool {
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
    let selected = super::method_site::spec_get_value(holder, name);
    if !call_builtin && is_builtin_method(selected) {
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
    Some(super::method_site::js_method_site_call_value(
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
    if !pristine(BuiltinProto::Array) {
        let live = crate::array::clean_arr_ptr_mut(arr);
        if !live.is_null() {
            let recv = crate::value::js_nanbox_pointer(live as i64);
            // SAFETY: no arguments.
            if let Some(value) = unsafe { redirect_call(recv, b"pop", std::ptr::null(), 0) } {
                return value;
            }
        }
    }
    crate::array::js_array_pop_f64(arr)
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
