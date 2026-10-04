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
//! runs exactly once, at whichever comes first of [`close`] (the family's
//! explicit close/final/digest), the sweep that finds the cell dead, or the
//! owning thread's teardown. After it runs the object stays a valid object:
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
//! bits, no raw GC pointers — the cell is a GC leaf and nothing traces into
//! it. A JS value the family must keep (listeners, a pipe destination, an
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
use std::sync::atomic::{AtomicI64, Ordering};

use crate::native_handle::NativeHandleHeader;
use crate::object::ObjectHeader;

/// A family: one class id, one payload type, one prototype per realm.
pub struct NativePayloadFamily {
    /// The family's id from `native_class_ids.rs`.
    pub class_id: u32,
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
    /// Install a builtin method: writable, non-enumerable, configurable, with
    /// the given `.name` and `.length`. `info` comes from `perry_runtime::fn_info!`
    /// with `with_declared(arity)`; the body receives `this` and `arity`
    /// arguments (missing ones arrive as `undefined`).
    pub fn method(&mut self, name: &str, info: *const crate::closure::JsFunctionInfo, arity: u32) {
        crate::object::install_proto_method(self.proto, name, info, arity);
    }
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
    crate::native_class_ids::CRYPTO_DECIPHERIV - crate::native_class_ids::WEB_BUILTIN_BLOCK_START
        < PROTOTYPE_SLOTS as u32,
    "a native-payload family id outgrew the prototype slot array"
);

#[inline]
fn slot_index(class_id: u32) -> usize {
    let index = class_id.wrapping_sub(crate::native_class_ids::WEB_BUILTIN_BLOCK_START) as usize;
    assert!(
        index < PROTOTYPE_SLOTS,
        "class id {class_id:#x} is not in the web-builtin block"
    );
    index
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
    let proto = crate::object::js_object_alloc(0, 0);
    if proto.is_null() {
        return proto;
    }
    let mut builder = PayloadPrototype { proto };
    (family.install_prototype)(&mut builder);
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
    if crate::value::JSValue::from_bits(constructor.to_bits()).is_pointer() {
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

/// Is `word` (an `ObjectMeta.native_state`) a payload cell reference? The
/// one predicate the meta record's GC arm uses to decide whether the word is
/// an edge. Only payload families store a POINTER_TAG-boxed word there.
#[inline(always)]
pub(crate) fn is_payload_state_word(word: u64) -> bool {
    word & crate::value::TAG_MASK == crate::value::POINTER_TAG
        && word & crate::value::POINTER_MASK != 0
}

/// Allocate an instance of `family` owning `payload`.
///
/// `external_bytes` is the native memory the payload really retains (heap
/// buffers it owns, not `size_of::<T>()` alone unless that is all it holds);
/// it feeds GC pacing until the payload is finalized. `own` lists node's own
/// enumerable data properties in node's order (`[("_options", undefined)]`
/// for a `Hash`); values may be heap values, they are rooted here.
pub fn alloc<T: 'static>(
    family: &'static NativePayloadFamily,
    payload: T,
    external_bytes: usize,
    own: &[(&[u8], f64)],
) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let own_roots: Vec<_> = own
        .iter()
        .map(|&(key, value)| (key, scope.root_nanbox_f64(value)))
        .collect();
    let proto = family_prototype(family);
    if proto.is_null() {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    let obj = unsafe { born_instance(family.class_id, proto, own.len() as u32) };
    if obj.is_null() {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    let obj = scope.root_raw_mut_ptr(obj);
    for (key, value) in &own_roots {
        set_own(&scope, &obj, key, value.get_nanbox_f64());
    }
    let meta = obj
        .with_mut_ptr::<ObjectHeader, _>(|obj| unsafe { crate::object::object_meta_ensure(obj) });
    if meta.is_null() {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    // The cell allocation may collect, which can move the meta record; it is
    // re-read through the rooted object after.
    let resource = Box::into_raw(Box::new(payload)) as *mut c_void;
    let cell = unsafe {
        crate::native_handle::native_handle_new_rust_payload(
            resource,
            type_tag::<T>(family.class_id),
            drop_payload::<T>,
            family.name,
        )
    };
    let word = crate::value::JSValue::pointer(cell as *const u8).bits();
    obj.with_mut_ptr::<ObjectHeader, _>(|obj| unsafe {
        let meta = (*obj).meta;
        debug_assert!(!meta.is_null(), "object_meta_ensure ran above");
        // GC_STORE_AUDIT(BARRIERED): metadata-record slot store + object
        // barrier, exactly as `ObjectMeta::arguments` is stored.
        (*meta).native_state = word;
        crate::gc::runtime_write_barrier_slot(
            meta as usize,
            &(*meta).native_state as *const _ as usize,
            word,
        );
    });
    // Only now, with the cell reachable from the rooted object: reporting the
    // bytes can start a collection.
    if external_bytes != 0 {
        unsafe { crate::native_handle::native_handle_set_external_bytes(cell, external_bytes) };
    }
    obj.with_mut_ptr::<ObjectHeader, _>(|obj| crate::value::js_nanbox_pointer(obj as i64))
}

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
unsafe fn born_instance(class_id: u32, proto: *mut ObjectHeader, slots: u32) -> *mut ObjectHeader {
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
    let bits = value.to_bits();
    if bits & crate::value::TAG_MASK != crate::value::POINTER_TAG {
        return None;
    }
    let addr = (bits & crate::value::POINTER_MASK) as usize;
    let header = unsafe { crate::value::addr_class::try_read_gc_header(addr)? };
    if header.obj_type != crate::gc::GC_TYPE_OBJECT {
        return None;
    }
    let obj = addr as *mut ObjectHeader;
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

/// Explicit close: drop the payload now and release its external bytes. The
/// object stays valid and reads as [`PayloadMiss::Closed`] afterwards. True
/// when this call dropped it; false for a foreign value or one already closed.
pub fn close(value: f64, family: &NativePayloadFamily) -> bool {
    match payload_cell(value, family.class_id) {
        Ok(cell) => unsafe { crate::native_handle::native_handle_dispose_rust_payload(cell) },
        Err(_) => false,
    }
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
    let index = (*obj)
        .class_id
        .wrapping_sub(crate::native_class_ids::WEB_BUILTIN_BLOCK_START) as usize;
    if index >= PROTOTYPE_SLOTS {
        return None;
    }
    let proto = PAYLOAD_PROTOTYPES.with(|slots| slots[index].load(Ordering::Acquire))
        as *const ObjectHeader;
    if proto.is_null()
        || crate::object::shapes::object_prototype_word(obj)
            != crate::value::js_nanbox_pointer(proto as i64).to_bits()
    {
        return None;
    }
    let name_str = std::str::from_utf8(name).ok()?;
    if crate::object::dictionary::is_dictionary(obj)
        || crate::object::descriptor_state::may_have_descriptor_entry(obj as usize, name_str, true)
        || shape_key_index(obj, name).is_some()
        || crate::object::dictionary::is_dictionary(proto)
        || crate::object::descriptor_state::may_have_descriptor_entry(
            proto as usize,
            name_str,
            true,
        )
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
    let undefined = f64::from_bits(crate::value::TAG_UNDEFINED);
    let Some(obj) = instance_of(value, family.class_id) else {
        return undefined;
    };
    let scope = crate::gc::RuntimeHandleScope::new();
    let obj = scope.root_raw_mut_ptr(obj);
    let key = scope.root_string_ptr(crate::string::intern_ascii_literal(JS_STATE_KEY));
    let existing = key.with_const_ptr::<crate::StringHeader, _>(|key| {
        obj.with_mut_ptr::<ObjectHeader, _>(|obj| {
            crate::object::js_object_get_field_by_name_f64(obj, key)
        })
    });
    if crate::value::JSValue::from_bits(existing.to_bits()).is_pointer() || !create {
        return existing;
    }
    let state = crate::object::js_object_alloc(0, 4);
    if state.is_null() {
        return undefined;
    }
    let state = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(state as i64));
    set_own(&scope, &obj, JS_STATE_KEY, state.get_nanbox_f64());
    state.get_nanbox_f64()
}

/// `obj[key] = value` for a runtime-owned ASCII key, with the key rooted
/// across the store (the store may allocate a shape transition).
fn set_own(
    scope: &crate::gc::RuntimeHandleScope,
    obj: &crate::gc::RuntimeHandle<'_>,
    key: &[u8],
    value: f64,
) {
    let value = scope.root_nanbox_f64(value);
    let key = scope.root_string_ptr(crate::string::intern_ascii_literal(key));
    key.with_const_ptr::<crate::StringHeader, _>(|key| {
        obj.with_mut_ptr::<ObjectHeader, _>(|obj| {
            crate::object::js_object_set_field_by_name(obj, key, value.get_nanbox_f64())
        })
    });
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
