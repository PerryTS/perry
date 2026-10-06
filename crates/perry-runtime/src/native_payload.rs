//! Ordinary GC objects that own a native payload (#11919 P0).
//!
//! The shared shape every native-handle family converts to. A JS-visible
//! instance is an ordinary `GC_TYPE_OBJECT`:
//!
//! * its **family class id** (`native_class_ids.rs`) brands it, so
//!   `instanceof`, the worker-transfer guard and every receiver check are one
//!   compare on the object itself;
//! * its **per-realm prototype** carries the methods, so `typeof`, method
//!   reads, `Object.keys`, `JSON.stringify`, `Map`/`WeakMap` keys and identity
//!   (`a !== b`) are those of any object;
//! * its **`ObjectMeta.native_state` word** holds the payload: a
//!   POINTER_TAG-boxed `GC_TYPE_NATIVE_HANDLE` cell (`native_handle.rs`) that
//!   owns a `Box<T>` of the family's Rust state. The meta record's GC arm
//!   traces that word (`gc/layout_slot_visit.rs`), so the cell lives exactly
//!   as long as the object and is finalized by the sweep that finds both dead.
//!
//! Lifetime. The cell's finalizer is the monomorphized drop of `Box<T>`; it
//! runs once per installed payload, at release ([`close`]), sweep or thread
//! teardown. Close leaves a CLOSED cell and its traced owner edge alive;
//! only sweep and teardown finalize it. [`attach`] reopens the same cell.
//! After close the object stays a valid object:
//! [`payload_mut`] answers [`PayloadMiss::Closed`] and the family reports its
//! node-shaped "already finalized" error.
//!
//! Pacing. The payload's native size is reported through
//! `gc_note_external_side_alloc` when the cell is created (and re-stated by
//! [`set_external_bytes`] when a buffer grows or is released), and released
//! by the finalizer, so a program that holds many payloads drives collections
//! by the memory they really retain.
//!
//! What may sit in a payload: plain Rust data only. No JS values, no NaN-boxed
//! bits, no raw GC pointers. The optional cell.owner slot is the only traced
//! back-edge; nothing traces into Box<T>. Stable OwnerLink tokens in callback
//! sites are inert native data. A JS value the family must keep (listeners, a pipe destination, an
//! options object) lives on the object: in an ordinary own property when node
//! shows it, otherwise in the hidden per-object state object ([`js_state`]),
//! which the collector traces and moves like any field. `Drop for T` must not
//! allocate on the GC heap, call JS or touch thread-locals: it runs inside a
//! collection or during thread teardown.
//!
//! Thread affinity. A payload is bound to the thread that created it (the
//! cell records its creator; objects never cross threads — the family's class
//! id is inside `is_native_backed_class_id`, so `postMessage` refuses it).
//!
//! The per-family conversion checklist is `docs/native-payload-pattern.md`.

use std::ffi::c_void;
use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};

use crate::native_handle::NativeHandleHeader;
use crate::object::ObjectHeader;

#[path = "native_payload_birth.rs"]
mod birth;
pub use birth::{BirthMemo, OwnAccessor};
#[path = "native_payload_slots.rs"]
mod callback_slots;
pub use callback_slots::{call_callback, callback_at, callback_from_link};
pub(crate) use callback_slots::{callback_slot_address, NativeCallbackCell};
use callback_slots::{store_callbacks, sync_callbacks};

/// A family: one class id, one payload type, one prototype per realm.
pub struct NativePayloadFamily {
    /// The family's id from `native_class_ids.rs`.
    pub class_id: u32,
    /// Trace a back-edge from the stable cell for registered callbacks/events.
    pub links_owner: bool,
    /// The constructor name node reports (`h.constructor.name`).
    pub name: &'static str,
    /// The module export that IS this family's constructor in node
    /// (`crypto.Cipheriv`): the prototype's `constructor` is that value. When
    /// `None` (node's `crypto.Hash` is a deprecation wrapper, not the
    /// constructor), `constructor` is a non-constructable builtin named
    /// [`Self::name`] with length [`Self::constructor_length`].
    pub constructor_export: Option<(&'static str, &'static str)>,
    /// `x.constructor.length` when `constructor_export` is `None`.
    pub constructor_length: u32,
    /// Installs the prototype's methods. Called once per realm.
    pub install_prototype: fn(&mut PayloadPrototype),
}

/// The prototype under construction, handed to
/// [`NativePayloadFamily::install_prototype`].
pub struct PayloadPrototype {
    proto: *mut ObjectHeader,
}

impl PayloadPrototype {
    /// LazyTransform's inherited state getters (first state read initializes).
    pub fn lazy_stream_state_getters(&mut self) {
        crate::node_stream::native_hooks::install_lazy_state_getters(self.proto);
    }
    /// Install a builtin method: writable, non-enumerable, configurable, with
    /// the given `.name` and `.length`. `info` comes from `perry_runtime::fn_info!`
    /// with `with_declared(arity)`; the body receives `this` and `arity`
    /// arguments (missing ones arrive as `undefined`).
    pub fn method(&mut self, name: &str, info: *const crate::closure::JsFunctionInfo, arity: u32) {
        crate::object::install_proto_method(self.proto, name, info, arity);
    }

    /// Install a non-enumerable, configurable builtin getter.
    pub fn getter(&mut self, name: &str, info: *const crate::closure::JsFunctionInfo) {
        let closure = crate::closure::js_closure_alloc(info, 0);
        crate::object::native_module::set_bound_native_closure_name(
            closure,
            &format!("get {name}"),
        );
        crate::object::native_module::set_builtin_closure_length(closure as usize, 0);
        crate::object::native_module::set_builtin_closure_non_constructable(closure as usize);
        unsafe {
            crate::object::install_builtin_getter(
                self.proto,
                name,
                crate::value::js_nanbox_pointer(closure as i64).to_bits(),
            );
        }
    }

    /// Install an ordinary data property on the prototype.
    pub fn data(
        &mut self,
        name: &str,
        value: f64,
        writable: bool,
        enumerable: bool,
        configurable: bool,
    ) {
        let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
        crate::object::define_builtin_data_property(
            self.proto,
            key,
            value,
            name.to_string(),
            crate::object::PropertyAttrs::new(writable, enumerable, configurable),
        );
    }

    /// Install a builtin method under a well-known symbol (`Symbol.dispose`):
    /// `well_known` is the symbol's short name, `name` the function's `.name`.
    pub fn symbol_method(
        &mut self,
        well_known: &str,
        name: &str,
        info: *const crate::closure::JsFunctionInfo,
        arity: u32,
    ) {
        let symbol = crate::symbol::well_known_symbol(well_known);
        if symbol.is_null() {
            return;
        }
        let closure = crate::closure::js_closure_alloc(info, 0);
        if closure.is_null() {
            return;
        }
        crate::object::native_module::set_bound_native_closure_name(closure, name);
        crate::object::native_module::set_builtin_closure_length(closure as usize, arity);
        crate::object::native_module::set_builtin_closure_non_constructable(closure as usize);
        unsafe {
            crate::symbol::js_object_set_symbol_property(
                crate::value::js_nanbox_pointer(self.proto as i64),
                f64::from_bits(crate::value::JSValue::pointer(symbol as *const u8).bits()),
                crate::value::js_nanbox_pointer(closure as i64),
            );
        }
    }

    /// Set the prototype's own `[[Prototype]]`.
    pub fn inherit(&mut self, parent: f64) {
        if crate::value::JSValue::from_bits(parent.to_bits()).is_pointer() {
            crate::object::prototype_chain::object_link_class_default_prototype(
                self.proto as usize,
                parent.to_bits(),
            );
        }
    }
}

/// Install a family's method set on a prototype materialized by an older
/// native constructor export. Kept for the two legacy async class ids whose
/// subclass machinery already shares that exact prototype object.
pub fn install_on_prototype(proto: *mut ObjectHeader, install: fn(&mut PayloadPrototype)) {
    if !proto.is_null() {
        // The installer allocates one closure and one string per property.
        // Its caller hands us a raw prototype pointer, so keep the heap fixed
        // until every method has been attached.
        let _no_move = crate::gc::GcSuppressScope::new();
        install(&mut PayloadPrototype { proto });
    }
}

/// Adopt a constructor's existing canonical prototype into the family's
/// per-realm cache. Its methods are installed only when that prototype is
/// first adopted, so user changes to the prototype remain ordinary changes.
pub fn adopt_prototype(
    family: &NativePayloadFamily,
    proto: *mut ObjectHeader,
) -> *mut ObjectHeader {
    adopt_prototype_with(family.class_id, proto, |proto| {
        install_on_prototype(proto, family.install_prototype)
    })
}

pub(crate) fn adopt_prototype_with(
    class_id: u32,
    proto: *mut ObjectHeader,
    install: impl FnOnce(*mut ObjectHeader),
) -> *mut ObjectHeader {
    let index = slot_index(class_id);
    let existing = PAYLOAD_PROTOTYPES.with(|slots| slots[index].load(Ordering::Acquire));
    if existing != 0 {
        return existing as *mut ObjectHeader;
    }
    if !proto.is_null() {
        let _no_move = crate::gc::GcSuppressScope::new();
        install(proto);
        PAYLOAD_PROTOTYPES.with(|slots| {
            crate::gc::runtime_store_root_atomic_raw_i64(
                &slots[index],
                proto as i64,
                Ordering::Release,
            );
        });
    }
    proto
}

/// Why [`payload_mut`] has no payload for a value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PayloadMiss {
    /// Not an instance of this family.
    Foreign,
    /// An instance whose payload was already closed or finalized.
    Closed,
}

/// Prototype slots, one per id in the web-builtin block, per realm. Indexed by
/// `class_id - WEB_BUILTIN_BLOCK_START`; a slot is a GC root (scanned by
/// [`scan_payload_prototype_roots_mut`]) because every instance's
/// `[[Prototype]]` points there.
const PROTOTYPE_SLOTS: usize = 64;

crate::perry_thread_local! {
    static PAYLOAD_PROTOTYPES: [AtomicI64; PROTOTYPE_SLOTS] =
        const { [const { AtomicI64::new(0) }; PROTOTYPE_SLOTS] };
}

const _: () = assert!(
    crate::native_class_ids::ZLIB_BASE - crate::native_class_ids::WEB_BUILTIN_BLOCK_START
        < PROTOTYPE_SLOTS as u32,
    "a native-payload family id outgrew the prototype slot array"
);

#[inline]
fn slot_index(class_id: u32) -> usize {
    if class_id == crate::native_class_ids::ASYNC_LOCAL_STORAGE_LEGACY {
        return PROTOTYPE_SLOTS - 2;
    }
    if class_id == crate::native_class_ids::ASYNC_RESOURCE_LEGACY {
        return PROTOTYPE_SLOTS - 1;
    }
    let index = class_id.wrapping_sub(crate::native_class_ids::WEB_BUILTIN_BLOCK_START) as usize;
    assert!(
        index < PROTOTYPE_SLOTS,
        "class id {class_id:#x} is not in the web-builtin block"
    );
    index
}

#[inline]
fn slot_index_if_payload(class_id: u32) -> Option<usize> {
    if matches!(
        class_id,
        crate::native_class_ids::ASYNC_LOCAL_STORAGE_LEGACY
            | crate::native_class_ids::ASYNC_RESOURCE_LEGACY
    ) {
        return Some(slot_index(class_id));
    }
    let index = class_id.wrapping_sub(crate::native_class_ids::WEB_BUILTIN_BLOCK_START) as usize;
    (index < PROTOTYPE_SLOTS).then_some(index)
}

/// An existing family prototype, without creating a second prototype object.
pub(crate) fn materialized_prototype(class_id: u32) -> Option<*mut ObjectHeader> {
    let index = slot_index_if_payload(class_id)?;
    let ptr = PAYLOAD_PROTOTYPES.with(|slots| slots[index].load(Ordering::Acquire));
    (ptr != 0).then_some(ptr as *mut ObjectHeader)
}

/// GC roots for the payload prototypes. Called from
/// `object::scan_object_cache_roots_mut`, beside the timer prototypes.
pub(crate) fn scan_payload_prototype_roots_mut(visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    PAYLOAD_PROTOTYPES.with(|slots| {
        for slot in slots.iter() {
            visitor.visit_atomic_i64_slot(slot, Ordering::Acquire, Ordering::Release);
        }
    });
}

fn family_prototype(family: &NativePayloadFamily) -> *mut ObjectHeader {
    let index = slot_index(family.class_id);
    let existing = PAYLOAD_PROTOTYPES.with(|slots| slots[index].load(Ordering::Acquire));
    if existing != 0 {
        return existing as *mut ObjectHeader;
    }
    // Raw locals stay stable across the allocating installs below, exactly as
    // the timer and iterator prototypes do (#7251).
    let _no_move = crate::gc::GcSuppressScope::new();
    // Resolve the constructor first. Creating a module export runs that
    // module's attach hook, which may materialize this very prototype (so the
    // export carries `prototype` before any instance exists); adopt the one
    // it published instead of building a second.
    let constructor = match family.constructor_export {
        Some((module, export)) => crate::object::bound_native_callable_export_value(module, export),
        None => {
            let closure = crate::closure::js_closure_alloc(
                crate::fn_info!(payload_ctor_thunk, 0; with_declared(0)),
                0,
            );
            if closure.is_null() {
                f64::from_bits(crate::value::TAG_UNDEFINED)
            } else {
                crate::object::native_module::set_bound_native_closure_name(closure, family.name);
                crate::object::native_module::set_builtin_closure_length(
                    closure as usize,
                    family.constructor_length,
                );
                crate::value::js_nanbox_pointer(closure as i64)
            }
        }
    };
    let existing = PAYLOAD_PROTOTYPES.with(|slots| slots[index].load(Ordering::Acquire));
    if existing != 0 {
        return existing as *mut ObjectHeader;
    }
    let proto = crate::object::js_object_alloc(0, 0);
    if proto.is_null() {
        return proto;
    }
    let mut builder = PayloadPrototype { proto };
    (family.install_prototype)(&mut builder);
    if crate::value::JSValue::from_bits(constructor.to_bits()).is_pointer() {
        let constructor_ptr = crate::value::js_nanbox_get_pointer(constructor) as usize;
        crate::closure::closure_set_dynamic_prop(
            constructor_ptr,
            "prototype",
            crate::value::js_nanbox_pointer(proto as i64),
        );
        crate::object::set_builtin_property_attrs(
            constructor_ptr,
            "prototype".to_string(),
            crate::object::PropertyAttrs::new(false, false, false),
        );
        let key = crate::string::js_string_from_bytes(b"constructor".as_ptr(), 11);
        // Spec shape for `constructor`: writable, NOT enumerable, configurable.
        crate::object::define_builtin_data_property(
            proto,
            key,
            constructor,
            "constructor".to_string(),
            crate::object::PropertyAttrs::new(true, false, true),
        );
    }
    PAYLOAD_PROTOTYPES.with(|slots| {
        crate::gc::runtime_store_root_atomic_raw_i64(
            &slots[index],
            proto as i64,
            Ordering::Release,
        );
    });
    proto
}

/// The stand-in `constructor` of a family whose node constructor is internal.
extern "C" fn payload_ctor_thunk(
    _c: *const crate::closure::ClosureHeader,
    _this: crate::closure::JsThis,
) -> f64 {
    let message = b"Illegal constructor";
    let msg = crate::string::js_string_from_bytes(message.as_ptr(), message.len() as u32);
    let err = crate::error::js_typeerror_new(msg);
    crate::exception::js_throw(crate::value::js_nanbox_pointer(err as i64))
}

/// The cell's type tag: the family id plus the payload's layout. One family
/// has exactly one payload type, so the id is the real discriminator; the
/// layout half catches a family that reads its payload as the wrong type.
#[inline(always)]
const fn type_tag<T>(class_id: u32) -> u64 {
    (class_id as u64)
        | ((std::mem::size_of::<T>() as u64 & 0xFF_FFFF) << 32)
        | ((std::mem::align_of::<T>() as u64 & 0xFF) << 56)
}

unsafe extern "C" fn drop_payload<T>(resource: *mut c_void, _hint: *mut c_void) {
    drop(Box::from_raw(resource as *mut T));
}

/// What the cell's `finalizer` word names for a Rust payload: the payload
/// type's drop, and (for a stream family) the [`StreamHooks`] through which
/// the runtime's one stream state machine reaches the codec. One static per
/// payload type, so the word costs no cell growth and the runtime finds the
/// hooks from the object it already holds (`meta.native_state` -> cell ->
/// vtable), with no name, table or class switch.
///
/// `#[repr(C)]`: a binding crate's family names a vtable of the same layout
/// (perry-ffi `native_stream`).
#[repr(C)]
pub struct PayloadVTable {
    /// Frees one boxed payload (release, sweep or thread teardown).
    pub drop: unsafe extern "C" fn(resource: *mut c_void, hint: *mut c_void),
    /// The stream hooks of a stream family (`None` for every other family).
    pub stream: Option<&'static StreamHooks>,
}

pub use crate::node_stream::native_hooks::{
    StepIn, StepOut, StepStatus, StepTiming, StreamHooks, StreamKind, StreamOp,
};

/// A payload type that is a stream codec: its family's objects are ordinary
/// runtime Transforms/Writables whose writes the runtime feeds to `HOOKS.step`
/// (`node_stream::native_hooks`).
pub trait StreamPayload: 'static {
    /// The family's hooks. A `static`, so the vtable naming it is one too.
    const HOOKS: &'static StreamHooks;
}

/// The static vtables of a payload type `T`.
struct VTableOf<T>(std::marker::PhantomData<T>);

impl<T: 'static> VTableOf<T> {
    const PLAIN: PayloadVTable = PayloadVTable {
        drop: drop_payload::<T>,
        stream: None,
    };
}

impl<T: StreamPayload> VTableOf<T> {
    const HOOKED: PayloadVTable = PayloadVTable {
        drop: drop_payload::<T>,
        stream: Some(T::HOOKS),
    };
}

#[inline(always)]
fn plain_vtable<T: 'static>() -> &'static PayloadVTable {
    &VTableOf::<T>::PLAIN
}

#[inline(always)]
fn hooked_vtable<T: StreamPayload>() -> &'static PayloadVTable {
    &VTableOf::<T>::HOOKED
}

/// Is `word` (an `ObjectMeta.native_state`) a payload cell reference? The
/// one predicate the meta record's GC arm uses to decide whether the word is
/// an edge. Native payloads and weak collections use this same GC-edge encoding.
#[inline(always)]
pub(crate) fn is_payload_state_word(word: u64) -> bool {
    word & crate::value::TAG_MASK == crate::value::POINTER_TAG
        && word & crate::value::POINTER_MASK != 0
}

/// Allocate an instance of `family` owning `payload`.
///
/// `external_bytes` is the native memory the payload really retains (heap
/// buffers it owns, not `size_of::<T>()` alone unless that is all it holds);
/// it feeds GC pacing until the payload is released. `own` lists node's own
/// enumerable data properties in node's order (`[("_options", undefined)]`
/// for a `Hash`); values may be heap values, they are rooted here.
pub fn alloc<T: 'static>(
    family: &'static NativePayloadFamily,
    payload: T,
    external_bytes: usize,
    own: &[(&[u8], f64)],
) -> f64 {
    let proto = family_prototype(family);
    alloc_cell(
        family,
        Some(payload),
        plain_vtable::<T>(),
        external_bytes,
        own,
        own.len(),
        proto,
    )
}

/// [`alloc`] for a stream family: the cell's vtable carries `T::HOOKS`. The
/// caller then runs `node_stream::init_transform_in_place` (or the Writable
/// twin) on the object, in node's constructor order relative to its own
/// fields, so the result is an ordinary runtime stream that owns the codec.
pub fn alloc_stream<T: StreamPayload>(
    family: &'static NativePayloadFamily,
    payload: T,
    external_bytes: usize,
    own: &[(&[u8], f64)],
) -> f64 {
    let proto = family_prototype(family);
    alloc_cell(
        family,
        Some(payload),
        hooked_vtable::<T>(),
        external_bytes,
        own,
        own.len(),
        proto,
    )
}

/// The family's per-realm prototype as a value (built on first use).
pub fn prototype_value(family: &'static NativePayloadFamily) -> f64 {
    let proto = family_prototype(family);
    if proto.is_null() {
        return undefined();
    }
    crate::value::js_nanbox_pointer(proto as i64)
}

/// [`alloc`] with the instance's JS state built at birth: `state` lists its
/// fields in order, and `accessors` the own accessors the instance is born
/// with (after its own data keys). The state object has a null prototype, so
/// storing its fields can run no inherited setter. Every birth at one call
/// site has the same keys, so the site's `memo` records the final shapes its
/// first birth reached and later births are allocated directly in them
/// (`native_payload_birth.rs`). Declare one memo per call site with
/// [`birth_memo!`](crate::birth_memo).
pub fn alloc_with_state<T: 'static>(
    family: &'static NativePayloadFamily,
    payload: T,
    external_bytes: usize,
    own: &[(&[u8], f64)],
    state: &[(&[u8], f64)],
    accessors: &[OwnAccessor],
    memo: &'static BirthMemo,
) -> f64 {
    birth::alloc_born(family, payload, external_bytes, own, state, accessors, memo)
}

/// Allocate an ordinary instance with its permanent cell but no resource.
/// The first attach establishes the family's payload layout and drop thunk.
pub fn alloc_closed(family: &'static NativePayloadFamily, own: &[(&[u8], f64)]) -> f64 {
    let proto = family_prototype(family);
    alloc_cell::<()>(family, None, plain_vtable::<()>(), 0, own, own.len(), proto)
}

/// Allocate a payload-family instance linked to an already-materialized
/// constructor prototype, including AsyncLocalStorage and AsyncResource.
pub fn alloc_with_prototype<T: 'static>(
    family: &'static NativePayloadFamily,
    payload: T,
    external_bytes: usize,
    own: &[(&[u8], f64)],
    proto: *mut ObjectHeader,
) -> f64 {
    alloc_cell(
        family,
        Some(payload),
        plain_vtable::<T>(),
        external_bytes,
        own,
        own.len(),
        proto,
    )
}

/// `slots`: the instance's live inline slots, at least `own.len()` (a birth
/// that adds keys after `own` sizes for them, so none spills).
fn alloc_cell<T: 'static>(
    family: &'static NativePayloadFamily,
    payload: Option<T>,
    vtable: &'static PayloadVTable,
    external_bytes: usize,
    own: &[(&[u8], f64)],
    slots: usize,
    proto: *mut ObjectHeader,
) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let own_roots: Vec<_> = own
        .iter()
        .map(|&(key, value)| (key, scope.root_nanbox_f64(value)))
        .collect();
    if proto.is_null() {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    let obj = unsafe { born_instance(family.class_id, proto, slots as u32) };
    if obj.is_null() {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    let obj = scope.root_raw_mut_ptr(obj);
    for (key, value) in &own_roots {
        set_own(&scope, &obj, key, value.get_nanbox_f64());
    }
    attach_rooted(&obj, family, payload, vtable, external_bytes);
    obj.with_mut_ptr::<ObjectHeader, _>(|obj| crate::value::js_nanbox_pointer(obj as i64))
}

#[path = "native_payload_attach.rs"]
mod attach;
use attach::attach_rooted;
pub(crate) use attach::{any_object, attach_external_rooted, set_own};
pub use attach::{
    attach_stream_to_object, attach_to_object, define_own_accessor, iter_result_done_value,
    iterator_prototype, prototype, state_get, state_get_memo, state_set, state_set_memo,
};

/// A fresh instance of `class_id` linked to its family prototype `proto`,
/// with `slots` live inline slots.
///
/// The first instance takes the ordinary sequence (allocate, link the
/// class-default prototype) and records the keyless birth ShapeId that link
/// left on the prototype's meta (`instance_birth`, #10507's birth record).
/// Every later instance is born directly on that ShapeId, which names the
/// prototype, so no per-instance link (a shape transition and a descriptor
/// intern) runs. The record is re-validated on every replay
/// (`shape_is_keyless_birth`), so a retired ShapeId is never replayed.
///
/// # Safety
/// `proto` is the family's live prototype; it is rooted by its slot.
pub(crate) unsafe fn born_instance(
    class_id: u32,
    proto: *mut ObjectHeader,
    slots: u32,
) -> *mut ObjectHeader {
    let meta = (*proto).meta;
    if !meta.is_null() {
        let word = (*meta).instance_birth;
        let shape_id = (word >> 32) as u32;
        if word != 0
            && word as u32 == class_id
            && crate::object::shapes::shape_is_keyless_birth(shape_id, (*meta).proto_serial, slots)
        {
            return crate::object::object_alloc_born(class_id, slots, shape_id);
        }
    }
    mint_birth_record(class_id, proto, slots)
}

#[cold]
#[inline(never)]
unsafe fn mint_birth_record(
    class_id: u32,
    proto: *mut ObjectHeader,
    slots: u32,
) -> *mut ObjectHeader {
    let scope = crate::gc::RuntimeHandleScope::new();
    let proto = scope.root_raw_mut_ptr(proto);
    let obj = crate::object::js_object_alloc(class_id, slots);
    if obj.is_null() {
        return obj;
    }
    let obj = scope.root_raw_mut_ptr(obj);
    let proto_bits = proto
        .with_mut_ptr::<ObjectHeader, _>(|proto| crate::value::js_nanbox_pointer(proto as i64))
        .to_bits();
    obj.with_mut_ptr::<ObjectHeader, _>(|obj| {
        crate::object::prototype_chain::object_link_class_default_prototype(
            obj as usize,
            proto_bits,
        )
    });
    let shape_id =
        obj.with_mut_ptr::<ObjectHeader, _>(|obj| crate::object::shapes::object_shape_stamp(obj));
    let meta =
        proto.with_mut_ptr::<ObjectHeader, _>(|proto| crate::object::object_meta_ensure(proto));
    if !meta.is_null() {
        // GC_STORE_AUDIT(POINTER_FREE): a class id and a ShapeId, never a heap
        // reference.
        (*meta).instance_birth = u64::from(class_id) | u64::from(shape_id) << 32;
    }
    obj.with_mut_ptr::<ObjectHeader, _>(|obj| obj)
}

/// The object behind `value` when it is an instance of `class_id`.
#[inline]
fn instance_of(value: f64, class_id: u32) -> Option<*mut ObjectHeader> {
    let obj = any_object(value)?;
    (unsafe { (*obj).class_id } == class_id).then_some(obj)
}

#[inline]
fn payload_cell(value: f64, class_id: u32) -> Result<*mut NativeHandleHeader, PayloadMiss> {
    let obj = instance_of(value, class_id).ok_or(PayloadMiss::Foreign)?;
    let meta = unsafe { (*obj).meta };
    if meta.is_null() {
        return Err(PayloadMiss::Foreign);
    }
    let word = unsafe { (*meta).native_state };
    if !is_payload_state_word(word) {
        return Err(PayloadMiss::Foreign);
    }
    Ok((word & crate::value::POINTER_MASK) as *mut NativeHandleHeader)
}

/// The stream hooks of `value`'s attached payload, and its cell, when its
/// family is a stream family: `meta.native_state` -> cell -> the vtable the
/// cell's `finalizer` word names. Three loads from the object the caller
/// already holds; no name, table or class switch, and a subclass instance
/// that attached the payload in `super()` answers the same way.
#[inline]
pub(crate) fn stream_hooks_of(
    value: f64,
) -> Option<(&'static StreamHooks, *mut NativeHandleHeader)> {
    let obj = any_object(value)?;
    // SAFETY: a live ordinary object (checked by `any_object`).
    let meta = unsafe { (*obj).meta };
    if meta.is_null() {
        return None;
    }
    // SAFETY: the meta record of a live object.
    let word = unsafe { (*meta).native_state };
    if !is_payload_state_word(word) {
        return None;
    }
    let cell = (word & crate::value::POINTER_MASK) as *mut NativeHandleHeader;
    // SAFETY: a payload state word names the object's live payload cell.
    let vtable = unsafe { crate::native_handle::cell_vtable(cell)? };
    Some((vtable.stream?, cell))
}

/// Is `value` an instance of `family` (live or closed)?
#[inline]
pub fn is_instance(value: f64, family: &NativePayloadFamily) -> bool {
    payload_cell(value, family.class_id).is_ok()
}

/// The live payload of `value`.
///
/// # Safety
/// The reference is valid until the payload is closed or finalized. The
/// caller must not hold it across anything that can finalize it: a [`close`]
/// of the same object, or a GC allocation / JS call while `value` is not
/// rooted (a receiver held only in a register is found by the conservative
/// stack scan, but a family that allocates or calls JS while holding the
/// reference roots the receiver in a `RuntimeHandleScope` first). It must not
/// be held across a call that could re-enter this family's methods on the
/// same object (no two live `&mut T` to one payload).
#[inline]
pub unsafe fn payload_mut<'a, T: 'static>(
    value: f64,
    family: &NativePayloadFamily,
) -> Result<&'a mut T, PayloadMiss> {
    let cell = payload_cell(value, family.class_id)?;
    let resource =
        crate::native_handle::native_handle_rust_payload_ptr(cell, type_tag::<T>(family.class_id));
    if resource.is_null() {
        return Err(PayloadMiss::Closed);
    }
    Ok(&mut *(resource as *mut T))
}

/// The live payload attached to any ordinary object, including a subclass
/// instance whose own class id differs from the native base's id.
pub unsafe fn payload_mut_attached<'a, T: 'static>(
    value: f64,
    family: &NativePayloadFamily,
) -> Result<&'a mut T, PayloadMiss> {
    let obj = any_object(value).ok_or(PayloadMiss::Foreign)?;
    let meta = (*obj).meta;
    if meta.is_null() || !is_payload_state_word((*meta).native_state) {
        return Err(PayloadMiss::Foreign);
    }
    let cell = ((*meta).native_state & crate::value::POINTER_MASK) as *mut NativeHandleHeader;
    if (*cell).type_id != type_tag::<T>(family.class_id) {
        return Err(PayloadMiss::Foreign);
    }
    let resource =
        crate::native_handle::native_handle_rust_payload_ptr(cell, type_tag::<T>(family.class_id));
    if resource.is_null() {
        return Err(PayloadMiss::Closed);
    }
    Ok(&mut *(resource as *mut T))
}

/// Result of an explicit close; the JS object remains a valid instance.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CloseOutcome {
    Closed,
    Deferred,
    AlreadyClosed,
    Foreign,
}

pub(crate) const PENDING: u8 = 1;
pub(crate) const CLOSING: u8 = 2;
/// The cell's `finalizer` word names a static [`PayloadVTable`] (every Rust
/// payload cell), not a bare C finalizer (a plain native handle).
pub(crate) const VTABLE_WORD: u8 = 4;
/// The owner's JS state already holds the `pendingException` slot.
pub(crate) const PENDING_SLOT: u8 = 8;
pub(crate) const CALLBACK_STORAGE: u8 = 16;

/// The non-finalized cell states. Finalized cells answer PayloadMiss::Closed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lifecycle {
    Open,
    Closing,
    Closed,
}

pub fn lifecycle(value: f64, family: &NativePayloadFamily) -> Result<Lifecycle, PayloadMiss> {
    let cell = payload_cell(value, family.class_id)?;
    unsafe {
        if (*cell).finalized != 0
            || (*cell).creator_thread_id != crate::native_handle::current_thread_id()
        {
            return Err(PayloadMiss::Closed);
        }
        Ok(if (*cell).flags & CLOSING != 0 {
            Lifecycle::Closing
        } else if (*cell).resource_ptr.is_null() {
            Lifecycle::Closed
        } else {
            Lifecycle::Open
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AttachMiss {
    Foreign,
    Open,
    Closing,
    Finalized,
}

/// CLOSED -> OPEN in the same cell. No JS or GC allocation before install.
/// On rejection payload is dropped. Check lifecycle before opening a C resource.
/// One payload type per family, including across reopen.
pub fn attach<T: 'static>(
    value: f64,
    family: &'static NativePayloadFamily,
    payload: T,
    external_bytes: usize,
) -> Result<(), AttachMiss> {
    let cell = payload_cell(value, family.class_id).map_err(|_| AttachMiss::Foreign)?;
    attach_cell(
        value,
        cell,
        family,
        payload,
        plain_vtable::<T>(),
        external_bytes,
    )
}

fn attach_cell<T: 'static>(
    value: f64,
    cell: *mut NativeHandleHeader,
    family: &'static NativePayloadFamily,
    payload: T,
    vtable: &'static PayloadVTable,
    external_bytes: usize,
) -> Result<(), AttachMiss> {
    unsafe {
        if (*cell).magic != crate::native_handle::NATIVE_HANDLE_MAGIC
            || (*cell).creator_thread_id != crate::native_handle::current_thread_id()
        {
            return Err(AttachMiss::Foreign);
        }
        if (*cell).finalized != 0 && !lifecycle_sabotage("attach_finalized") {
            return Err(AttachMiss::Finalized);
        }
        if (*cell).flags & CLOSING != 0 || (*cell).busy != 0 {
            return Err(AttachMiss::Closing);
        }
        if !(*cell).resource_ptr.is_null() {
            return Err(AttachMiss::Open);
        }
        let tag = type_tag::<T>(family.class_id);
        if (*cell).type_id != family.class_id as u64
            && ((*cell).type_id != tag
                || crate::native_handle::cell_drop_fn(cell).map(|f| f as usize)
                    != Some(vtable.drop as usize))
        {
            return Err(AttachMiss::Foreign);
        }
        // alloc_closed cannot know T. Establish the layout/vtable on first
        // attach; subsequent opens keep both unchanged.
        if (*cell).type_id == family.class_id as u64 {
            (*cell).type_id = tag;
            (*cell).finalizer = vtable as *const PayloadVTable as *mut c_void;
            (*cell).flags |= VTABLE_WORD;
        }
        let resource = Box::into_raw(Box::new(payload)) as *mut c_void;
        crate::native_handle::native_handle_attach_rust_payload(cell, resource);
        // Root the owner before reporting bytes: pacing can collect. The
        // payload is already installed and reachable through the owner.
        let scope = crate::gc::RuntimeHandleScope::new();
        let _owner = scope.root_nanbox_f64(value);
        crate::native_handle::native_handle_set_external_bytes(cell, external_bytes);
    }
    Ok(())
}

/// A resource incarnation. Store it in reopenable payloads, children and data
/// completions, and compare once before using the current resource.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OpenSerial(u64);
static NEXT_OPEN_SERIAL: AtomicU64 = AtomicU64::new(1);

pub fn next_open_serial() -> OpenSerial {
    let serial = NEXT_OPEN_SERIAL.fetch_add(1, Ordering::Relaxed);
    assert_ne!(serial, 0, "native open serial exhausted");
    OpenSerial(serial)
}

/// Close immediately, or defer native destruction until the outer C call ends.
pub fn close(value: f64, family: &NativePayloadFamily) -> CloseOutcome {
    let Ok(cell) = payload_cell(value, family.class_id) else {
        return CloseOutcome::Foreign;
    };
    unsafe {
        if (*cell).finalized != 0 || (*cell).flags & CLOSING != 0 || (*cell).resource_ptr.is_null()
        {
            return CloseOutcome::AlreadyClosed;
        }
        // Preserve ordinary receiver thread validation (trampolines use link_owner).
        if crate::native_handle::rust_payload_ptr_on_owner_thread(cell).is_null() {
            return CloseOutcome::Foreign;
        }
        #[cfg(test)]
        if callback_sabotage("close") {
            crate::native_handle::native_handle_release_rust_payload(cell);
            return CloseOutcome::Closed;
        }
        if (*cell).busy != 0 {
            (*cell).flags |= CLOSING;
            return CloseOutcome::Deferred;
        }
        crate::native_handle::native_handle_release_rust_payload(cell);
        #[cfg(test)]
        if lifecycle_sabotage("close_pin") {
            link_ref(OwnerLink(cell as usize));
        }
        CloseOutcome::Closed
    }
}

/// Stable, inert token. Send it to the owner thread; never dereference it on a
/// native worker. Valid only while the C resource/sites or a link_ref keep the
/// cell alive. Explicit close does not revoke outstanding refs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(transparent)]
pub struct OwnerLink(pub(crate) usize);

/// Obtain a link in any non-finalized state with the traced owner edge enabled.
pub fn owner_link(value: f64, family: &NativePayloadFamily) -> Result<OwnerLink, PayloadMiss> {
    if !family.links_owner {
        return Err(PayloadMiss::Foreign);
    }
    let cell = payload_cell(value, family.class_id)?;
    unsafe {
        if (*cell).magic != crate::native_handle::NATIVE_HANDLE_MAGIC
            || (*cell).finalized != 0
            || (*cell).creator_thread_id != crate::native_handle::current_thread_id()
        {
            return Err(PayloadMiss::Closed);
        }
        if (*cell).owner == 0 {
            return Err(PayloadMiss::Foreign);
        }
    }
    Ok(OwnerLink(cell as usize))
}

/// Never allocates or throws, including during finalization and on a wrong thread.
///
/// # Safety
/// `link` must name a live cell, owned by its C resource or kept alive by link_ref.
#[inline]
pub unsafe fn link_owner(link: OwnerLink) -> Option<f64> {
    let cell = link.0 as *mut NativeHandleHeader;
    #[cfg(test)]
    if callback_sabotage("finalized_check") {
        return Some(f64::from_bits((*cell).owner));
    }
    #[cfg(test)]
    if callback_sabotage("thread") {
        crate::native_handle::native_handle_rust_payload_ptr(cell, (*cell).type_id);
    }
    if crate::native_handle::rust_payload_ptr_on_owner_thread(cell).is_null() {
        return None;
    }
    ((*cell).owner != 0).then(|| f64::from_bits((*cell).owner))
}

/// Owner lookup for pump events, including terminal events after release.
/// Never throws or allocates. Read listeners from JS state at dispatch time.
///
/// # Safety
/// The cell must be alive on its creator thread, kept by one ref per queued
/// item. Drop items before worker heap teardown; never dispatch after teardown.
pub unsafe fn link_event_owner(link: OwnerLink) -> Option<f64> {
    let cell = link.0 as *mut NativeHandleHeader;
    if (*cell).magic != crate::native_handle::NATIVE_HANDLE_MAGIC
        || (*cell).finalized != 0
        || (*cell).creator_thread_id != crate::native_handle::current_thread_id()
    {
        return None;
    }
    ((*cell).owner != 0).then(|| f64::from_bits((*cell).owner))
}

#[repr(C)]
pub struct CallbackSite {
    pub link: OwnerLink,
    pub index: u32,
}

/// Stable userdata addresses; declare the C resource BEFORE this field so it
/// closes before the sites are freed. Native destroy callbacks do not free sites.
#[derive(Default)]
pub struct CallbackSites(Vec<Box<CallbackSite>>);
impl CallbackSites {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn site(&mut self, link: OwnerLink, index: u32) -> *mut c_void {
        let mut site = Box::new(CallbackSite { link, index });
        let ptr = &mut *site as *mut CallbackSite as *mut c_void;
        self.0.push(site);
        ptr
    }
}

/// One C call. Finish explicitly BEFORE result conversion or a JS throw, and
/// keep the receiver rooted across the call. No payload borrow/lock may span it.
#[must_use = "finish after C returns, before any conversion or throw"]
pub struct NativeCallGuard {
    link: OwnerLink,
    _thread: std::marker::PhantomData<std::rc::Rc<()>>,
}

pub fn enter(value: f64, family: &NativePayloadFamily) -> Result<NativeCallGuard, PayloadMiss> {
    if !family.links_owner {
        return Err(PayloadMiss::Foreign);
    }
    let cell = payload_cell(value, family.class_id)?;
    unsafe { enter_link(OwnerLink(cell as usize)) }
}

/// Enter through a previously validated, stable owner link. Each entry still
/// checks OPEN and thread affinity; keep the owner rooted for the entire call.
///
/// # Safety
/// As [`link_owner`]; `link` must remain live until the returned guard finishes.
#[inline]
pub unsafe fn enter_link(link: OwnerLink) -> Result<NativeCallGuard, PayloadMiss> {
    {
        let cell = link.0 as *mut NativeHandleHeader;
        if crate::native_handle::rust_payload_ptr_on_owner_thread(cell).is_null() {
            return Err(PayloadMiss::Closed);
        }
        if (*cell).owner == 0 {
            return Err(PayloadMiss::Foreign);
        }
        #[cfg(test)]
        if callback_sabotage("reentry") && (*cell).busy != 0 {
            return Err(PayloadMiss::Closed);
        }
        (*cell).busy = (*cell)
            .busy
            .checked_add(1)
            .expect("native call nesting overflow");
    }
    Ok(NativeCallGuard {
        link,
        _thread: std::marker::PhantomData,
    })
}

/// Finish outside C frames. A callback exception takes priority over close.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum CallEnd {
    Threw(f64),
    Closed,
}

impl NativeCallGuard {
    /// The validated link, usable while the rooted receiver keeps its cell alive.
    #[inline]
    pub fn owner_link(&self) -> OwnerLink {
        self.link
    }

    /// Run one native call and finish before its result can be converted.
    ///
    /// # Safety
    /// `f` may run only native code and non-throwing native data helpers.
    /// It may call JS only through the native callback protocol. No payload
    /// borrow or lock may span it. Keep the receiver rooted throughout.
    /// Between callbacks, `f` must not change managed stack state other than
    /// GC root scopes; callback argument conversion may add those scopes.
    #[inline]
    pub unsafe fn call<R>(self, f: impl FnOnce() -> R) -> (R, Result<(), CallEnd>) {
        let cell = self.link.0 as *mut NativeHandleHeader;
        let mut catch = crate::exception::NativeCatch::new();
        if (*cell).flags & CALLBACK_STORAGE == 0 {
            let value = f();
            return (value, self.finish());
        }
        let extended = cell as *mut NativeCallbackCell;
        let previous = (*extended).catch;
        (*extended).catch = &mut catch;
        let value = f();
        catch.finish();
        #[cfg(test)]
        let previous = if callback_sabotage("catch_reentry") {
            std::ptr::null_mut()
        } else {
            previous
        };
        (*extended).catch = previous;
        (value, self.finish())
    }

    /// Decrement busy, take the first pending exception, then complete a
    /// deferred close at zero. Return the exception to throw outside C frames.
    pub fn finish(self) -> Result<(), CallEnd> {
        unsafe {
            let cell = self.link.0 as *mut NativeHandleHeader;
            assert_ne!((*cell).busy, 0, "unbalanced native call");
            (*cell).busy -= 1;
            let closing = (*cell).flags & CLOSING != 0;
            if (*cell).flags & (PENDING | CLOSING) == 0 {
                return Ok(());
            }
            let scope = crate::gc::RuntimeHandleScope::new();
            let owner = scope.root_nanbox_f64(f64::from_bits((*cell).owner));
            let result = if (*cell).flags & PENDING != 0 {
                let state = root_pointer::<ObjectHeader>(
                    &scope,
                    js_state_for_class(owner.get_nanbox_f64(), (*cell).type_id as u32, false),
                );
                let err = scope.root_nanbox_f64(state_field(&state, b"pendingException"));
                // No allocation follows this take: the existing plain slot is cleared.
                set_state_field(&scope, &state, b"pendingException", undefined());
                (*cell).flags &= !PENDING;
                if closing && lifecycle_sabotage("closed_priority") {
                    Err(CallEnd::Closed)
                } else {
                    Err(CallEnd::Threw(err.get_nanbox_f64()))
                }
            } else if closing {
                Err(CallEnd::Closed)
            } else {
                Ok(())
            };
            if (*cell).busy == 0 && (*cell).flags & CLOSING != 0 {
                crate::native_handle::native_handle_release_rust_payload(cell);
            }
            result
        }
    }
}

fn undefined() -> f64 {
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

fn root_pointer<'a, T>(
    scope: &'a crate::gc::RuntimeHandleScope,
    value: f64,
) -> crate::gc::RuntimeHandle<'a> {
    scope.root_raw_mut_ptr(crate::JSValue::from_bits(value.to_bits()).as_pointer::<T>() as *mut T)
}

/// Runtime-owned data slots: no prototype lookup or accessor invocation can
/// run JS while a native trampoline is preparing or parking an exception.
fn state_field(state: &crate::gc::RuntimeHandle<'_>, key: &[u8]) -> f64 {
    state.with_mut_ptr::<ObjectHeader, _>(|obj| unsafe {
        state_key_index(obj, key).map_or_else(undefined, |i| state_slot_get(obj, i))
    })
}

/// [`state_field`] through a per-site [`StateKeyMemo`].
fn state_field_memo(
    state: &crate::gc::RuntimeHandle<'_>,
    key: &[u8],
    memo: &'static StateKeyMemo,
) -> f64 {
    state.with_mut_ptr::<ObjectHeader, _>(|obj| unsafe {
        state_key_index_memo(obj, key, memo).map_or_else(undefined, |i| state_slot_get(obj, i))
    })
}

/// The slot a JS-state key had under one ShapeId, remembered per call site
/// and per agent (ShapeIds are per agent), exactly like an emitted `o.key`
/// site's compact word: one ShapeId compare, then one load. A miss walks the
/// keys and re-primes. Declare one with [`state_key_memo!`].
pub type StateKeyMemo = std::thread::LocalKey<std::cell::Cell<u64>>;

/// Declare a [`StateKeyMemo`]: `state_key_memo!(static MEMO_DB);`.
#[macro_export]
macro_rules! state_key_memo {
    ($vis:vis static $name:ident) => {
        ::std::thread_local! {
            $vis static $name: ::std::cell::Cell<u64> = const { ::std::cell::Cell::new(0) };
        }
    };
}

unsafe fn state_key_index_memo(
    obj: *const ObjectHeader,
    name: &[u8],
    memo: &'static StateKeyMemo,
) -> Option<usize> {
    let shape = crate::object::shapes::object_shape_stamp(obj);
    if shape != 0 {
        let word = memo.with(|cell| cell.get());
        if (word >> 32) as u32 == shape {
            return Some((word as u32) as usize);
        }
    }
    let i = state_key_index(obj, name)?;
    if shape != 0 {
        memo.with(|cell| cell.set(((shape as u64) << 32) | i as u64));
    }
    Some(i)
}

/// Read a field of a live object through a memo. Never allocates, so no
/// handle scope is needed around it.
unsafe fn raw_field_memo(obj: *mut ObjectHeader, key: &[u8], memo: &'static StateKeyMemo) -> f64 {
    state_key_index_memo(obj, key, memo).map_or_else(undefined, |i| state_slot_get(obj, i))
}

/// The JS state object of `obj` if it has one (no allocation).
unsafe fn raw_js_state(obj: *mut ObjectHeader) -> Option<*mut ObjectHeader> {
    let state = raw_field_memo(obj, JS_STATE_KEY, &JS_STATE_MEMO);
    let js = crate::value::JSValue::from_bits(state.to_bits());
    js.is_pointer()
        .then(|| js.as_pointer::<ObjectHeader>() as *mut ObjectHeader)
}

state_key_memo!(static JS_STATE_MEMO);
state_key_memo!(static CALLBACKS_MEMO);
state_key_memo!(static PENDING_MEMO);

/// A state field past the inline slots lives in the object's overflow
/// storage (a state grows by definition, one key at a time).
unsafe fn state_slot_get(obj: *mut ObjectHeader, i: usize) -> f64 {
    // This is a live runtime-owned object, and the key lookup already proved
    // the slot index. Resolve its inline bound once; the shared helper also
    // handles overflow and the invalid null-pointer sentinel.
    let live = crate::object::object_live_slot_count(obj);
    f64::from_bits(crate::object::object_field_at_with_live(obj, i as u32, live).bits())
}

unsafe fn state_slot_set(obj: *mut ObjectHeader, i: usize, value: f64) {
    if i < state_inline_limit(obj) {
        // The ordinary indexed store includes the exact-slot barrier.
        crate::object::js_object_set_field(
            obj,
            i as u32,
            crate::JSValue::from_bits(value.to_bits()),
        );
    } else {
        crate::object::overflow_set(obj as usize, i, value.to_bits());
    }
}

unsafe fn state_inline_limit(obj: *mut ObjectHeader) -> usize {
    (crate::object::object_live_slot_count(obj) as usize).max(crate::object::INLINE_SLOT_FLOOR)
}

unsafe fn state_key_index(obj: *const ObjectHeader, name: &[u8]) -> Option<usize> {
    let keys = crate::object::object_keys(obj);
    let (slots, len) = keys.dense_slots();
    (0..(keys.count() as usize).min(len)).find(|&i| {
        crate::string::js_string_key_matches_bytes(
            crate::JSValue::from_bits((*slots.add(i)).to_bits()),
            name,
        )
    })
}

fn set_state_field(
    scope: &crate::gc::RuntimeHandleScope,
    state: &crate::gc::RuntimeHandle<'_>,
    key: &[u8],
    value: f64,
) {
    let value = scope.root_nanbox_f64(value);
    if state.with_mut_ptr::<ObjectHeader, _>(|obj| unsafe {
        if let Some(i) = state_key_index(obj, key) {
            state_slot_set(obj, i, value.get_nanbox_f64());
            true
        } else {
            false
        }
    }) {
        return;
    }
    let key = scope.root_string_ptr(crate::string::intern_ascii_literal(key));
    key.with_const_ptr::<crate::StringHeader, _>(|key| {
        state.with_mut_ptr::<ObjectHeader, _>(|obj| unsafe {
            // Define an own data slot directly; [[Set]] could call an inherited setter.
            crate::object::object_ops::define_property_force_store_value(
                obj,
                key,
                value.get_nanbox_f64(),
            );
        });
    });
}

/// The callbacks array on the owner's traced JS state, created on demand.
pub fn callbacks(owner: f64, family: &NativePayloadFamily) -> f64 {
    if !is_instance(owner, family) {
        return undefined();
    }
    if let Some(obj) = instance_of(owner, family.class_id) {
        unsafe {
            if let Some(state) = raw_js_state(obj) {
                let existing = raw_field_memo(state, b"callbacks", &CALLBACKS_MEMO);
                if crate::JSValue::from_bits(existing.to_bits()).is_pointer() {
                    return existing;
                }
            }
        }
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let owner = scope.root_nanbox_f64(owner);
    let state =
        root_pointer::<ObjectHeader>(&scope, js_state(owner.get_nanbox_f64(), family, true));
    let existing = state_field_memo(&state, b"callbacks", &CALLBACKS_MEMO);
    if crate::JSValue::from_bits(existing.to_bits()).is_pointer() {
        return existing;
    }
    let array = scope.root_raw_mut_ptr(crate::array::js_array_alloc(0));
    let value = array.with_mut_ptr::<crate::array::ArrayHeader, _>(|p| {
        crate::value::js_nanbox_pointer(p as i64)
    });
    set_state_field(&scope, &state, b"callbacks", value);
    // Re-read the array after the state store, which may collect.
    sync_callbacks(
        owner.get_nanbox_f64(),
        family,
        b"callbacks",
        array.with_mut_ptr::<crate::array::ArrayHeader, _>(|p| {
            crate::value::js_nanbox_pointer(p as i64)
        }),
    );
    array
        .with_mut_ptr::<crate::array::ArrayHeader, _>(|p| crate::value::js_nanbox_pointer(p as i64))
}

pub fn set_callback(owner: f64, family: &NativePayloadFamily, index: u32, f: f64) {
    if !is_instance(owner, family) {
        return;
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let f = scope.root_nanbox_f64(f);
    let owner = scope.root_nanbox_f64(owner);
    let array = root_pointer::<crate::array::ArrayHeader>(
        &scope,
        callbacks(owner.get_nanbox_f64(), family),
    );
    let updated = array.with_mut_ptr::<crate::array::ArrayHeader, _>(|p| {
        crate::array::js_array_set_f64_extend(p, index, f.get_nanbox_f64())
    });
    let updated = scope.root_raw_mut_ptr(updated);
    let state =
        root_pointer::<ObjectHeader>(&scope, js_state(owner.get_nanbox_f64(), family, true));
    set_state_field(
        &scope,
        &state,
        b"callbacks",
        updated.with_mut_ptr::<crate::array::ArrayHeader, _>(|p| {
            crate::value::js_nanbox_pointer(p as i64)
        }),
    );
    sync_callbacks(
        owner.get_nanbox_f64(),
        family,
        b"callbacks",
        updated.with_mut_ptr::<crate::array::ArrayHeader, _>(|p| {
            crate::value::js_nanbox_pointer(p as i64)
        }),
    );
}

/// Call JS under a catch, park the first throw on the owner and return an error
/// to C. A pending throw suppresses every subsequent callback until finish.
///
/// # Safety
/// Values and args must be valid on the creator thread. A C trampoline must
/// root the owner before any allocation and must never throw through C frames.
pub unsafe fn call_from_native(
    owner: f64,
    callee: f64,
    this: f64,
    args: &[f64],
) -> Result<f64, ()> {
    let bits = owner.to_bits();
    if bits & crate::value::TAG_MASK != crate::value::POINTER_TAG {
        return Err(());
    }
    let Some(header) =
        crate::value::addr_class::try_read_gc_header((bits & crate::value::POINTER_MASK) as usize)
    else {
        return Err(());
    };
    if header.obj_type != crate::gc::GC_TYPE_OBJECT {
        return Err(());
    }
    let class_id = (*((bits & crate::value::POINTER_MASK) as *const ObjectHeader)).class_id;
    let Ok(cell) = payload_cell(owner, class_id) else {
        return Err(());
    };
    if crate::native_handle::rust_payload_ptr_on_owner_thread(cell).is_null()
        || ((*cell).flags & PENDING != 0 && !pending_sabotage())
    {
        return Err(());
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let owner = scope.root_nanbox_f64(owner);
    let callee = scope.root_nanbox_f64(callee);
    let this = scope.root_nanbox_f64(this);
    let args_roots: Vec<_> = args.iter().map(|&v| scope.root_nanbox_f64(v)).collect();
    // Pre-create the pending field before running JS. The throw landing only
    // writes an existing slot: it must not allocate with an unrooted exception.
    ensure_pending_slot(cell, owner.get_nanbox_f64(), class_id);
    let args: Vec<_> = args_roots.iter().map(|v| v.get_nanbox_f64()).collect();
    #[cfg(test)]
    if callback_sabotage("catch") {
        return Ok(crate::closure::native_call_value_this(
            callee.get_nanbox_f64(),
            crate::closure::JsThis::from_f64(this.get_nanbox_f64()),
            args.as_ptr(),
            args.len(),
        ));
    }
    match crate::exception::catch_native_callback(
        callback_slots::catch_from_cell(cell),
        callee.get_nanbox_f64(),
        this.get_nanbox_f64(),
        &args,
    ) {
        Ok(value) => Ok(value),
        Err(err) => {
            // JS may have moved the owner. Re-read it through its handle;
            // the pending helper also preserves an earlier nested failure.
            let _ = set_pending_exception(owner.get_nanbox_f64(), err);
            Err(())
        }
    }
}

/// The throw landing must not allocate with an unrooted exception, so the
/// pending slot exists before JS runs. While PENDING is clear it holds
/// undefined (finish and the landing keep that invariant): create it once
/// per owner and remember that on the cell.
unsafe fn ensure_pending_slot(cell: *mut NativeHandleHeader, owner: f64, class_id: u32) {
    if (*cell).flags & PENDING_SLOT != 0 {
        return;
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let owner = scope.root_nanbox_f64(owner);
    let state = root_pointer::<ObjectHeader>(
        &scope,
        js_state_for_class(owner.get_nanbox_f64(), class_id, true),
    );
    if state.with_mut_ptr::<ObjectHeader, _>(|obj| {
        state_key_index_memo(obj, b"pendingException", &PENDING_MEMO).is_none()
    }) {
        set_state_field(&scope, &state, b"pendingException", undefined());
    }
    (*cell).flags |= PENDING_SLOT;
}

/// [`call_from_native`] for an S trampoline that holds the userdata's link
/// and proved it with [`link_owner`] on entry. The cell's state is
/// re-checked on every call (an earlier callback may have closed the owner
/// or parked an exception); its thread cannot change, so the trampoline's
/// proof stands. The owner is re-read from the cell (the GC rewrites the
/// slot). The caller keeps `callee` and every pointer in `args` rooted, or
/// reads them after its last allocation.
///
/// # Safety
/// As [`call_from_native`]; `link` passed [`link_owner`] on this thread in
/// the calling trampoline.
#[inline]
pub unsafe fn call_from_link(link: OwnerLink, callee: f64, args: &[f64]) -> Result<f64, ()> {
    let cell = link.0 as *mut NativeHandleHeader;
    if !cell_open(cell) || ((*cell).flags & PENDING != 0 && !pending_sabotage()) {
        return Err(());
    }
    if (*cell).flags & PENDING_SLOT == 0 {
        return first_call_from_link(link, callee, args);
    }
    call_open_link(cell, callee, args)
}

/// The owner's first callback: create its pending slot (see
/// [`ensure_pending_slot`]), then call.
#[cold]
#[inline(never)]
unsafe fn first_call_from_link(link: OwnerLink, callee: f64, args: &[f64]) -> Result<f64, ()> {
    let cell = link.0 as *mut NativeHandleHeader;
    let scope = crate::gc::RuntimeHandleScope::new();
    let callee_root = scope.root_nanbox_f64(callee);
    let roots: Vec<_> = args.iter().map(|v| scope.root_nanbox_f64(*v)).collect();
    ensure_pending_slot(cell, f64::from_bits((*cell).owner), (*cell).type_id as u32);
    let args: Vec<f64> = roots.iter().map(|v| v.get_nanbox_f64()).collect();
    call_from_link(link, callee_root.get_nanbox_f64(), &args)
}

/// [`call_from_link`] once the cell is open, nothing is pending and the
/// pending slot exists.
unsafe fn call_open_link(
    cell: *mut NativeHandleHeader,
    callee: f64,
    args: &[f64],
) -> Result<f64, ()> {
    let this = undefined();
    #[cfg(test)]
    if callback_sabotage("catch") {
        return Ok(crate::closure::native_call_value_this(
            callee,
            crate::closure::JsThis::from_f64(this),
            args.as_ptr(),
            args.len(),
        ));
    }
    match crate::exception::catch_native_callback(
        callback_slots::catch_from_cell(cell),
        callee,
        this,
        args,
    ) {
        Ok(value) => Ok(value),
        Err(err) => {
            let _ = set_pending_exception(f64::from_bits((*cell).owner), err);
            Err(())
        }
    }
}

/// The open-cell half of [`link_owner`] (payload installed, neither closing
/// nor finalized), for a cell whose thread is already proven.
#[inline]
unsafe fn cell_open(cell: *mut NativeHandleHeader) -> bool {
    (*cell).finalized == 0 && (*cell).flags & CLOSING == 0 && !(*cell).resource_ptr.is_null()
}

/// Park a trampoline validation failure without throwing through C. Returns
/// Err when the receiver is invalid/finalized/wrong-thread or an earlier throw
/// is pending. Accepts CLOSING while a guard still owns the native call.
/// The first pending exception wins; this function never calls JS.
pub fn set_pending_exception(owner: f64, exception: f64) -> Result<(), ()> {
    let bits = owner.to_bits();
    if bits & crate::value::TAG_MASK != crate::value::POINTER_TAG {
        return Err(());
    }
    let addr = (bits & crate::value::POINTER_MASK) as usize;
    let Some(header) = (unsafe { crate::value::addr_class::try_read_gc_header(addr) }) else {
        return Err(());
    };
    if header.obj_type != crate::gc::GC_TYPE_OBJECT {
        return Err(());
    }
    let class_id = unsafe { (*(addr as *const ObjectHeader)).class_id };
    let cell = payload_cell(owner, class_id).map_err(|_| ())?;
    unsafe {
        if (*cell).magic != crate::native_handle::NATIVE_HANDLE_MAGIC
            || (*cell).finalized != 0
            || (*cell).creator_thread_id != crate::native_handle::current_thread_id()
            || (*cell).flags & PENDING != 0
        {
            return Err(());
        }
        let scope = crate::gc::RuntimeHandleScope::new();
        let owner = scope.root_nanbox_f64(owner);
        let exception = scope.root_nanbox_f64(exception);
        let state = root_pointer::<ObjectHeader>(
            &scope,
            js_state_for_class(owner.get_nanbox_f64(), class_id, true),
        );
        set_state_field(
            &scope,
            &state,
            b"pendingException",
            exception.get_nanbox_f64(),
        );
        (*cell).flags |= PENDING;
        Ok(())
    }
}

/// # Safety
/// Call on the creator thread with a live cell, once per outstanding item.
pub unsafe fn link_ref(link: OwnerLink) {
    let cell = link.0 as *mut NativeHandleHeader;
    assert_eq!(
        (*cell).creator_thread_id,
        crate::native_handle::current_thread_id()
    );
    (*cell).refs = (*cell).refs.checked_add(1).expect("native refs overflow");
    #[cfg(test)]
    if callback_sabotage("pin") {
        return;
    }
    if (*cell).refs == 1 {
        #[cfg(test)]
        if callback_sabotage("latch") {
            crate::gc::pin_object(
                (((*cell).owner & crate::value::POINTER_MASK) as *mut u8)
                    .sub(crate::gc::GC_HEADER_SIZE) as *mut crate::gc::GcHeader,
            );
            return;
        }
        crate::gc::pin_object_non_young(
            (cell as *mut u8).sub(crate::gc::GC_HEADER_SIZE) as *mut crate::gc::GcHeader
        );
    }
}

/// # Safety
/// Match a link_ref on the creator thread, even when explicitly closed.
pub unsafe fn link_unref(link: OwnerLink) {
    let cell = link.0 as *mut NativeHandleHeader;
    assert_eq!(
        (*cell).creator_thread_id,
        crate::native_handle::current_thread_id()
    );
    assert_ne!((*cell).refs, 0, "unbalanced native unref");
    (*cell).refs -= 1;
    if (*cell).refs == 0 {
        crate::gc::unpin_object(
            (cell as *mut u8).sub(crate::gc::GC_HEADER_SIZE) as *mut crate::gc::GcHeader
        );
    }
}

/// Explicitly close a payload attached to an ordinary subclass instance.
///
/// Unlike [`close`], this accepts an object whose own class id differs from
/// the native base family.  The typed probe is what proves that the attached
/// cell belongs to `family`; a foreign native cell is never disposed.
pub fn close_attached<T: 'static>(value: f64, family: &NativePayloadFamily) -> bool {
    if unsafe { payload_mut_attached::<T>(value, family) }.is_err() {
        return false;
    }
    let Some(obj) = any_object(value) else {
        return false;
    };
    let meta = unsafe { (*obj).meta };
    if meta.is_null() || !is_payload_state_word(unsafe { (*meta).native_state }) {
        return false;
    }
    let cell =
        (unsafe { (*meta).native_state } & crate::value::POINTER_MASK) as *mut NativeHandleHeader;
    unsafe { crate::native_handle::native_handle_release_rust_payload(cell) }
}

/// Re-state the native bytes a live payload retains (after a buffer grew or
/// was released). No-op for a foreign or closed value.
pub fn set_external_bytes(value: f64, family: &NativePayloadFamily, bytes: usize) {
    if let Ok(cell) = payload_cell(value, family.class_id) {
        unsafe { crate::native_handle::native_handle_set_external_bytes(cell, bytes) };
    }
}

/// The slot index of `name` among `obj`'s own string keys, read from its
/// shape's keys array. `None` when absent (or the shape publishes no keys).
unsafe fn shape_key_index(obj: *const ObjectHeader, name: &[u8]) -> Option<usize> {
    let descriptor = crate::object::shapes::object_shape_descriptor(obj)?;
    let keys = descriptor.keys as usize as *const crate::array::ArrayHeader;
    if keys.is_null() || !crate::value::addr_class::is_above_handle_band(keys as usize) {
        return None;
    }
    let (slots, slot_len) = crate::object::keys_array_dense_slots_resolved(keys);
    let key_count = (descriptor.logical_key_count as usize).min(slot_len);
    (0..key_count).find(|&i| {
        let key = crate::JSValue::from_bits((*slots.add(i)).to_bits());
        crate::string::js_string_key_matches_bytes(key, name)
    })
}

/// `x.method(...)` on a payload-family instance, answered directly when the
/// call provably resolves to a DATA property of the family's prototype (the
/// same proof `timer::try_timer_method_fast_dispatch` makes, generalized to
/// every family). Called from a method site's miss (`method_site.rs`, which
/// classifies the receiver by its class id) and from the tower's class arm
/// (`handle_methods.rs`), so no other receiver pays for the check:
///
/// * the receiver is an instance of a family whose prototype exists, its
///   `[[Prototype]]` is still that prototype, it is not in dictionary mode,
///   it has no own key and no recorded descriptor for the name;
/// * the prototype is not in dictionary mode, has no recorded descriptor
///   (accessor) for the name, and holds the name as an own data property
///   whose value is a function.
///
/// The value found is the one the tower would call, so user replacements
/// (`Hash.prototype.update = f`) are honoured; every other shape of change
/// fails a check and takes the tower. The builtin methods are not admitted by
/// the per-site memos (those enter compiled user bodies only), so without this
/// every call paid a failed site prime, the tower's probes and a by-name
/// prototype walk (about 2x the old handle dispatch on a hashing loop).
///
/// # Safety
/// `args_ptr` holds `args_len` values.
pub(crate) unsafe fn try_payload_method_fast_dispatch(
    object: f64,
    name: &[u8],
    args_ptr: *const f64,
    args_len: usize,
) -> Option<f64> {
    let bits = object.to_bits();
    if bits & crate::value::TAG_MASK != crate::value::POINTER_TAG || name.is_empty() {
        return None;
    }
    let addr = (bits & crate::value::POINTER_MASK) as usize;
    let header = crate::value::addr_class::try_read_gc_header(addr)?;
    if header.obj_type != crate::gc::GC_TYPE_OBJECT {
        return None;
    }
    let obj = addr as *const ObjectHeader;
    let index = slot_index_if_payload((*obj).class_id)?;
    let proto = PAYLOAD_PROTOTYPES.with(|slots| slots[index].load(Ordering::Acquire))
        as *const ObjectHeader;
    if proto.is_null()
        || crate::object::shapes::object_prototype_word(obj)
            != crate::value::js_nanbox_pointer(proto as i64).to_bits()
    {
        return None;
    }
    // Both are ordinary objects, whose keys carry every key's attributes
    // (charter step 3): the instance owning no key of this name has no own
    // descriptor for it, and the prototype's key entry says whether it is an
    // accessor.
    if crate::object::dictionary::is_dictionary(obj)
        || shape_key_index(obj, name).is_some()
        || crate::object::dictionary::is_dictionary(proto)
        || crate::object::key_attrs::object_key_is_accessor(proto, name)
    {
        return None;
    }
    let slot = shape_key_index(proto, name)?;
    let value = crate::object::js_object_get_field(proto, slot as u32);
    if !value.is_pointer() {
        return None;
    }
    let closure = value.as_pointer::<crate::closure::ClosureHeader>();
    if !crate::closure::is_closure_ptr(closure as usize) {
        return None;
    }
    Some(crate::closure::native_call_value_this(
        f64::from_bits(value.bits()),
        crate::closure::JsThis::from_f64(object),
        args_ptr,
        args_len,
    ))
}

/// The hidden own property holding a family's JS-side state.
pub(crate) const JS_STATE_KEY: &[u8] = b"#<perry:native-payload-js-state>";

/// The per-object JS state object of an instance: an ordinary object stored
/// under a hidden own key (invisible to `Object.keys`, `for…in`,
/// `getOwnPropertyNames`, `JSON.stringify`), so every JS value a family keeps
/// for an instance is a traced field that moves and dies with it. Created on
/// first use when `create` is true; `undefined` otherwise when absent.
pub fn js_state(value: f64, family: &NativePayloadFamily, create: bool) -> f64 {
    js_state_for_class(value, family.class_id, create)
}

fn js_state_for_class(value: f64, class_id: u32, create: bool) -> f64 {
    let undefined = undefined();
    let Some(obj) = instance_of(value, class_id) else {
        return undefined;
    };
    if let Some(state) = unsafe { raw_js_state(obj) } {
        return crate::value::js_nanbox_pointer(state as i64);
    }
    if !create {
        return undefined;
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let obj = scope.root_raw_mut_ptr(obj);
    let existing = state_field_memo(&obj, JS_STATE_KEY, &JS_STATE_MEMO);
    if crate::value::JSValue::from_bits(existing.to_bits()).is_pointer() || !create {
        return existing;
    }
    let state = crate::object::js_object_alloc(0, 6);
    if state.is_null() {
        return undefined;
    }
    let state = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(state as i64));
    set_state_field(&scope, &obj, JS_STATE_KEY, state.get_nanbox_f64());
    state.get_nanbox_f64()
}

/// The family whose class id is `class_id`, for `instanceof` against a module
/// export. Families register nothing at runtime: this is the static list of
/// node constructors that answer by class id.
pub(crate) fn export_class_id(module: &str, export: &str) -> Option<u32> {
    use crate::native_class_ids as ids;
    Some(match (module, export) {
        ("crypto", "Hash") => ids::CRYPTO_HASH,
        ("crypto", "Hmac") => ids::CRYPTO_HMAC,
        ("crypto", "Cipheriv") => ids::CRYPTO_CIPHERIV,
        ("crypto", "Decipheriv") => ids::CRYPTO_DECIPHERIV,
        ("async_hooks", "AsyncLocalStorage") => ids::ASYNC_LOCAL_STORAGE_LEGACY,
        ("async_hooks", "AsyncResource") => ids::ASYNC_RESOURCE_LEGACY,
        ("domain", "Domain") => ids::DOMAIN,
        ("sqlite", "DatabaseSync") => ids::SQLITE_DATABASE_SYNC,
        ("sqlite", "StatementSync") => ids::SQLITE_STATEMENT_SYNC,
        ("sqlite", "Session") => ids::SQLITE_SESSION,
        ("zlib", "Gzip") => ids::GZIP,
        ("zlib", "Gunzip") => ids::GUNZIP,
        ("zlib", "Deflate") => ids::DEFLATE,
        ("zlib", "Inflate") => ids::INFLATE,
        ("zlib", "DeflateRaw") => ids::DEFLATE_RAW,
        ("zlib", "InflateRaw") => ids::INFLATE_RAW,
        ("zlib", "Unzip") => ids::UNZIP,
        ("zlib", "BrotliCompress") => ids::BROTLI_COMPRESS,
        ("zlib", "BrotliDecompress") => ids::BROTLI_DECOMPRESS,
        ("zlib", "ZstdCompress") => ids::ZSTD_COMPRESS,
        ("zlib", "ZstdDecompress") => ids::ZSTD_DECOMPRESS,

        _ => return None,
    })
}

/// Forget this thread's prototypes. Test-only: a GC test that builds a
/// prototype inside an isolated scanner window must not leave a slot naming
/// an object a later test's collection reclaimed.
#[cfg(test)]
pub(crate) fn reset_payload_prototypes_for_tests() {
    PAYLOAD_PROTOTYPES.with(|slots| {
        for slot in slots.iter() {
            slot.store(0, Ordering::Release);
        }
    });
}

// Faults exist only in unit-test binaries, selected by child processes. No
// production flags, owner registries or latch are introduced.
#[cfg(test)]
pub(crate) fn callback_sabotage(fault: &str) -> bool {
    std::env::var("PERRY_TEST_CALLBACK_SABOTAGE").as_deref() == Ok(fault)
}
#[inline]
fn pending_sabotage() -> bool {
    #[cfg(test)]
    {
        callback_sabotage("pending")
    }
    #[cfg(not(test))]
    {
        false
    }
}

#[inline]
pub(crate) fn lifecycle_sabotage(fault: &str) -> bool {
    #[cfg(test)]
    {
        std::env::var("PERRY_TEST_LIFECYCLE_SABOTAGE").as_deref() == Ok(fault)
    }
    #[cfg(not(test))]
    {
        let _ = fault;
        false
    }
}
