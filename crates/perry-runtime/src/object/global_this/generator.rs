use super::*;

/// Distinguishes plain vs async generator closures for the intrinsic-tower
/// lookups.
#[derive(Clone, Copy, PartialEq, Eq)]
enum GeneratorKind {
    Sync,
    Async,
}

/// Classify a `GC_TYPE_CLOSURE` pointer as a (plain | async) generator
/// function, or `None` for any other closure. Async generators register in
/// BOTH the generator and async registries (the lowering carries `is_async &&
/// is_generator`), so async-registry membership disambiguates the two.
fn closure_generator_kind(closure_ptr: usize) -> Option<GeneratorKind> {
    let closure = closure_ptr as *const crate::closure::ClosureHeader;
    let func_ptr = crate::closure::get_valid_func_ptr(closure);
    if func_ptr.is_null() {
        return None;
    }
    // Async generators are registered in BOTH registries (they share the sync
    // generator's `{next,return,throw}` lowering), so check the async-generator
    // registry first — it's the only signal that disambiguates the two.
    if crate::closure::is_registered_async_generator_function(func_ptr) {
        Some(GeneratorKind::Async)
    } else if crate::closure::is_registered_generator_function(func_ptr) {
        Some(GeneratorKind::Sync)
    } else {
        None
    }
}

fn intrinsic_pointer_value(slot: i64) -> Option<f64> {
    if slot != 0 {
        Some(crate::value::js_nanbox_pointer(slot))
    } else {
        None
    }
}

/// `Object.getPrototypeOf(g)` for a generator-function closure `g` →
/// `%Generator%` / `%AsyncGenerator%` (a.k.a. `<Ctor>.prototype`). Returns
/// `None` for non-generator closures so the caller keeps its existing
/// `closure_static_prototype` / null resolution. (#3664)
pub(crate) fn generator_function_proto_of(closure_ptr: usize) -> Option<f64> {
    let kind = closure_generator_kind(closure_ptr)?;
    // The towers are normally built in `populate_global_this_builtins`, but a
    // program that reflects on a generator without ever touching `globalThis`
    // would otherwise see null. Build lazily (idempotent) on first use.
    ensure_generator_intrinsics();
    let slot = match kind {
        GeneratorKind::Sync => crate::object::GENERATOR_INTRINSIC_PROTO_PTR.load(Ordering::Acquire),
        GeneratorKind::Async => {
            crate::object::ASYNC_GENERATOR_INTRINSIC_PROTO_PTR.load(Ordering::Acquire)
        }
    };
    intrinsic_pointer_value(slot)
}

/// `g.constructor` for a generator-function closure `g` → `%GeneratorFunction%`
/// / `%AsyncGeneratorFunction%`. `None` for non-generator closures. (#3664)
pub(crate) fn generator_function_constructor_of(closure_ptr: usize) -> Option<f64> {
    let proto = generator_function_proto_of(closure_ptr)?;
    let proto_ptr = crate::value::js_nanbox_get_pointer(proto) as *const ObjectHeader;
    if proto_ptr.is_null() {
        return None;
    }
    let key =
        crate::string::js_string_from_bytes(b"constructor".as_ptr(), "constructor".len() as u32);
    let value = js_object_get_field_by_name(proto_ptr, key);
    Some(f64::from_bits(value.bits()))
}

/// `g.prototype` for a generator-function closure `g`: a lazily-created object
/// whose `[[Prototype]]` is `%Generator.prototype%` / `%AsyncGenerator.prototype%`,
/// cached as the closure's own `prototype` dynamic-prop so the identity is
/// stable across reads (`g.prototype === g.prototype`). Returns `None` for
/// non-generator closures (their `.prototype` keeps its existing behaviour).
/// A live generator instance's `[[Prototype]]` is set to this object (Phase 3b),
/// completing the spec chain `g() → g.prototype → %Generator.prototype%`. (#3664)
pub(crate) fn generator_function_prototype_of(closure_ptr: usize) -> Option<f64> {
    let kind = closure_generator_kind(closure_ptr)?;
    // A previously-created (or user-assigned) `prototype` wins — preserves
    // identity and lets `g.prototype = X` overrides stick.
    let existing = crate::closure::closure_get_dynamic_prop(closure_ptr, "prototype");
    if existing.to_bits() != crate::value::TAG_UNDEFINED {
        return Some(f64::from_bits(existing.to_bits()));
    }
    ensure_generator_intrinsics();
    // #7577: `js_object_alloc` below can run a copying minor, which moves both
    // `%Generator.prototype%` (an ordinary heap object reached through a
    // GC-rooted atomic the collector rewrites) and, once it exists, `obj`
    // itself. `object_set_static_prototype` allocates too — `object_meta_ensure`
    // mints a meta record — so `obj` was stale again by the time it was
    // NaN-boxed for `closure_set_dynamic_prop`, and the `g.prototype` this
    // returned was a from-space address. So: allocate first, root, and re-read
    // the prototype from its rooted slot rather than caching it across.
    let obj = js_object_alloc(0, 0);
    if obj.is_null() {
        return None;
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let obj_h = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(obj as i64));

    let gen_proto = generator_prototype_ptr(matches!(kind, GeneratorKind::Async));
    if !gen_proto.is_null() {
        let proto_bits = crate::value::js_nanbox_pointer(gen_proto as i64).to_bits();
        super::super::prototype_chain::object_set_static_prototype(
            handle_object_ptr(&obj_h) as usize,
            proto_bits,
        );
    }
    let obj_value = obj_h.get_nanbox_f64();
    crate::closure::closure_set_dynamic_prop(closure_ptr, "prototype", obj_value);
    Some(obj_h.get_nanbox_f64())
}

/// `%Generator.prototype%` / `%AsyncGenerator.prototype%` pointer (the object
/// carrying `next`/`return`/`throw`). Used by Phase 2/3 to wire `g.prototype`'s
/// `[[Prototype]]` and the live generator-object chain. Null until
/// `populate_global_this_builtins` has run. (#3664)
pub(crate) fn generator_prototype_ptr(is_async: bool) -> *mut ObjectHeader {
    ensure_generator_intrinsics();
    let slot = if is_async {
        crate::object::ASYNC_GENERATOR_PROTOTYPE_PTR.load(Ordering::Acquire)
    } else {
        crate::object::GENERATOR_PROTOTYPE_PTR.load(Ordering::Acquire)
    };
    slot as *mut ObjectHeader
}

/// Set a data property on an intrinsic object and record its descriptor attrs
/// for `Object.getOwnPropertyDescriptor` reflection. (#3664)
pub(crate) fn set_intrinsic_data_prop(
    obj: *mut ObjectHeader,
    name: &str,
    value: f64,
    attrs: super::super::PropertyAttrs,
) {
    let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
    js_object_set_field_by_name(obj, key, value);
    super::super::set_builtin_property_attrs(obj as usize, name.to_string(), attrs);
}

/// Set `obj[Symbol.toStringTag] = tag` (the descriptor is the spec default
/// `{ writable:false, enumerable:false, configurable:true }`). (#3664)
pub(crate) fn set_intrinsic_to_string_tag(obj: *mut ObjectHeader, tag: &str) {
    let sym = crate::symbol::well_known_symbol("toStringTag");
    if sym.is_null() {
        return;
    }
    let tag_str = crate::string::js_string_from_bytes(tag.as_ptr(), tag.len() as u32);
    unsafe {
        crate::symbol::js_object_set_symbol_property(
            crate::value::js_nanbox_pointer(obj as i64),
            f64::from_bits(crate::value::JSValue::pointer(sym as *const u8).bits()),
            f64::from_bits(crate::js_nanbox_string(tag_str as i64).to_bits()),
        );
    }
    crate::symbol::set_symbol_property_attrs(
        obj as usize,
        sym as usize,
        super::super::PropertyAttrs::new(false, false, true),
    );
}

/// Build a `TypeError` value for a `%Generator.prototype%` method invoked on a
/// receiver that isn't a generator object (NaN-boxed pointer, not thrown). (#3664)
fn generator_receiver_type_error_value(method: &[u8]) -> f64 {
    let mut msg = b"Generator.prototype.".to_vec();
    msg.extend_from_slice(method);
    msg.extend_from_slice(b" called on incompatible receiver");
    let h = crate::string::js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
    let err = crate::error::js_typeerror_new(h);
    crate::value::js_nanbox_pointer(err as i64)
}

/// Shared body for `%Generator.prototype%`/`%AsyncGenerator.prototype%`'s
/// `next`/`return`/`throw`. These prototype methods exist so test262's
/// brand-check cases (`GeneratorPrototype.next.call(nonGenerator)`) and method-
/// identity reads resolve. The real state machine lives in each generator
/// instance's OWN `next`/`return`/`throw` closures (Perry lowers a generator
/// call to a `{next,return,throw}` object), so for a valid receiver we delegate
/// to the instance's own same-named method. Normal `iter.next()` reads the own
/// property directly and never reaches here, so generator execution is
/// unaffected.
///
/// `is_async` selects the spec's incompatible-receiver behaviour: sync
/// generators throw a `TypeError` synchronously, async generators return a
/// rejected promise (their methods always return promises). (#3664)
fn generator_proto_method(method: &[u8], arg: f64, is_async: bool) -> f64 {
    let bad_receiver = |method: &[u8]| -> f64 {
        let errv = generator_receiver_type_error_value(method);
        if is_async {
            let promise = crate::promise::js_promise_rejected(errv);
            crate::value::js_nanbox_pointer(promise as i64)
        } else {
            crate::exception::js_throw(errv)
        }
    };
    let this = crate::object::js_implicit_this_get();
    let jv = JSValue::from_bits(this.to_bits());
    if !jv.is_pointer() {
        return bad_receiver(method);
    }
    let this_obj = jv.as_pointer::<ObjectHeader>();
    // Reject the prototype singletons themselves: they carry these methods as
    // OWN thunks, so delegating below would re-enter this thunk forever. A real
    // generator instance is never the prototype object.
    if this_obj == generator_prototype_ptr(false) || this_obj == generator_prototype_ptr(true) {
        return bad_receiver(method);
    }
    // Brand-check + delegation use OWN properties only. A generator instance
    // (Perry's `{next,return,throw}` object) owns all three state-machine
    // closures; an object that merely INHERITS them (e.g. `g.prototype`, whose
    // [[Prototype]] is `%Generator.prototype%`) is not a generator — and reading
    // the inherited method would resolve back to this very thunk and recurse.
    let own_method = |name: &[u8]| -> Option<*const crate::closure::ClosureHeader> {
        let v = crate::object::js_object_get_own_field_or_undef(this, name.as_ptr(), name.len());
        let vv = JSValue::from_bits(v.to_bits());
        if vv.is_pointer() && crate::closure::is_closure_ptr(vv.as_pointer::<u8>() as usize) {
            Some(vv.as_pointer::<crate::closure::ClosureHeader>())
        } else {
            None
        }
    };
    if own_method(b"next").is_none()
        || own_method(b"return").is_none()
        || own_method(b"throw").is_none()
    {
        return bad_receiver(method);
    }
    // A sync generator instance also owns `next`/`return`/`throw`, so the
    // structural check above can't tell it from an async generator. The
    // `%AsyncGenerator.prototype%` methods must reject a sync-generator `this`
    // (and vice versa): gate on the async request-queue brand.
    if is_async
        != super::super::async_generator_queue::is_async_generator_instance(
            this_obj as *mut ObjectHeader,
        )
    {
        return bad_receiver(method);
    }
    match own_method(method) {
        Some(own_closure) => crate::closure::js_closure_call1(own_closure, arg),
        None => bad_receiver(method),
    }
}

extern "C" fn generator_proto_next_thunk(
    _c: *const crate::closure::ClosureHeader,
    arg: f64,
) -> f64 {
    generator_proto_method(b"next", arg, false)
}
extern "C" fn generator_proto_return_thunk(
    _c: *const crate::closure::ClosureHeader,
    arg: f64,
) -> f64 {
    generator_proto_method(b"return", arg, false)
}
extern "C" fn generator_proto_throw_thunk(
    _c: *const crate::closure::ClosureHeader,
    arg: f64,
) -> f64 {
    generator_proto_method(b"throw", arg, false)
}
extern "C" fn async_generator_proto_next_thunk(
    _c: *const crate::closure::ClosureHeader,
    arg: f64,
) -> f64 {
    generator_proto_method(b"next", arg, true)
}
extern "C" fn async_generator_proto_return_thunk(
    _c: *const crate::closure::ClosureHeader,
    arg: f64,
) -> f64 {
    generator_proto_method(b"return", arg, true)
}
extern "C" fn async_generator_proto_throw_thunk(
    _c: *const crate::closure::ClosureHeader,
    arg: f64,
) -> f64 {
    generator_proto_method(b"throw", arg, true)
}

/// `%AsyncGenerator.prototype%[Symbol.asyncIterator]()` returns `this` (spec
/// inherits this from `%AsyncIteratorPrototype%`). Without it, `for await` /
/// `GetIterator(obj, async)` over a generator instance can't obtain the async
/// iterator and either throws or silently produces nothing.
extern "C" fn async_generator_proto_async_iterator_thunk(
    _c: *const crate::closure::ClosureHeader,
    _arg: f64,
) -> f64 {
    crate::object::js_implicit_this_get()
}

/// `%Generator.prototype%[Symbol.iterator]()` returns `this` (spec inherits this
/// from `%IteratorPrototype%`). Mirrors the async thunk above; see the install
/// note in `build_generator_tower` for why the sync prototype now carries this.
extern "C" fn generator_proto_iterator_thunk(
    _c: *const crate::closure::ClosureHeader,
    _arg: f64,
) -> f64 {
    crate::object::js_implicit_this_get()
}

/// Install a well-known-symbol-keyed method (returning `this`) on a
/// generator/async-generator prototype, with the spec descriptor shape
/// (`name`/`length` own props, non-enumerable value).
fn install_proto_symbol_self_method(
    proto: *mut ObjectHeader,
    symbol_name: &str,
    display_name: &str,
    func_ptr: *const u8,
) {
    let closure = crate::closure::js_closure_alloc(func_ptr, 0);
    if closure.is_null() {
        return;
    }
    crate::closure::js_register_closure_arity(func_ptr, 0);
    super::super::native_module::set_bound_native_closure_name(closure, display_name);
    super::super::native_module::set_builtin_closure_length(closure as usize, 0);
    let configurable = super::super::PropertyAttrs::new(false, false, true);
    super::super::set_builtin_property_attrs(closure as usize, "name".to_string(), configurable);
    super::super::set_builtin_property_attrs(closure as usize, "length".to_string(), configurable);
    let sym = crate::symbol::well_known_symbol(symbol_name);
    if sym.is_null() {
        return;
    }
    unsafe {
        crate::symbol::js_object_set_symbol_property(
            crate::value::js_nanbox_pointer(proto as i64),
            f64::from_bits(JSValue::pointer(sym as *const u8).bits()),
            crate::value::js_nanbox_pointer(closure as i64),
        );
    }
}

/// Stamp `NO_THIS_REBIND_FLAG` onto the `next`/`return`/`throw` step-closure
/// headers of a generator instance object. These closures capture the generator
/// BODY's `this` lexically (the last capture slot, gated by CAPTURES_THIS_FLAG);
/// the `yield*` delegation desugar calls `next.call(iter, v)`, whose
/// `clone_closure_rebind_this` would otherwise overwrite that captured `this`
/// with the iterator object. Marking the header per-closure (rather than via a
/// global func_ptr table built at module-init) is robust under codegen-units and
/// debug builds, and needs no public-struct change. Only closures that actually
/// carry CAPTURES_THIS_FLAG are stamped — a generator whose body never reads
/// `this` is left untouched.
fn mark_generator_step_closures_no_rebind(obj: f64) {
    use crate::closure::{CAPTURES_THIS_FLAG, NO_THIS_REBIND_FLAG};
    for name in [
        b"next".as_slice(),
        b"return".as_slice(),
        b"throw".as_slice(),
    ] {
        let v = crate::object::js_object_get_own_field_or_undef(obj, name.as_ptr(), name.len());
        let vv = JSValue::from_bits(v.to_bits());
        if !vv.is_pointer() {
            continue;
        }
        let ptr = vv.as_pointer::<u8>() as usize;
        if !crate::closure::is_closure_ptr(ptr) {
            continue;
        }
        let header = ptr as *mut crate::closure::ClosureHeader;
        unsafe {
            let cc = (*header).capture_count;
            // Only meaningful for closures that capture `this`; the rebind path
            // is a no-op for the rest, but skip them anyway to keep the flag's
            // invariant tight.
            if cc & CAPTURES_THIS_FLAG != 0 {
                (*header).capture_count = cc | NO_THIS_REBIND_FLAG;
            }
        }
    }
}

/// #4141: link a freshly-built generator/async-generator instance object into
/// the spec `[[Prototype]]` chain. Perry lowers `gen()` to a `{next,return,
/// throw}` object literal; this interposes a fresh intermediate object (the
/// per-instance stand-in for `g.prototype`) as the instance's `[[Prototype]]`,
/// whose own `[[Prototype]]` is `%Generator.prototype%` /
/// `%AsyncGenerator.prototype%`. The result is the two-hop chain Node exposes:
/// `Object.getPrototypeOf(gen())` → intermediate →
/// `Object.getPrototypeOf(...)` → the brand-checked prototype carrying
/// `next`/`return`/`throw`.
///
/// Returns `obj` unchanged so codegen can use it inline in return position.
/// GC: both links go through `object_set_static_prototype`, whose side-table is
/// traced + pointer-rewritten by the collector (see `prototype_chain.rs`), so
/// the intermediate stays live as long as the instance does and dies with it.
///
/// # Rooting (#7577)
///
/// **This frame owns the only reference to the receiver.** Codegen nulls the
/// statepoint slot immediately before the call:
///
/// ```llvm
/// %r67 = bitcast i64 %r66 to double
/// store ptr addrspace(1) null, ptr %r28              ; the caller's root, dropped
/// %r68 = call double @js_generator_attach_prototype(double %r67, i32 0)
/// ```
///
/// That is the ordinary contract — a runtime helper roots its own arguments —
/// but every call below allocates, so the entry-time address goes stale inside
/// this function and nothing else can rewrite it:
///
/// * `wrap_async_generator_instance` allocates three closures and sets three
///   fields;
/// * `generator_prototype_ptr` lazily builds the entire generator intrinsic
///   tower on its first call;
/// * `js_object_alloc` is an allocation by definition;
/// * `object_set_static_prototype` allocates too — `object_meta_ensure` mints
///   the object's meta record out of the arena.
///
/// Pre-fix, a copying minor in any of those windows produced *two* wrong
/// answers with no diagnostic: the `[[Prototype]]` link was installed on the
/// **pre-move** address, so `Object.getPrototypeOf(gen())` on the live object
/// found nothing; and the function **returned** that pre-move address, so the
/// caller's generator was a dangling pointer into retired from-space. That is
/// the SIGBUS in #7577, and without the instruments it is a wrong answer and
/// exit 0.
///
/// Every pointer below is therefore re-read from a handle after the call that
/// could have moved it, per `docs/src/internals/gc-rooting-invariant.md`.
/// `generator_prototype_ptr` is deliberately called a second time rather than
/// cached: it reads a GC-rooted atomic slot the collector rewrites, so a fresh
/// call is the re-read.
#[no_mangle]
pub extern "C" fn js_generator_attach_prototype(obj: f64, is_async: i32) -> f64 {
    let jv = JSValue::from_bits(obj.to_bits());
    if !jv.is_pointer() {
        return obj;
    }
    if jv.as_pointer::<u8>() as usize == 0 {
        return obj;
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let obj_h = scope.root_nanbox_f64(obj);

    // Pin each step closure's lexical generator-body `this` so `yield*`
    // delegation (`next.call(iter, v)`) can't rebind it to the iterator object.
    mark_generator_step_closures_no_rebind(obj_h.get_nanbox_f64());
    if is_async != 0 {
        let async_target = handle_object_ptr(&obj_h);
        super::super::async_generator_queue::wrap_async_generator_instance(async_target);
    }
    // First call may build the whole tower; only its NULL-ness is used here, so
    // the pointer itself is re-read after the allocation below.
    if generator_prototype_ptr(is_async != 0).is_null() {
        return obj_h.get_nanbox_f64();
    }
    // Intermediate object stands in for `g.prototype`: own `[[Prototype]]` is
    // `%Generator.prototype%`, carries no own methods (the instance inherits
    // `next`/`return`/`throw` from the brand-checked prototype two hops up).
    let intermediate = js_object_alloc(0, 0);
    if intermediate.is_null() {
        return obj_h.get_nanbox_f64();
    }
    let intermediate_h =
        scope.root_nanbox_f64(crate::value::js_nanbox_pointer(intermediate as i64));

    let gen_proto_bits =
        crate::value::js_nanbox_pointer(generator_prototype_ptr(is_async != 0) as i64).to_bits();
    super::super::prototype_chain::object_set_static_prototype(
        handle_object_ptr(&intermediate_h) as usize,
        gen_proto_bits,
    );
    // `object_set_static_prototype` above allocated the intermediate's meta
    // record, so BOTH operands are re-read here.
    let intermediate_bits = intermediate_h.get_nanbox_u64();
    super::super::prototype_chain::object_set_static_prototype(
        handle_object_ptr(&obj_h) as usize,
        intermediate_bits,
    );
    obj_h.get_nanbox_f64()
}

/// The current address of a NaN-boxed object held in `handle`.
///
/// #7577: reading it through this helper rather than binding a `usize` at the
/// top of a function is the point — the pre-collection address is never
/// nameable. See `RuntimeHandle::across_mut`'s docs.
#[inline]
fn handle_object_ptr(handle: &crate::gc::RuntimeHandle<'_>) -> *mut ObjectHeader {
    crate::value::js_nanbox_get_pointer(handle.get_nanbox_f64()) as *mut ObjectHeader
}

/// Link a generator/async-generator instance to the concrete generator
/// function closure's cached `.prototype` object. This is the identity path
/// Node exposes for `Object.getPrototypeOf(g()) === g.prototype`; the
/// fallback `js_generator_attach_prototype` above is used when codegen cannot
/// see the owning closure.
/// # Rooting (#7577)
///
/// Same hazard, same shape as [`js_generator_attach_prototype`]: the caller has
/// already dropped its root by the time this is entered, and both
/// `wrap_async_generator_instance` and `generator_function_prototype_of`
/// allocate — the latter mints a fresh `g.prototype` object on its first call
/// per generator function. Binding `obj_ptr` at entry and using it at the tail
/// linked the prototype onto a pre-move address and returned it.
#[no_mangle]
pub extern "C" fn js_generator_attach_closure_prototype(
    obj: f64,
    closure_ptr: *const crate::closure::ClosureHeader,
) -> f64 {
    let jv = JSValue::from_bits(obj.to_bits());
    if !jv.is_pointer() {
        return obj;
    }
    if jv.as_pointer::<u8>() as usize == 0 {
        return obj;
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let obj_h = scope.root_nanbox_f64(obj);

    // Pin the step closures' lexical generator-body `this` (see the fallback
    // `js_generator_attach_prototype`); this is the closure-identity wiring path.
    mark_generator_step_closures_no_rebind(obj_h.get_nanbox_f64());

    let closure = crate::closure::clean_closure_ptr(closure_ptr);
    if closure.is_null() || crate::closure::get_valid_func_ptr(closure).is_null() {
        return obj_h.get_nanbox_f64();
    }

    // Async-generator instances need the request-queue wrapper installed on
    // their `next`/`return`/`throw` so same-stack follow-up calls queue (spec
    // AsyncGeneratorEnqueue) and `.return(v)` awaits `v`. The non-closure
    // fallback (`js_generator_attach_prototype`) does this when codegen knows
    // the function is async; on the closure-identity path we read the async
    // brand from the function's registration (the `async function*` wrapper
    // symbol is recorded via `js_register_closure_async_generator_function`).
    if crate::closure::is_registered_async_generator_function(crate::closure::get_valid_func_ptr(
        closure,
    )) {
        let async_target = handle_object_ptr(&obj_h);
        super::super::async_generator_queue::wrap_async_generator_instance(async_target);
    }

    // Allocates a fresh `g.prototype` on the first call for this function, so
    // the receiver is re-read from its handle AFTER it, not before.
    let Some(proto) = generator_function_prototype_of(closure as usize) else {
        return obj_h.get_nanbox_f64();
    };
    if !JSValue::from_bits(proto.to_bits()).is_pointer() {
        return obj_h.get_nanbox_f64();
    }

    super::super::prototype_chain::object_set_static_prototype(
        handle_object_ptr(&obj_h) as usize,
        proto.to_bits(),
    );
    obj_h.get_nanbox_f64()
}

/// Build one generator-intrinsic tower (sync or async) and store its three
/// objects in the GC-rooted atomics declared in `object/mod.rs`.
///
/// Spec chain (sync names; async mirrors with the `Async` prefix):
/// ```text
/// %GeneratorFunction%             ctor closure, name "GeneratorFunction", length 1
///   .prototype = %Generator%      (non-writable, non-enumerable, non-configurable)
/// %Generator%  (= %GeneratorFunction.prototype%)
///   .constructor = %GeneratorFunction%      (non-writable, non-enum, configurable)
///   .prototype   = %Generator.prototype%    (non-writable, non-enum, configurable)
///   [Symbol.toStringTag] = "GeneratorFunction"
/// %Generator.prototype%  (= %GeneratorFunction.prototype.prototype%)
///   .constructor = %Generator%              (non-writable, non-enum, configurable)
///   .next / .return / .throw                (Phase 1: noop-backed for descriptor tests)
///   [Symbol.toStringTag] = "Generator"
/// ```
fn build_generator_tower(
    is_async: bool,
    ctor_slot: &crate::object::RealmAtomicI64,
    proto_slot: &crate::object::RealmAtomicI64,
    gen_proto_slot: &crate::object::RealmAtomicI64,
) {
    // #7251: this builds an IMMORTAL object graph (the tower hangs off an
    // agent-local intrinsic root for the life of the thread) by threading
    // `ctor`/`proto`/`gen_proto` as raw `*mut ObjectHeader` / `*mut
    // ClosureHeader` locals across a dozen-plus allocating installs below.
    // None of those locals is a slot the collector rewrites, so a relocating
    // collection reached from one of this function's own allocations would
    // leave the rest of the build writing through from-space addresses —
    // exactly the invariant #7217 fixed for `populate_global_this_builtins`
    // ("a bootstrap that builds an IMMORTAL object graph through raw pointers
    // held across its own allocations must run in a NO-MOVE WINDOW").
    //
    // On the SAFEPOINT route this was unreachable: a back-edge poll only
    // fires while user JS runs, and this function runs none. It IS reachable
    // on the allocation-point route — this tower is lazily built ahead of
    // `populate_global_this_builtins` by `generator_prototype_ptr` /
    // `generator_function_prototype_of`, called from the codegen-emitted
    // `js_generator_attach_prototype` / `js_generator_attach_closure_prototype`
    // helpers on literally the FIRST `gen()` call in a program that has not
    // yet touched `globalThis`. See `gc::tests::lazy_intrinsic_towers` for the
    // gate: it arms a collection at the very next arena-block-crossing
    // allocation (this tower is far smaller than a block, so it cannot arm
    // one on its own) and asserts this function does not service it.
    let _no_move = crate::gc::GcSuppressScope::new();
    let (ctor_name, ctor_tag, inst_tag) = if is_async {
        (
            "AsyncGeneratorFunction",
            "AsyncGeneratorFunction",
            "AsyncGenerator",
        )
    } else {
        ("GeneratorFunction", "GeneratorFunction", "Generator")
    };
    let noop = global_this_builtin_noop_thunk as *const u8;
    let ctor = crate::closure::js_closure_alloc(noop, 0);
    let proto = js_object_alloc(0, 0); // %Generator% / %AsyncGenerator%
    let gen_proto = js_object_alloc(0, 0); // %Generator.prototype%
    if ctor.is_null() || proto.is_null() || gen_proto.is_null() {
        return;
    }
    let non_writable = super::super::PropertyAttrs::new(false, false, false);
    let configurable = super::super::PropertyAttrs::new(false, false, true);

    // --- %GeneratorFunction% constructor ---
    crate::closure::js_register_closure_arity(noop, 1);
    super::super::native_module::set_bound_native_closure_name(ctor, ctor_name);
    super::super::native_module::set_builtin_closure_length(ctor as usize, 1);
    super::super::set_builtin_property_attrs(ctor as usize, "name".to_string(), configurable);
    super::super::set_builtin_property_attrs(ctor as usize, "length".to_string(), configurable);
    set_intrinsic_data_prop(
        ctor as *mut ObjectHeader,
        "prototype",
        crate::value::js_nanbox_pointer(proto as i64),
        non_writable,
    );

    // --- %Generator% (= %GeneratorFunction.prototype%) ---
    set_intrinsic_data_prop(
        proto,
        "constructor",
        crate::value::js_nanbox_pointer(ctor as i64),
        configurable,
    );
    set_intrinsic_data_prop(
        proto,
        "prototype",
        crate::value::js_nanbox_pointer(gen_proto as i64),
        configurable,
    );
    set_intrinsic_to_string_tag(proto, ctor_tag);

    // --- %Generator.prototype% ---
    set_intrinsic_data_prop(
        gen_proto,
        "constructor",
        crate::value::js_nanbox_pointer(proto as i64),
        configurable,
    );
    let (next_thunk, return_thunk, throw_thunk) = if is_async {
        (
            async_generator_proto_next_thunk as *const u8,
            async_generator_proto_return_thunk as *const u8,
            async_generator_proto_throw_thunk as *const u8,
        )
    } else {
        (
            generator_proto_next_thunk as *const u8,
            generator_proto_return_thunk as *const u8,
            generator_proto_throw_thunk as *const u8,
        )
    };
    install_proto_method(gen_proto, "next", next_thunk, 1);
    install_proto_method(gen_proto, "return", return_thunk, 1);
    install_proto_method(gen_proto, "throw", throw_thunk, 1);
    // Spec: `%AsyncGenerator.prototype%` inherits `[Symbol.asyncIterator]` from
    // `%AsyncIteratorPrototype%` and `%Generator.prototype%` inherits
    // `[Symbol.iterator]` from `%IteratorPrototype%` — both returning `this`.
    // Without the async one, `for await (x of gen())` over an async-generator
    // *method instance* can't resolve the async iterator and hangs/yields nothing
    // (the instance carries no own iterator symbol). The async-iterator-
    // acquisition path (`js_get_async_iterator`) sets the implicit-this before
    // invoking this thunk, so it returns the generator instance.
    //
    // The SYNC `%Generator.prototype%` carries `[Symbol.iterator]` for the same
    // reason (#6696): a *computed* read `gen[Symbol.iterator]` walks the
    // prototype chain and must resolve to a callable that returns the generator
    // — Node exposes it via `%IteratorPrototype%`, and esbuild's `__yieldStar`
    // helper (emitted for `yield*` at `--target=es2015|es2017`) does
    // `value[Symbol.iterator]()` on the delegate generator. `for (x of gen())`
    // is unaffected: it drives the generator's own `.next()` directly (the
    // builtin-iterator recognizers), and where the sync iterator-acquisition
    // path (`js_get_iterator`) does read `[Symbol.iterator]`, it binds
    // implicit-this before invoking the method, so the thunk returns the
    // generator instance.
    let (symbol_name, display_name, thunk) = if is_async {
        (
            "asyncIterator",
            "[Symbol.asyncIterator]",
            async_generator_proto_async_iterator_thunk as *const u8,
        )
    } else {
        (
            "iterator",
            "[Symbol.iterator]",
            generator_proto_iterator_thunk as *const u8,
        )
    };
    install_proto_symbol_self_method(gen_proto, symbol_name, display_name, thunk);
    set_intrinsic_to_string_tag(gen_proto, inst_tag);

    ctor_slot.store(ctor as i64, Ordering::Release);
    proto_slot.store(proto as i64, Ordering::Release);
    gen_proto_slot.store(gen_proto as i64, Ordering::Release);
}

/// Build both generator intrinsic towers. Idempotent within the current
/// agent; called during its global bootstrap or by the lazy accessors. (#3664)
pub(crate) fn ensure_generator_intrinsics() {
    if crate::object::GENERATOR_FUNCTION_INTRINSIC_PTR.load(Ordering::Acquire) == 0 {
        build_generator_tower(
            false,
            &crate::object::GENERATOR_FUNCTION_INTRINSIC_PTR,
            &crate::object::GENERATOR_INTRINSIC_PROTO_PTR,
            &crate::object::GENERATOR_PROTOTYPE_PTR,
        );
    }
    if crate::object::ASYNC_GENERATOR_FUNCTION_INTRINSIC_PTR.load(Ordering::Acquire) == 0 {
        build_generator_tower(
            true,
            &crate::object::ASYNC_GENERATOR_FUNCTION_INTRINSIC_PTR,
            &crate::object::ASYNC_GENERATOR_INTRINSIC_PROTO_PTR,
            &crate::object::ASYNC_GENERATOR_PROTOTYPE_PTR,
        );
    }
}
