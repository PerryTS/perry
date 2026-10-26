//! Sync generator births and their object-owned resume record.
//!
//! The public object has no protocol properties. Its shape proves the private
//! generator brand and its prototype at birth. `meta.native_state` is the
//! already traced GC edge to a single closure whose captures are fixed state
//! slots. No address registry, own-method lookup or const-function finalize.

use super::generator::*;
use super::*;
use crate::codegen_abi::LayoutWord;

// Private evaluation brands occupy the low serial range. Reserve a high
// identity for the intrinsic sync-generator brand, carried by all transitions.
pub(crate) const GENERATOR_STATE_BRAND: u64 = u64::MAX - 2;

/// A fresh class evaluation owns the actual functions of its symbol methods.
/// Their keys are retained in its existing computed-declaration slots.
pub(crate) unsafe fn install_evaluation_generator_symbols(proto: f64, owner: f64, class_id: u32) {
    let Some(decl) = crate::object::class_registry::declarations::class_declaration(class_id)
    else {
        return;
    };
    let scope = crate::gc::RuntimeHandleScope::new();
    let proto = scope.root_nanbox_f64(proto);
    let owner = scope.root_nanbox_f64(owner);
    for member in decl
        .members()
        .iter()
        .filter(|m| m.name.is_null() && m.is_method() && !m.entry.is_null())
    {
        if !crate::closure::info_has(
            &*(member.entry as *const crate::closure::JsFunctionInfo),
            crate::closure::FN_GENERATOR,
        ) {
            continue;
        }
        let Some(key) = crate::object::class_registry::declarations::evaluation_computed_member_key(
            crate::value::js_nanbox_get_pointer(owner.get_nanbox_f64()) as *const ObjectHeader,
            class_id,
            member,
        ) else {
            continue;
        };
        if crate::symbol::js_is_symbol(key) == 0 {
            continue;
        }
        let key = scope.root_nanbox_f64(key);
        let name = crate::symbol::symbol_function_name(crate::symbol::sym_key_from_f64(
            key.get_nanbox_f64(),
        ));
        let method = scope.root_nanbox_f64(
            crate::object::native_module::class_method_declaration_value(
                owner.get_nanbox_f64(),
                &name,
                member.code as usize,
                member.param_count,
                member.has_synthetic_arguments(),
                member.has_rest(),
                member.entry as usize,
                true,
            ),
        );
        crate::object::native_module::set_bound_native_closure_name(
            crate::value::js_nanbox_get_pointer(method.get_nanbox_f64())
                as *mut crate::closure::ClosureHeader,
            &name,
        );
        crate::symbol::define_symbol_data_property(
            proto.get_nanbox_f64(),
            key.get_nanbox_f64(),
            method.get_nanbox_f64(),
        );
        crate::symbol::set_symbol_property_attrs(
            crate::value::js_nanbox_get_pointer(proto.get_nanbox_f64()) as usize,
            crate::symbol::sym_key_from_f64(key.get_nanbox_f64()),
            crate::object::PropertyAttrs::new(true, false, true),
        );
    }
}

/// A computed static symbol method is an ordinary own property of its class.
/// Retain its actual callable and home, as named class methods already do.
pub(crate) unsafe fn install_class_symbol_method(
    class_id: u32,
    owner: f64,
    key: f64,
    body: usize,
    params: u32,
    rest: bool,
    entry: usize,
) {
    let scope = crate::gc::RuntimeHandleScope::new();
    let key = scope.root_nanbox_f64(key);
    let owner = if crate::object::value_is_object_like(owner) {
        owner
    } else {
        crate::object::class_value::class_value(class_id)
    };
    let owner = scope.root_nanbox_f64(owner);
    let name =
        crate::symbol::symbol_function_name(crate::symbol::sym_key_from_f64(key.get_nanbox_f64()));
    // Use the same declaration value as named/computed-string methods:
    // its home and ordinary method metadata belong to this callable.
    let method = crate::object::native_module::class_method_declaration_value(
        owner.get_nanbox_f64(),
        name.as_bytes(),
        body,
        params,
        false,
        rest,
        entry,
        true,
    );
    let method = scope.root_nanbox_f64(method);
    // A static Symbol-keyed method is a class static symbol member: arm the
    // latch that instanceof's Symbol.hasInstance lookup and the other
    // static-symbol readers gate on, or `static [Symbol.hasInstance]` is never
    // consulted.
    crate::symbol::CLASS_STATIC_SYMBOLS_LATCH.arm();
    crate::symbol::define_symbol_data_property(
        owner.get_nanbox_f64(),
        key.get_nanbox_f64(),
        method.get_nanbox_f64(),
    );
    crate::symbol::set_symbol_property_attrs(
        crate::value::js_nanbox_get_pointer(owner.get_nanbox_f64()) as usize,
        crate::symbol::sym_key_from_f64(key.get_nanbox_f64()),
        crate::object::PropertyAttrs::new(true, false, true),
    );
}

/// The source entry selects generator lowering, while Get and the ordinary
/// callable invocation prove the selected method and its receiver per use.
pub(crate) unsafe fn call_generator_symbol_method(
    object: f64,
    key: f64,
    args: *const f64,
    len: usize,
) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let object = scope.root_nanbox_f64(object);
    let key = scope.root_nanbox_f64(key);
    let args = scope.root_nanbox_f64_slice(if len == 0 {
        &[]
    } else {
        std::slice::from_raw_parts(args, len)
    });
    let recv = if let Some(cid) = crate::object::class_prototype_ref_id(object.get_nanbox_f64()) {
        crate::object::class_registry::class_decl_prototype_value(cid)
    } else if let Some(cid) =
        crate::object::class_value::legacy_class_value_word(object.get_nanbox_u64())
    {
        crate::value::js_nanbox_pointer(crate::object::class_value::class_value_ptr(cid) as i64)
    } else {
        object.get_nanbox_f64()
    };
    let recv = scope.root_nanbox_f64(recv);
    let value =
        crate::symbol::js_object_get_symbol_property(recv.get_nanbox_f64(), key.get_nanbox_f64());
    let args = crate::gc::RuntimeHandleScope::refreshed_nanbox_f64_slice(&args);
    crate::object::method_call::js_method_call_value(
        value,
        recv.get_nanbox_f64(),
        args.as_ptr(),
        args.len(),
    )
}

/// The instance's `[[GeneratorState]]`: its private slot holding the resume
/// record. Every sync generator is born with it at slot 0
/// ([`birth_shape`]); own-key transitions keep it there and carry the brand.
pub(super) static STATE_READ: crate::object::IntrinsicPrivateReadSite =
    crate::object::IntrinsicPrivateReadSite::new(
        &crate::object::runtime_layouts::intrinsic_private_atoms::GENERATOR_STATE,
    );

/// A generator function's original prototype, kept in its bag's private slot
/// from the bag's birth (`closure::props`): `g.prototype` may be reassigned
/// later, and an instance born while the original was unmaterialized names it
/// through its owner.
pub(crate) static ORIGINAL_PROTOTYPE_READ: crate::object::IntrinsicPrivateReadSite =
    crate::object::IntrinsicPrivateReadSite::new(
        &crate::object::runtime_layouts::intrinsic_private_atoms::GENERATOR_PROTOTYPE,
    );

crate::perry_thread_local! {
    /// This agent's sync generator birth ShapeId ([`birth_shape`]): an
    /// immutable id, held for the agent's life like RegExp's birth shape.
    static BIRTH_SHAPE: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

/// The spelling of the bag's `[[GeneratorPrototype]]` private key, which a
/// generator function's bag is born with (`closure::props`).
pub(crate) fn generator_original_prototype_key() -> &'static str {
    // SAFETY: the static atom's image is ASCII.
    unsafe { std::str::from_utf8_unchecked(ORIGINAL_PROTOTYPE_READ.spelling()) }
}

/// One layout for every sync generator: no own keys, the `[[GeneratorState]]`
/// private slot, the generator brand, extensible, and a [[Prototype]] its
/// record holds (`PROTO_ID_RECORD_HELD`): the shape names no prototype, so
/// a birth neither hashes a prototype identity nor mints a shape.
fn birth_shape() -> LayoutWord {
    let shape = BIRTH_SHAPE.with(std::cell::Cell::get);
    if shape != 0 {
        return LayoutWord(shape);
    }
    prepare_birth_shape()
}

#[cold]
#[inline(never)]
fn prepare_birth_shape() -> LayoutWord {
    let shape = record_birth_shape(GENERATOR_STATE_BRAND);
    BIRTH_SHAPE.with(|memo| memo.set(shape.0));
    shape
}

/// The record-held birth layout of a generator kind (`brand`): the
/// `[[GeneratorState]]` private slot, the brand, extensible, prototype held
/// by the record. Published once per agent per kind and kept for its life.
#[cold]
#[inline(never)]
pub(super) fn record_birth_shape(brand: u64) -> LayoutWord {
    use crate::object::canonical_keys::{CanonicalKeys, SharedLayout};
    let _no_move = crate::gc::GcSuppressScope::new();
    let proof = SharedLayout::shape_cache_entry();
    let keys = unsafe {
        crate::object::canonical_keys::extend_key_with_entry(
            &proof,
            CanonicalKeys::EMPTY,
            STATE_READ.birth_key(),
            crate::object::key_attrs::PRIVATE_FIELD_ENTRY,
        )
    };
    let list = keys.as_ptr();
    let summary = unsafe { crate::object::key_attrs::keys_summary(list, 1) };
    let shape = crate::object::shapes::publish_shape_result(
        crate::object::shapes::shape_descriptor_intern_with_rep(
            list,
            1,
            1,
            0,
            crate::object::shapes::ShapeObjectKind::Ordinary,
            0,
            crate::object::shapes::PROTO_ID_RECORD_HELD,
            summary,
            crate::object::field_rep::REP_ANY,
            &[brand],
            None,
            0, // newborn generator is extensible
        ),
    );
    // Held for the agent's life with no receiver in between: an external
    // carrier, so a trace that finds no live generator cannot retire it.
    // SAFETY: the record of the id just published in this agent.
    unsafe {
        crate::object::shapes::note_external_shape_carrier(
            crate::object::shapes::shape_descriptor_for_word(shape),
        );
    }
    shape
}

/// The fallback layout for a prototype the record cannot hold unambiguously
/// (a function, or a non-heap object such as a Proxy): the shape names it, as
/// every ordinary object's shape does. Rare; one intern per birth.
fn shape_held_birth_shape(proto: f64) -> LayoutWord {
    shape_held_birth_shape_for(proto, GENERATOR_STATE_BRAND)
}

/// [`shape_held_birth_shape`] for a generator kind's `brand`.
pub(super) fn shape_held_birth_shape_for(proto: f64, brand: u64) -> LayoutWord {
    use crate::object::canonical_keys::{CanonicalKeys, SharedLayout};
    let scope = crate::gc::RuntimeHandleScope::new();
    let proto = scope.root_nanbox_f64(proto);
    let proto_id = unsafe { prototype_identity(proto.get_nanbox_f64()) }
        .unwrap_or_else(crate::object::shapes::fresh_unique_proto_id);
    // Serial descriptors snapshot their link at mint time. Publish that link
    // first so the born shape itself proves the inherited protocol chain.
    if crate::object::shapes::identity_prototype_word(proto_id) != proto.get_nanbox_u64() {
        crate::object::shapes::write_identity_word(proto_id, proto.get_nanbox_u64());
    }
    let _no_move = crate::gc::GcSuppressScope::new();
    let proof = SharedLayout::shape_cache_entry();
    let keys = unsafe {
        crate::object::canonical_keys::extend_key_with_entry(
            &proof,
            CanonicalKeys::EMPTY,
            STATE_READ.birth_key(),
            crate::object::key_attrs::PRIVATE_FIELD_ENTRY,
        )
    };
    let list = keys.as_ptr();
    let summary = unsafe { crate::object::key_attrs::keys_summary(list, 1) };
    let shape = crate::object::shapes::publish_shape_result(
        crate::object::shapes::shape_descriptor_intern_with_rep(
            list,
            1,
            1,
            0,
            crate::object::shapes::ShapeObjectKind::Ordinary,
            0,
            proto_id,
            summary,
            crate::object::field_rep::REP_ANY,
            &[brand],
            None,
            0,
        ),
    );
    crate::gc::runtime_shade_external_edge(proto.get_nanbox_u64());
    shape
}

pub(crate) fn fixed_generator_state(value: f64) -> Option<*const crate::closure::ClosureHeader> {
    let value = JSValue::from_bits(value.to_bits());
    if !value.is_pointer() {
        return None;
    }
    let addr = value.as_pointer::<u8>() as usize;
    unsafe {
        let header = crate::value::addr_class::try_read_gc_header(addr)?;
        if header.obj_type != crate::gc::GC_TYPE_OBJECT {
            return None;
        }
        fixed_generator_state_of_object(addr as *const ObjectHeader)
    }
}

/// [`fixed_generator_state`] of a proven live `ObjectHeader`. On the birth
/// layout the record is slot 0; a transitioned instance keeps the brand and
/// its private slot. Allocation-free.
///
/// # Safety
/// `obj` is a live `GC_TYPE_OBJECT` cell.
#[inline(always)]
pub(crate) unsafe fn fixed_generator_state_of_object(
    obj: *const ObjectHeader,
) -> Option<*const crate::closure::ClosureHeader> {
    let shape = BIRTH_SHAPE.with(std::cell::Cell::get);
    let bits = if shape != 0 && crate::object::shapes::object_shape_stamp(obj) == LayoutWord(shape)
    {
        *(obj as *const u8)
            .add(std::mem::size_of::<ObjectHeader>())
            .cast::<u64>()
    } else {
        transitioned_generator_state_bits(obj)?
    };
    let state = JSValue::from_bits(bits);
    state
        .is_pointer()
        .then(|| state.as_pointer::<crate::closure::ClosureHeader>() as *const _)
}

/// The `[[GeneratorState]]` of an instance off its birth layout: own keys or
/// integrity transitions keep the brand and the private slot.
#[cold]
#[inline(never)]
unsafe fn transitioned_generator_state_bits(obj: *const ObjectHeader) -> Option<u64> {
    if !crate::object::shapes::object_has_brand(obj, GENERATOR_STATE_BRAND) {
        return None;
    }
    Some(
        STATE_READ
            .read(crate::value::js_nanbox_pointer(obj as i64))?
            .to_bits(),
    )
}

/// The record's sixth compiler-owned control: the instance's [[Prototype]]
/// holder. The birth writes it; the body never reads it. It holds the
/// prototype object, or the owner generator function while the function's
/// original prototype is unmaterialized (a closure is never held as an
/// explicit prototype: such a birth takes [`shape_held_birth_shape`]).
pub(super) const PROTO_SLOT: usize = 5;

/// The [[Prototype]] bits of an instance whose shape's identity is
/// `PROTO_ID_RECORD_HELD`, read from its record. Allocation-free, for every
/// prototype walk: an original prototype not yet materialized is an empty
/// object nothing could have reached, whose own [[Prototype]] is
/// `%GeneratorPrototype%`, so a walk continues there. A reader that hands the
/// prototype itself to the program materializes it first
/// ([`generator_observe_record_prototype`]).
///
/// # Safety
/// `obj` is a live `GC_TYPE_OBJECT` cell.
#[inline(always)]
pub(crate) unsafe fn generator_record_prototype_word(obj: *const ObjectHeader) -> u64 {
    // A sync birth layout answers from its own kind alone; every other
    // record-held instance (async, or transitioned) resolves its kind first.
    let sync_shape = BIRTH_SHAPE.with(std::cell::Cell::get);
    if sync_shape != 0 && crate::object::shapes::object_shape_stamp(obj) == LayoutWord(sync_shape) {
        return record_prototype_word::<false>(birth_record_bits(obj));
    }
    other_record_prototype_word(obj)
}

/// [`generator_record_prototype_word`] for an async or transitioned instance.
#[inline(never)]
unsafe fn other_record_prototype_word(obj: *const ObjectHeader) -> u64 {
    let Some((state, is_async)) = record_of_record_held(obj) else {
        debug_assert!(false, "a record-held shape names a generator instance");
        return crate::value::TAG_NULL;
    };
    let record = crate::value::js_nanbox_pointer(state as i64).to_bits();
    if is_async {
        record_prototype_word::<true>(record)
    } else {
        record_prototype_word::<false>(record)
    }
}

/// The [[Prototype]] bits a `record` (its NaN-boxed bits) of kind `ASYNC`
/// holds in its prototype control.
#[inline(always)]
unsafe fn record_prototype_word<const ASYNC: bool>(record: u64) -> u64 {
    let record = JSValue::from_bits(record);
    if !record.is_pointer() {
        debug_assert!(false, "a record-held shape names a generator instance");
        return crate::value::TAG_NULL;
    }
    let state = record.as_pointer::<crate::closure::ClosureHeader>();
    let holder = *state.add(1).cast::<u64>().add(PROTO_SLOT);
    owner_original_prototype_word::<ASYNC>(holder).unwrap_or(holder)
}

/// The record of a sync or async generator instance, with its kind. Either
/// kind's birth layout answers from its shape stamp alone; only a
/// transitioned instance consults the brands.
///
/// # Safety
/// `obj` is a live `GC_TYPE_OBJECT` cell.
#[inline(always)]
unsafe fn record_of_record_held(
    obj: *const ObjectHeader,
) -> Option<(*const crate::closure::ClosureHeader, bool)> {
    let stamp = crate::object::shapes::object_shape_stamp(obj);
    let sync_shape = BIRTH_SHAPE.with(std::cell::Cell::get);
    let (bits, is_async) = if sync_shape != 0 && stamp == LayoutWord(sync_shape) {
        (birth_record_bits(obj), false)
    } else {
        async_or_transitioned_record_bits(obj, stamp)?
    };
    let record = JSValue::from_bits(bits);
    record.is_pointer().then(|| {
        (
            record.as_pointer::<crate::closure::ClosureHeader>() as *const _,
            is_async,
        )
    })
}

/// Slot 0 of a birth layout: the `[[GeneratorState]]` private slot.
///
/// # Safety
/// `obj` is a live object on a sync or async generator birth layout.
#[inline(always)]
unsafe fn birth_record_bits(obj: *const ObjectHeader) -> u64 {
    *(obj as *const u8)
        .add(std::mem::size_of::<ObjectHeader>())
        .cast::<u64>()
}

/// [`record_of_record_held`] off the sync birth layout: the async birth
/// layout's stamp, else the brands. Out of line, so a sync instance's
/// prototype walk reads only its own kind's layout.
#[inline(never)]
unsafe fn async_or_transitioned_record_bits(
    obj: *const ObjectHeader,
    stamp: LayoutWord,
) -> Option<(u64, bool)> {
    let async_shape = super::async_generator_state::published_birth_shape();
    if async_shape != LayoutWord(0) && stamp == async_shape {
        return Some((birth_record_bits(obj), true));
    }
    transitioned_record_held_bits(obj)
}

/// [`record_of_record_held`] off both birth layouts: the brand names the kind.
#[cold]
#[inline(never)]
unsafe fn transitioned_record_held_bits(obj: *const ObjectHeader) -> Option<(u64, bool)> {
    if let Some(bits) = transitioned_generator_state_bits(obj) {
        return Some((bits, false));
    }
    super::async_generator_state::transitioned_record_bits(obj).map(|bits| (bits, true))
}

/// The prototype a control holding the owner function names: the owner's
/// original, kept in its bag, or `%GeneratorPrototype%`'s while the original
/// is unmaterialized. `None` for a control holding the prototype itself.
#[inline(never)]
unsafe fn owner_original_prototype_word<const ASYNC: bool>(holder: u64) -> Option<u64> {
    let owner = holder_owner(holder)?;
    let bag = (*owner).props;
    if !bag.is_null() {
        if let Some(original) =
            ORIGINAL_PROTOTYPE_READ.read(crate::value::js_nanbox_pointer(bag as i64))
        {
            return Some(original.to_bits());
        }
    }
    let gen_proto = if ASYNC {
        crate::object::ASYNC_GENERATOR_PROTOTYPE_PTR.load(std::sync::atomic::Ordering::Acquire)
    } else {
        crate::object::GENERATOR_PROTOTYPE_PTR.load(std::sync::atomic::Ordering::Acquire)
    };
    debug_assert_ne!(gen_proto, 0, "a birth built the generator intrinsics");
    Some(crate::value::js_nanbox_pointer(gen_proto).to_bits())
}

/// The owner function a prototype control names, when it holds one.
#[inline]
unsafe fn holder_owner(holder: u64) -> Option<*const crate::closure::ClosureHeader> {
    let value = JSValue::from_bits(holder);
    if !value.is_pointer() {
        return None;
    }
    let addr = value.as_pointer::<u8>() as usize;
    let header = crate::value::addr_class::try_read_gc_header(addr)?;
    (header.obj_type == crate::gc::GC_TYPE_CLOSURE).then_some(addr as *const _)
}

/// Before the program observes a record-held instance's [[Prototype]]
/// itself (`Object.getPrototypeOf`, `Reflect.getPrototypeOf`, `__proto__`,
/// a `setPrototypeOf` SameValue test), materialize its owner's original
/// prototype, so the identity it reads is the one every later read and every
/// sibling instance agrees on. Allocates; a no-op for every other value.
pub(crate) fn generator_observe_record_prototype(value: f64) {
    let bits = value.to_bits();
    let jv = JSValue::from_bits(bits);
    if !jv.is_pointer() {
        return;
    }
    let addr = jv.as_pointer::<u8>() as usize;
    let owner = unsafe {
        let Some(header) = crate::value::addr_class::try_read_gc_header(addr) else {
            return;
        };
        if header.obj_type != crate::gc::GC_TYPE_OBJECT
            || header.gc_flags & crate::gc::GC_FLAG_FORWARDED != 0
        {
            return;
        }
        let obj = addr as *const ObjectHeader;
        if crate::object::shapes::shape_proto_id(crate::object::shapes::object_shape_stamp(obj))
            != Some(crate::object::shapes::PROTO_ID_RECORD_HELD)
        {
            return;
        }
        let Some((state, _)) = record_of_record_held(obj) else {
            return;
        };
        let Some(owner) = holder_owner(*state.add(1).cast::<u64>().add(PROTO_SLOT)) else {
            return;
        };
        if !(*owner).props.is_null() {
            return;
        }
        owner
    };
    // The bag's birth declares the slot and keeps the original.
    let _ = generator_function_prototype_of(owner as usize);
}

/// The record's fifth compiler-owned control: how the body's last activation
/// ended. The body writes it (a pointer-free number) right before it returns;
/// the boundary reads it back. Controls 0..=3 are state, done, sent and
/// executing.
const STEP_SLOT: usize = 4;
/// The returned value is a yielded value: `{ value, done: false }`.
pub(crate) const STEP_YIELD: u32 = 0;
/// The returned value is the completion value: `{ value, done: true }`.
pub(crate) const STEP_DONE: u32 = 1;
/// The returned value is a `yield*` delegate's inner result object, re-yielded
/// as-is (spec GeneratorYield(innerResult)).
pub(crate) const STEP_INNER_RESULT: u32 = 2;

/// One resumed step of a fixed-state record, before any IteratorResult exists.
/// A direct consumer (for-of, sync `yield*`) reads the pair; public `next()`,
/// `return()` and `throw()` materialize it with [`FixedStep::into_result`].
#[derive(Clone, Copy)]
pub(crate) struct FixedStep {
    pub value: f64,
    pub kind: u32,
}

impl FixedStep {
    /// CreateIterResultObject for a value step; an inner result is already
    /// the object to return.
    pub(crate) fn into_result(self) -> f64 {
        match self.kind {
            STEP_INNER_RESULT => self.value,
            kind => unsafe {
                crate::iter_result::make_iter_result(
                    JSValue::from_bits(self.value.to_bits()),
                    kind == STEP_DONE,
                )
            },
        }
    }
}

/// The kind of the record's last finished step, as its body recorded it.
pub(crate) unsafe fn fixed_step_kind(state: *const crate::closure::ClosureHeader) -> u32 {
    f64::from_bits(*state.add(1).cast::<u64>().add(STEP_SLOT)) as u32
}

/// Public protocol entry: the step as the IteratorResult object it denotes.
#[inline(never)]
pub(super) unsafe fn resume_fixed_state(
    state: *const crate::closure::ClosureHeader,
    op: f64,
    arg: f64,
) -> f64 {
    resume_fixed_step(state, op, arg).into_result()
}

/// The generator birth owns an inaccessible, two-parameter compiled resume
/// body (one argument for bodies with no abrupt routes). Once the receiver's brand proves that edge, no general callable,
/// rest-argument or arity dispatch remains to resolve. The body returns the
/// raw step value and records its kind in the record's step control.
#[inline(never)]
pub(crate) unsafe fn resume_fixed_step(
    state: *const crate::closure::ClosureHeader,
    op: f64,
    arg: f64,
) -> FixedStep {
    let scope = crate::gc::RuntimeHandleScope::new();
    let state = scope.root_raw_const_ptr(state);
    let get = |index| {
        state.with_const_ptr::<crate::closure::ClosureHeader, _>(|p| {
            *p.add(1).cast::<u64>().add(index)
        })
    };
    let set = |index, bits| {
        state.with_const_ptr::<crate::closure::ClosureHeader, _>(|p| {
            // GC_STORE_AUDIT(POINTER_FREE): shared done/executing/step controls only.
            p.add(1).cast::<u64>().cast_mut().add(index).write(bits);
        })
    };
    if get(3) == crate::value::TAG_TRUE {
        let msg = b"Generator is already executing";
        let msg = crate::string::js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
        let err = crate::error::js_typeerror_new(msg);
        crate::exception::js_throw(crate::value::js_nanbox_pointer(err as i64));
    }
    let next_only =
        state.with_const_ptr::<crate::closure::ClosureHeader, _>(|p| (*(*p).info).params == 1);
    if get(1) == crate::value::TAG_TRUE || (op != 0.0 && (next_only || get(0) == 0.0f64.to_bits()))
    {
        set(1, crate::value::TAG_TRUE);
        if op == 2.0 {
            crate::exception::js_throw(arg);
        }
        set(STEP_SLOT, (STEP_DONE as f64).to_bits());
        let value = if op == 1.0 {
            arg
        } else {
            f64::from_bits(crate::value::TAG_UNDEFINED)
        };
        return FixedStep {
            value,
            kind: STEP_DONE,
        };
    }
    set(3, crate::value::TAG_TRUE);
    if op == 0.0 {
        state.with_const_ptr::<crate::closure::ClosureHeader, _>(|p| {
            js_generator_set_slot(p.cast_mut(), 2, arg.to_bits(), 1)
        });
    }
    // next consumes the sent slot from this refreshed frame at body entry,
    // including after barrier work. Abrupt arguments cross no slot barrier;
    // their remaining setup is the non-collecting exception-stack capture.
    // The argument is never read here after the body can collect.
    let result = crate::exception::catch_generator_resume(&state, op, arg);
    set(3, crate::value::TAG_FALSE);
    match result {
        Ok(value) => {
            // The record can keep this value in a slot its body appends to
            // in place. Handing it out aliases it, exactly as storing it into
            // an IteratorResult member did: demote a uniquely owned string.
            if value.to_bits() & crate::value::TAG_MASK == crate::value::STRING_TAG {
                crate::string::js_string_addref(
                    (value.to_bits() & crate::value::POINTER_MASK) as *mut crate::StringHeader,
                );
            }
            let kind = f64::from_bits(get(STEP_SLOT)) as u32;
            debug_assert!(matches!(kind, STEP_YIELD | STEP_DONE | STEP_INNER_RESULT));
            FixedStep { value, kind }
        }
        Err(value) => {
            set(1, crate::value::TAG_TRUE);
            crate::exception::js_throw(value)
        }
    }
}

/// A previously marked prototype already owns its stable serial. Read those
/// existing metadata facts before asking the general allocating mark path.
unsafe fn prototype_identity(value: f64) -> Option<u64> {
    let addr = (value.to_bits() & crate::value::POINTER_MASK) as usize;
    if let Some(header) = crate::value::addr_class::try_read_gc_header(addr) {
        if header.obj_type == crate::gc::GC_TYPE_OBJECT {
            let meta = (*(addr as *const ObjectHeader)).meta;
            if !meta.is_null()
                && (*meta).flags & crate::object::OBJECT_META_FLAG_IS_PROTOTYPE != 0
                && (*meta).proto_serial != 0
            {
                return Some((*meta).proto_serial);
            }
        }
    }
    crate::object::proto_validity::mark_object_as_prototype(addr)
}

#[no_mangle]
pub extern "C" fn js_generator_create(
    resume: f64,
    owner: *const crate::closure::ClosureHeader,
) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let resume = scope.root_nanbox_f64(resume);
    // Direct class-method bodies have no incoming closure parameter. The
    // image identity of their private resume body supplies the function's
    // ordinary prototype property, rather than an intermediate per birth.
    let owner = if owner.is_null() {
        let state = crate::value::js_nanbox_get_pointer(resume.get_nanbox_f64())
            as *const crate::closure::ClosureHeader;
        unsafe { crate::closure::js_closure_alloc_singleton((*state).info) }
    } else {
        owner as *mut crate::closure::ClosureHeader
    };
    let owner = scope.root_raw_const_ptr(owner);
    ensure_generator_intrinsics();
    // GetPrototypeFromConstructor, read from the owner's declared slot
    // without materializing it.
    let holder = owner.with_const_ptr::<crate::closure::ClosureHeader, _>(|owner| unsafe {
        birth_prototype_holder(owner, false)
    });
    let holder = match holder {
        Holder::Record(bits) => Holder::Record(bits),
        Holder::Shape(bits) => Holder::Shape(bits),
        Holder::Materialize => {
            // An own slot the shape does not name and the bag lacks: the
            // function's ordinary read materializes it (allocates).
            let proto = owner.with_const_ptr::<crate::closure::ClosureHeader, _>(|owner| {
                generator_function_prototype_of(owner as usize)
            });
            match proto {
                Some(value) => classify_prototype(value, false),
                None => Holder::Record(generator_prototype_bits(false)),
            }
        }
    };
    let (holder_bits, record_held) = match holder {
        Holder::Record(bits) => (scope.root_nanbox_u64(bits), true),
        Holder::Shape(bits) => (scope.root_nanbox_u64(bits), false),
        Holder::Materialize => unreachable!("resolved above"),
    };
    let (obj, rep, shape) = if record_held {
        // The agent-lifetime birth layout: validated by the allocation itself.
        let shape = birth_shape();
        let (born, rep) = crate::object::object_alloc_plain_born(1, shape);
        (scope.root_raw_mut_ptr(born), rep, shape)
    } else {
        // A fresh shape is not retained by its id: allocate first, then
        // intern and publish it with no collection in between.
        let born = crate::object::alloc_basic::object_alloc_unpublished(0, 1);
        let obj = scope.root_raw_mut_ptr(born);
        let shape = shape_held_birth_shape(f64::from_bits(holder_bits.get_nanbox_u64()));
        (obj, None, shape)
    };
    obj.with_mut_ptr::<ObjectHeader, _>(|obj| unsafe {
        if rep.is_none() {
            // The unhurried allocation: publish the final descriptor and its
            // brand before the record edge is stored.
            crate::object::shapes::store_kind::premark_plain_ordinary(obj);
            crate::object::shapes::stamp_object_shape_id_with_carrier_note(obj, shape);
        }
        // The `[[GeneratorState]]` edge is the instance's own slot 0.
        crate::object::store_object_field_slot_layout_deferred(obj, 0, resume.get_nanbox_u64());
    });
    unsafe {
        let state = resume.get_nanbox_u64();
        let state = (state & crate::value::POINTER_MASK) as *mut crate::closure::ClosureHeader;
        debug_assert!(matches!((*(*state).info).params, 1 | 2));
        debug_assert!(
            crate::closure::real_capture_count((*state).capture_count) as usize > PROTO_SLOT
        );
        debug_assert_eq!((*(*state).info).flags & crate::closure::FN_REST_MASK, 0);
        // A fixed generator frame contains JSValues throughout its lifetime.
        // Use the existing tag scanner even when its initial values happen to
        // be scalar. Subsequent writes need only the exact write barrier,
        // rather than per-operation dynamic capture-mask maintenance.
        crate::gc::layout_mark_unknown(state.cast());
        if (*state).capture_count & crate::closure::CAPTURES_THIS_FLAG != 0 {
            // GC_STORE_AUDIT(POINTER_FREE): lexical receiver is never rebound.
            (*state).capture_count |= crate::closure::NO_THIS_REBIND_FLAG;
        }
        // The prototype control: a barriered store into the record.
        js_generator_set_slot(state, PROTO_SLOT as u32, holder_bits.get_nanbox_u64(), 0);
    }
    obj.with_mut_ptr::<ObjectHeader, _>(|obj| crate::value::js_nanbox_pointer(obj as i64))
}

/// Where a birth's [[Prototype]] lives.
pub(super) enum Holder {
    /// In the record's prototype control (the birth layout): an ordinary
    /// heap object, or the owner function while its original is
    /// unmaterialized.
    Record(u64),
    /// In the shape ([`shape_held_birth_shape`]).
    Shape(u64),
    /// Not yet known without the function's ordinary read.
    Materialize,
}

pub(super) fn generator_prototype_bits(is_async: bool) -> u64 {
    crate::value::js_nanbox_pointer(generator_prototype_ptr(is_async) as i64).to_bits()
}

/// GetPrototypeFromConstructor for a `prototype` value: a primitive selects
/// the realm's `%GeneratorPrototype%`; an ordinary heap object is held by the
/// record; any other object (a function, which the control reserves for the
/// owner, or a non-heap object) is named by the shape.
pub(super) fn classify_prototype(value: f64, is_async: bool) -> Holder {
    if !unsafe { crate::object::value_is_object_like(value) } {
        return Holder::Record(generator_prototype_bits(is_async));
    }
    let jv = JSValue::from_bits(value.to_bits());
    let ordinary = jv.is_pointer()
        && unsafe {
            crate::value::addr_class::try_read_gc_header(jv.as_pointer::<u8>() as usize)
                .is_some_and(|h| h.obj_type == crate::gc::GC_TYPE_OBJECT)
        };
    if ordinary {
        Holder::Record(value.to_bits())
    } else {
        Holder::Shape(value.to_bits())
    }
}

/// The owner's `prototype` for a birth, without materializing it: no owner
/// (a direct body) selects `%GeneratorPrototype%`; a function with no bag
/// has its original unmaterialized, so the record holds the function.
///
/// # Safety
/// `owner` is null or a live closure cell.
#[inline(always)]
pub(super) unsafe fn birth_prototype_holder(
    owner: *const crate::closure::ClosureHeader,
    is_async: bool,
) -> Holder {
    if owner.is_null() {
        return Holder::Record(generator_prototype_bits(is_async));
    }
    if crate::closure::props::bag_of(owner as usize).is_null()
        && crate::closure::props::closure_has_generator_body(owner)
    {
        return Holder::Record(crate::value::js_nanbox_pointer(owner as i64).to_bits());
    }
    match crate::closure::shape::closure_own_prototype_by_shape(owner) {
        Some(Some(value)) => classify_prototype(value, is_async),
        _ => match crate::closure::closure_get_own_dynamic_prop(owner as usize, "prototype") {
            Some(value) => classify_prototype(value, is_async),
            None => Holder::Materialize,
        },
    }
}

/// Fixed-frame stores share one barrier path. The birth selected the generic
/// JSValue scanner; no capture-layout update or callable check is needed.
#[no_mangle]
#[inline(never)]
pub unsafe extern "C" fn js_generator_set_slot(
    state: *mut crate::closure::ClosureHeader,
    index: u32,
    bits: u64,
    string_alias: i32,
) {
    // Preserve binding-alias demotion in this shared operation. Fresh string
    // computations and an accumulator's own update retain their append proof.
    if string_alias != 0 && bits & crate::value::TAG_MASK == crate::value::STRING_TAG {
        crate::string::js_string_addref(
            (bits & crate::value::POINTER_MASK) as *mut crate::StringHeader,
        );
    }
    let slot = state.add(1).cast::<u64>().add(index as usize);
    // GC_STORE_AUDIT(BARRIERED): exact fixed-frame destination below.
    slot.write(bits);
    crate::gc::runtime_write_barrier_slot(state as usize, slot as usize, bits);
}

/// Public result construction reuses the runtime's canonical iterator shape.
#[no_mangle]
#[inline(never)]
pub extern "C" fn js_generator_result(value: f64, done: i32) -> f64 {
    unsafe { crate::iter_result::make_iter_result(JSValue::from_bits(value.to_bits()), done != 0) }
}
