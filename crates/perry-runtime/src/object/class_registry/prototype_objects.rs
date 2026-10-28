use super::*;
use crate::JSValue;

pub(crate) fn ensure_function_prototype_object(func_value: f64) -> *mut ObjectHeader {
    if !is_callable_function_value(func_value) {
        return std::ptr::null_mut();
    }
    let func_ptr = (func_value.to_bits() & crate::value::POINTER_MASK) as usize;
    if let Some(value) = crate::closure::closure_get_own_dynamic_prop(func_ptr, "prototype") {
        let value = JSValue::from_bits(value.to_bits());
        if value.is_pointer() {
            let ptr = value.as_pointer::<ObjectHeader>() as usize;
            if unsafe { crate::value::addr_class::try_read_gc_header(ptr) }
                .is_some_and(|h| h.obj_type == crate::gc::GC_TYPE_OBJECT)
            {
                return ptr as *mut ObjectHeader;
            }
        }
        return std::ptr::null_mut();
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let func_handle = scope.root_nanbox_f64(func_value);
    let layout = match super::super::nm_ee_ops() {
        Some(ops) => unsafe { (ops.ee_prototype_layout)(func_handle.get_nanbox_f64()) },
        None => std::ptr::null(),
    };
    // `EventEmitter.prototype` is born on its static layout, old and never
    // moving like every builtin prototype (`runtime_layouts::node_prototypes`):
    // its [[Prototype]] is the layout's.
    let on_node_layout = std::ptr::eq(
        layout,
        std::ptr::addr_of!(super::super::runtime_layouts::EVENT_EMITTER_PROTOTYPE),
    );
    let proto = if layout.is_null() {
        crate::object::object_alloc_prototype_old(0, 0)
    } else if on_node_layout {
        unsafe { crate::object::layout_desc::alloc_layout_prototype(layout, 0) }
    } else {
        unsafe { crate::object::layout_desc::perry_alloc_layout(layout) }
    };
    if proto.is_null() {
        return proto;
    }
    let proto_handle = scope.root_raw_mut_ptr(proto);

    if !layout.is_null() {
        proto_handle.with_mut_ptr::<ObjectHeader, _>(|proto| {
            js_object_set_field(proto, 0, JSValue::from_bits(func_handle.get_nanbox_u64()))
        });
    } else {
        let constructor_key = scope.root_string_ptr(crate::string::js_string_from_bytes(
            b"constructor".as_ptr(),
            "constructor".len() as u32,
        ));
        proto_handle.with_mut_ptr::<ObjectHeader, _>(|proto| {
            constructor_key.with_const_ptr::<crate::StringHeader, _>(|key| {
                js_object_set_field_by_name(proto, key, func_handle.get_nanbox_f64())
            })
        });
        proto_handle.with_mut_ptr::<ObjectHeader, _>(|proto| {
            set_builtin_property_attrs(
                proto as usize,
                "constructor".to_string(),
                PropertyAttrs::new(true, false, true),
            )
        });
    }

    if on_node_layout {
        // The layout names `%Object.prototype%`.
    } else if let Some(object_proto_bits) = global_object_prototype_bits() {
        let object_proto = scope.root_nanbox_u64(object_proto_bits);
        proto_handle.with_mut_ptr::<ObjectHeader, _>(|proto| {
            super::super::prototype_chain::object_set_static_prototype(
                proto as usize,
                object_proto.get_nanbox_u64(),
            )
        });
    }

    // #5477: the bound `events.EventEmitter` / `EventEmitterAsyncResource` export's
    // synthetic prototype must carry the EventEmitter methods (`emit`/`on`/`once`/
    // …) so the `Object.setPrototypeOf(x, EventEmitter.prototype)` mixin pattern
    // (pino's logger prototype) gives `x` a working `emit`/`on`. The installed
    // closures read their `this` argument, so a plain object that merely inherits this
    // prototype dispatches against ITSELF (listener state is keyed by the receiver
    // object, not a captured instance). Mirrors what `Stream.prototype` already
    // does. The function owns this prototype, so the install runs once.
    // Routed through the armed ops table (see `nm_namespace_hooks`): bound
    // native-callable closures only exist once `callable_exports` minted one
    // (which arms), so binaries without module imports link neither the
    // probe nor the EventEmitter prototype machinery.
    if let Some(ops) = super::super::nm_ee_ops() {
        proto_handle.with_mut_ptr::<ObjectHeader, _>(|proto| unsafe {
            (ops.ee_prototype_install)(func_handle.get_nanbox_f64(), proto)
        });
    }

    let func_bits = func_handle.get_nanbox_u64();
    if (func_bits >> 48) == 0x7FFD {
        let func_ptr = (func_bits & crate::value::POINTER_MASK) as usize;
        if func_ptr != 0 {
            crate::closure::closure_set_dynamic_prop(
                func_ptr,
                "prototype",
                proto_handle.with_mut_ptr::<ObjectHeader, _>(|proto| {
                    crate::value::js_nanbox_pointer(proto as i64)
                }),
            );
            set_builtin_property_attrs(
                func_ptr,
                "prototype".to_string(),
                PropertyAttrs::new(true, false, false),
            );
        }
    }

    proto_handle.with_mut_ptr::<ObjectHeader, _>(|proto| proto)
}

/// Object.create's synthetic identity range, disjoint from ShapeIds and
/// declared/builtin class ids. Ordinary functions use their owned prototype
/// property and never allocate an entry in the synthetic class tables.
pub(crate) const SYNTHETIC_CLASS_ID_BASE: u32 = 0xC000_0000;
/// Exclusive end of the synthetic range: the first reserved builtin id.
pub(crate) const SYNTHETIC_CLASS_ID_END: u32 = 0xFFFF_0000;

const _: () = assert!(
    SYNTHETIC_CLASS_ID_BASE >= crate::object::shapes::SHAPE_ID_END,
    "rule 2: synthetic class ids must not overlap the ShapeId range"
);
const _: () = assert!(
    SYNTHETIC_CLASS_ID_END > SYNTHETIC_CLASS_ID_BASE,
    "the synthetic class-id range must be non-empty"
);

per_test_global! {
    /// Synthetic class id allocator for prototype-object classes. Allocated
    /// from `[SYNTHETIC_CLASS_ID_BASE, SYNTHETIC_CLASS_ID_END)` so the ids
    /// stay separate from codegen-assigned ids (which start from 1 and grow
    /// by module), from the reserved builtin bands, AND from the ShapeId
    /// range. Mint through [`alloc_synthetic_class_id`], never by a bare
    /// `fetch_add` — the bound is the invariant.
    pub static NEXT_SYNTHETIC_CLASS_ID: std::sync::atomic::AtomicU32 =
        std::sync::atomic::AtomicU32::new(SYNTHETIC_CLASS_ID_BASE);
}

/// Mint the next synthetic class id, parked at the end of the range rather
/// than wrapped.
///
/// A bare `fetch_add` could walk out of the range in two ways, both of which
/// break a rule the read path depends on: past `SYNTHETIC_CLASS_ID_END` it
/// collides with the reserved builtin ids (`0xFFFF_0000..`), and on u32 wrap
/// it lands back in — among others — the ShapeId range. Exhaustion is
/// unreachable in practice (2^30 ids, one per distinct
/// Object.create prototype identity), so
/// saturating is the conservative answer: `0` means "no synthetic id", which
/// every caller already handles as "stays parentless".
#[cfg(test)]
pub(crate) fn alloc_synthetic_class_id() -> u32 {
    use std::sync::atomic::Ordering;
    loop {
        let id = NEXT_SYNTHETIC_CLASS_ID.load(Ordering::Relaxed);
        if id >= SYNTHETIC_CLASS_ID_END {
            NEXT_SYNTHETIC_CLASS_ID.store(SYNTHETIC_CLASS_ID_END, Ordering::Relaxed);
            return 0;
        }
        if NEXT_SYNTHETIC_CLASS_ID
            .compare_exchange_weak(id, id + 1, Ordering::Relaxed, Ordering::Relaxed)
            .is_ok()
        {
            debug_assert!(
                !crate::object::shapes::is_layout_word(id),
                "rule 2: a synthetic class id must never be readable as a ShapeId"
            );
            return id;
        }
    }
}

#[cfg(test)]
pub(crate) fn test_alloc_synthetic_class_id() -> u32 {
    alloc_synthetic_class_id()
}

#[cfg(test)]
#[path = "prototype_objects/parent_class_object_tests.rs"]
mod parent_class_object_tests;

/// The `[[Prototype]]` object recorded for a SYNTHETIC class id — one of the
/// ids `Object.create(proto)` (#809) and `F.prototype = obj` (#711) allocate
/// from [`NEXT_SYNTHETIC_CLASS_ID`]. That link is the authoritative prototype
/// of every instance stamped with the id.
///
/// Unlike [`class_prototype_object`], this refuses a DECLARED class id, whose
/// entry in the same table is the parent CLASS OBJECT of a class-expression
/// subclass (#1788/#6552) rather than a prototype.
pub(crate) fn synthetic_class_prototype_object(class_id: u32) -> *mut ObjectHeader {
    if class_id < SYNTHETIC_CLASS_ID_BASE
        || class_id >= NEXT_SYNTHETIC_CLASS_ID.load(std::sync::atomic::Ordering::Relaxed)
    {
        return std::ptr::null_mut();
    }
    class_prototype_object(class_id)
}

/// `proto_obj` (the [`class_prototype_object`] entry for `class_id`) when it
/// is the parent CLASS OBJECT of a class-expression subclass (#1788/#6552)
/// rather than a prototype. A synthetic id's entry is a real prototype even
/// when that prototype is itself a class object (`Object.create(C)`).
fn declared_parent_class_object(
    class_id: u32,
    proto_obj: *mut ObjectHeader,
) -> Option<*mut ObjectHeader> {
    if proto_obj.is_null()
        || (SYNTHETIC_CLASS_ID_BASE..SYNTHETIC_CLASS_ID_END).contains(&class_id)
        || !is_class_object_ptr(proto_obj as *const u8)
    {
        return None;
    }
    Some(proto_obj)
}

/// Perform ordinary `.prototype` assignment, then synchronize the synthetic
/// class metadata used when a class extends a function (#711, #9365).
#[no_mangle]
pub extern "C" fn js_set_prototype_property(receiver: f64, value: f64, strict: i32) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let receiver = scope.root_nanbox_f64(receiver);
    let value = scope.root_nanbox_f64(value);
    let key = scope.root_string_ptr(crate::string::js_string_from_bytes(
        b"prototype".as_ptr(),
        9,
    ));
    let key = key
        .with_const_ptr::<crate::StringHeader, _>(|key| crate::value::js_nanbox_string(key as i64));
    crate::proxy::js_put_value_set(
        receiver.get_nanbox_f64(),
        key,
        value.get_nanbox_f64(),
        receiver.get_nanbox_f64(),
        strict,
    );

    value.get_nanbox_f64()
}

/// Legacy ABI: store the function's prototype in its owned property bag.
/// There is no synthetic function class identity; the unused result is zero.
#[no_mangle]
pub extern "C" fn js_set_function_prototype(func: f64, proto: f64) -> u32 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let func = scope.root_nanbox_f64(func);
    let proto = scope.root_nanbox_f64(proto);
    if is_callable_function_value(func.get_nanbox_f64()) {
        let ptr = (func.get_nanbox_u64() & crate::value::POINTER_MASK) as usize;
        crate::closure::closure_set_dynamic_prop(ptr, "prototype", proto.get_nanbox_f64());
        set_builtin_property_attrs(
            ptr,
            "prototype".to_string(),
            PropertyAttrs::new(true, false, false),
        );
    }
    0
}

/// The prototype holder an instance of class `class_id` reads through next:
/// the declared `class X {}` prototype when this agent built it, otherwise the
/// synthetic-class prototype object, otherwise null (not built yet).
///
/// One precedence for every consumer: the read walk
/// (`prototype_chain::class_link_prototype`), the prototype-method
/// registration that writes the holder (`class_prototype_set`), and the
/// typed-feedback direct-call guard that proves the holder unchanged. A
/// writer and a guard that each picked their own registry could disagree
/// about which object holds `C.prototype.m`.
#[inline]
pub(crate) fn class_holder_prototype(class_id: u32) -> *mut ObjectHeader {
    let word = class_holder_word(class_id);
    let value = crate::JSValue::from_bits(word);
    if value.is_pointer() {
        value.as_pointer::<ObjectHeader>() as *mut ObjectHeader
    } else {
        std::ptr::null_mut()
    }
}

/// The CLASS surface fact: zero means absent, undefined means declared but
/// unbuilt, and a pointer names the actual holder.
#[inline]
pub(crate) fn class_holder_word(class_id: u32) -> u64 {
    class_holder_word_selected(decl_prototype_identity_id(class_id))
}

/// Read the identity word for an origin already selected by the caller.
/// Keeping selection outside this helper avoids projecting an alias twice.
#[inline]
pub(crate) fn class_holder_word_selected(selected: u32) -> u64 {
    crate::object::shapes::identity_prototype_word(if selected == 0 {
        crate::object::shapes::PROTO_ID_DEFAULT
    } else {
        crate::object::shapes::PROTO_ID_CLASS | u64::from(selected)
    })
}

/// Lookup helper for the dispatch chain walk: returns the prototype
/// object pointer for a synthetic class id, or null if none.
#[inline]
pub(crate) fn class_prototype_object(class_id: u32) -> *mut ObjectHeader {
    // #7757: a monomorphized specialization shares its GENERIC's prototype
    // object — there is exactly one `Gen.prototype` at runtime, since
    // TypeScript erases the type arguments. This is the second of the two
    // prototype registries (the other is `CLASS_DECL_PROTOTYPE_OBJECTS`); both
    // must redirect or `x.constructor` and `Object.getPrototypeOf` disagree
    // about whether the specialization is `Gen`.
    let class_id = crate::object::class_generic_origin(class_id).unwrap_or(class_id);
    CLASS_PROTOTYPE_OBJECTS.with(|table| {
        if let Ok(read) = table.read() {
            if let Some(map) = read.as_ref() {
                return map.get(&class_id).copied().unwrap_or(0) as *mut ObjectHeader;
            }
        }
        std::ptr::null_mut()
    })
}

/// #711 / #809: resolve `key` by walking the synthetic-class-id prototype
/// chain (`CLASS_PROTOTYPE_OBJECTS`), recursing into each prototype object
/// as a normal field lookup. Used both when a receiver's own keys miss AND
/// when it has no `keys_array` at all (an `Object.create(proto)` result, or
/// a `Function.prototype = obj` instance with no own props). Returns the
/// first defined, non-null field found on the chain.
/// The receiver-less form, whose ONE caller is the CONSTRUCTOR-side read in
/// `js_object_get_field_by_name` (`C.foo` on an INT32 class ref). It therefore
/// refuses a name that is a declared INSTANCE member — see `constructor_side`.
pub(crate) unsafe fn resolve_proto_chain_field(
    class_id: u32,
    key: *const crate::StringHeader,
) -> Option<JSValue> {
    resolve_proto_chain_field_inner(class_id, key, None, true, None)
}

pub(crate) unsafe fn resolve_proto_chain_field_with_receiver(
    class_id: u32,
    key: *const crate::StringHeader,
    receiver: f64,
) -> Option<JSValue> {
    resolve_proto_chain_field_inner(class_id, key, Some(receiver), false, None)
}

/// [`resolve_proto_chain_field_with_receiver`] that also reports, through
/// `read_miss`, a prototype object this walk read IN FULL — the generic getter,
/// which walks that object's own chain — and which answered exactly
/// `undefined`. Its bits are left as they were when nothing qualified.
///
/// #10877: a `new F()` instance reaches `F.prototype` twice, through its
/// synthetic class id (this walk) and through its recorded per-object
/// prototype link (`resolve_inherited_field`), and the object-getter tail asks
/// both. Each is a full read of the same object, which re-enters the tail one
/// level up, so an absent read cost `2^depth` getter entries on a
/// constructor-function chain. The tail compares this to the receiver's
/// recorded prototype (`prototype_override::static_prototype_already_read`)
/// and skips the second read when they are the same object. Only an exact
/// `undefined` qualifies: this walk skips a `null` value, which the per-object
/// read must still return.
pub(crate) unsafe fn resolve_proto_chain_field_noting_miss(
    class_id: u32,
    key: *const crate::StringHeader,
    receiver: f64,
    read_miss: &mut u64,
) -> Option<JSValue> {
    resolve_proto_chain_field_inner(class_id, key, Some(receiver), false, Some(read_miss))
}

unsafe fn inherited_proto_accessor_value(
    proto_obj: *mut ObjectHeader,
    key: *const crate::StringHeader,
    receiver: f64,
) -> Option<JSValue> {
    if key.is_null() || !crate::object::object_has_descriptors(proto_obj as usize) {
        return None;
    }
    let key_ptr = (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
    let key_len = (*key).byte_len as usize;
    let name = std::str::from_utf8(std::slice::from_raw_parts(key_ptr, key_len)).ok()?;
    let acc = get_accessor_descriptor(proto_obj as usize, name)?;
    if acc.get == 0 {
        return Some(JSValue::undefined());
    }
    // Route through `invoke_accessor_getter` rather than a bare closure call
    // with the receiver as `this`. A getter installed via
    // `Object.defineProperty(Class.prototype, name, { get })` is an ORDINARY
    // method closure whose body reads `this` from its captured receiver slot —
    // not from its `this` argument — so merely passing `this` left the getter
    // observing the prototype it lives on instead of the instance (winston's
    // `get transports()` saw the prototype, whose `this._readableState` is
    // undefined → "Cannot convert undefined or null to object").
    // `invoke_accessor_getter` clones the closure with `this` rebound to the
    // real receiver (and applies strict/sloppy coercion), matching the
    // own-accessor read path.
    Some(super::super::field_get_set::invoke_accessor_getter(
        acc.get, receiver,
    ))
}

/// Read the actual prototype objects of a class whose parent is a fresh class
/// evaluation. The template-id walk below follows the parent's shared class
/// registry entry; that entry cannot see writes to this evaluation's
/// `Base.prototype` (such as an Effect tagged error's `name`).
unsafe fn evaluated_parent_instance_field(
    decl_proto: *mut ObjectHeader,
    key: *const crate::StringHeader,
    receiver: f64,
) -> Option<JSValue> {
    if decl_proto.is_null() || key.is_null() {
        return None;
    }
    let mut link = Some(crate::value::js_nanbox_pointer(decl_proto as i64).to_bits());
    for _ in 0..32 {
        let bits = link?;
        if bits == crate::value::TAG_NULL {
            return None;
        }
        let value = f64::from_bits(bits);
        if crate::proxy_hooks::proxy_is_proxy(value) != 0 {
            return super::super::prototype_chain::resolve_inherited_field_from_prototype(
                decl_proto as usize,
                bits,
                key,
            );
        }
        let addr = match bits >> 48 {
            0x7FFD => (bits & crate::value::POINTER_MASK) as usize,
            0 if crate::value::addr_class::is_above_handle_band(bits as usize) => bits as usize,
            _ => return None,
        };
        let Some(header) = crate::value::addr_class::try_read_gc_header(addr) else {
            return None;
        };
        if header.obj_type != crate::gc::GC_TYPE_OBJECT {
            return None;
        }
        let proto = addr as *mut ObjectHeader;
        if let Some(value) = inherited_proto_accessor_value(proto, key, receiver) {
            return Some(value);
        }
        if let Some(value) = super::super::field_get_set::own_data_field_by_name(proto, key) {
            return Some(value);
        }
        link = super::super::prototype_chain::object_static_prototype(addr);
    }
    None
}

/// Is `decl_proto` (class `cid`'s declaration prototype) standing on
/// anything but what its declaration links it to — the parent class's
/// declaration prototype, or `Object.prototype` for a base class? Read from
/// its recorded `[[Prototype]]` (a fact of its shape), not from history: a
/// relink back to the declared parent is not a relink. A declaration this
/// cannot name (a runtime-valued or native parent, `extends null`) answers
/// `true`, which only sends the caller down the exact prototype walk.
///
/// # Safety
/// `decl_proto` is class `cid`'s live declaration prototype.
pub(crate) unsafe fn decl_prototype_relinked(cid: u32, decl_proto: *mut ObjectHeader) -> bool {
    let recorded = super::super::prototype_chain::object_static_prototype(decl_proto as usize);
    let declared = match super::get_parent_class_id(cid) {
        Some(parent) if parent != 0 && parent != cid => {
            let parent_proto = class_decl_prototype_object(parent);
            if parent_proto.is_null() {
                return true;
            }
            Some(crate::value::js_nanbox_pointer(parent_proto as i64).to_bits())
        }
        _ => super::global_object_prototype_bits(),
    };
    recorded != declared
}

/// What the rest of a declared prototype's chain answers once a user
/// operation replaced that prototype's `[[Prototype]]`.
enum RelinkedRead {
    /// The declared prototype still stands on its class default; the parent
    /// class id names its next hop.
    NotRelinked,
    Answered(JSValue),
    /// The recorded chain was read in full (or ends in `null`) and lacks the key.
    Missed,
}

/// `C.prototype`'s own properties have been consulted; continue on its
/// RECORDED `[[Prototype]]` with the instance as receiver.
///
/// The class-id walk follows `get_parent_class_id`, which is fixed at
/// declaration. After `Object.setPrototypeOf(C.prototype, X)` that edge is no
/// longer on the chain, and reading the parent class's prototype answered a
/// property `C` instances no longer inherit. The relink is a shape fact of the
/// declared prototype (its ShapeId names the new prototype), so the holder
/// facts a read site validates decline on their own; this is the generic read
/// those sites fall back to and confirm their prime against.
unsafe fn relinked_decl_prototype_field(
    cid: u32,
    decl_proto: *mut ObjectHeader,
    key: *const crate::StringHeader,
    receiver: f64,
) -> RelinkedRead {
    if key.is_null() || !decl_prototype_relinked(cid, decl_proto) {
        return RelinkedRead::NotRelinked;
    }
    // A link recorded as the class default (or never recorded) keeps the
    // parent class id authoritative; only a user relink replaces it.
    let Some(bits) = super::super::prototype_chain::object_static_prototype(decl_proto as usize)
    else {
        return RelinkedRead::NotRelinked;
    };
    if bits == crate::value::TAG_NULL {
        return RelinkedRead::Missed;
    }
    let link = f64::from_bits(bits);
    if crate::proxy_hooks::proxy_is_proxy(link) != 0 {
        let key_val = f64::from_bits(crate::value::js_nanbox_string(key as i64).to_bits());
        let v = crate::proxy_hooks::proxy_get_with_receiver(link, key_val, receiver);
        return RelinkedRead::Answered(JSValue::from_bits(v.to_bits()));
    }
    let addr = match bits >> 48 {
        0x7FFD => (bits & crate::value::POINTER_MASK) as usize,
        0 if crate::value::addr_class::is_above_handle_band(bits as usize) => bits as usize,
        _ => return RelinkedRead::Missed,
    };
    if addr == decl_proto as usize || !super::super::is_valid_obj_ptr(addr as *const u8) {
        return RelinkedRead::Missed;
    }
    // The recursive read re-derives the accessor receiver from the prototype;
    // stash the instance so an inherited getter binds `this` to it.
    let scope = crate::gc::RuntimeHandleScope::new();
    let prev = super::super::field_get_set::accessor_receiver_override_begin(receiver);
    let prev = prev.map(|value| scope.root_nanbox_f64(value));
    let value = js_object_get_field_by_name(addr as *const ObjectHeader, key);
    super::super::field_get_set::accessor_receiver_override_end(
        prev.map(|handle| handle.get_nanbox_f64()),
    );
    if value.is_undefined() {
        RelinkedRead::Missed
    } else {
        RelinkedRead::Answered(value)
    }
}

/// `constructor_side`: this walk serves a read on the class CONSTRUCTOR, so a
/// name that is a declared INSTANCE member must not resolve through it.
///
/// #9404: declared instance methods are MIRRORED onto the reflective
/// `C.prototype` object as own data fields (see the `own_data_field_by_name`
/// arm below). That is right for an instance read, which is what the
/// `_with_receiver` form serves. It is wrong for the CONSTRUCTOR-side read the
/// receiver-less form serves: in JS a class object does not expose its
/// prototype methods as statics — `class C { m(){} }` has `C.m === undefined`,
/// because `m` lives on `C.prototype`. Walking into the decl-prototype from a
/// static lookup made every instance method resolve on the class ref, so
/// `typeof C.m` answered "function", `C.m === C.prototype.m` was true, and a
/// static body's `typeof this.m` answered "function" where node says
/// "undefined" — the surviving half of #9404 after the codegen predicate was
/// made honest. `js_object_has_property` already had this gate (`"m" in C` was
/// correctly false), and the sibling `is_prototype_ref` gate two hundred lines
/// up in `get_field_by_name.rs` plugged the same hole on the direct-vtable
/// door for #1021/NestJS; this is that door's chain-walk twin.
///
/// The exclusion is keyed on `class_instance_has_method` — the exact
/// "is this a prototype method / getter / setter of the chain" predicate —
/// plus the `constructor` back-edge, and NOT on "skip the decl-prototype
/// entirely": a blanket skip would also hide a user's own
/// `Object.defineProperty(C.prototype, ...)` data field from this walk.
///
/// #9467: `constructor` is the one decl-prototype data field that is NOT an
/// instance member by `class_instance_has_method`'s definition, and walking
/// into it from the constructor side answered `C.constructor === C`. Node says
/// `C.constructor === Function` — a constructor object's own chain is
/// `C → Function.prototype`, with no back-edge to `C`; only
/// `C.prototype.constructor` and `(new C()).constructor` are `C`. The
/// `Function` answer already existed as the tail fallback of the class-ref arm
/// in `get_field_by_name.rs`; this walk simply returned first. The wrong
/// answer used to be load-bearing: perry handed a decorator on an INSTANCE
/// member the class itself where the spec hands it `Class.prototype`, so
/// NestJS-style `Reflect.defineMetadata(k, v, target.constructor)` landed on
/// `C` only because `C.constructor === C`. Two divergences that cancelled;
/// fixing either alone broke decorator metadata
/// (`test_decorators_nest_common_canary`,
/// `test_decorators_legacy_property_metadata`). Both halves were fixed
/// together — the decorator target in
/// `perry-hir/src/lower/decorators.rs::member_decorator_target` and the
/// `constructor` clause below; `test_decorators_target_prototype_9467` pins
/// the pair.
///
/// The `class_prototype_object` step below is never skipped: for a subclass of
/// a class-EXPRESSION value it holds the parent CLASS OBJECT (#1788/#6552),
/// which is genuinely on the constructor's static chain.
unsafe fn resolve_proto_chain_field_inner(
    class_id: u32,
    key: *const crate::StringHeader,
    receiver: Option<f64>,
    constructor_side: bool,
    mut read_miss: Option<&mut u64>,
) -> Option<JSValue> {
    if let Some(receiver) = receiver {
        let receiver_value = JSValue::from_bits(receiver.to_bits());
        if receiver_value.is_pointer() {
            let receiver_obj = receiver_value.as_pointer::<ObjectHeader>();
            if !receiver_obj.is_null() {
                if let Some(pin) = instance_pinned_constructing_class(receiver_obj) {
                    // A factory-created class can be evaluated again after
                    // this instance was built. Its template class id then
                    // points at the later evaluation. Resolve through this
                    // instance's pinned class object and its own prototype.
                    let scope = crate::gc::RuntimeHandleScope::new();
                    let receiver = scope.root_nanbox_f64(receiver);
                    let pin = scope.root_nanbox_f64(pin);
                    let key = scope.root_string_ptr(key as *mut crate::StringHeader);
                    let pin_obj = JSValue::from_bits(pin.get_nanbox_f64().to_bits())
                        .as_pointer::<ObjectHeader>();
                    let proto_value =
                        super::super::field_get_set::class_object_prototype_value(pin_obj);
                    let proto = JSValue::from_bits(proto_value.bits()).as_pointer::<ObjectHeader>();
                    if !proto.is_null() {
                        let proto = scope.root_raw_mut_ptr(proto as *mut ObjectHeader);
                        if let Some(value) = proto.with_mut_ptr::<ObjectHeader, _>(|proto| {
                            key.with_const_ptr::<crate::StringHeader, _>(|key| {
                                evaluated_parent_instance_field(
                                    proto,
                                    key,
                                    receiver.get_nanbox_f64(),
                                )
                            })
                        }) {
                            return Some(value);
                        }
                    }
                }
            }
        }
    }
    // Resolved once: `class_instance_has_method` already walks the parent
    // chain, so a parent's instance method is excluded from a subclass's
    // constructor read too.
    let skip_decl_prototype = constructor_side && !key.is_null() && {
        let key_ptr = crate::string::string_data(key);
        let key_len = (*key).byte_len as usize;
        std::str::from_utf8(std::slice::from_raw_parts(key_ptr, key_len))
            .map(|name| {
                // #9467: `C.prototype.constructor` is `C`; `C.constructor` is
                // `Function` (the class-ref arm's tail fallback), never `C`.
                name == "constructor" || crate::object::class_instance_has_method(class_id, name)
            })
            .unwrap_or(false)
    };
    let mut cid = class_id;
    let mut depth = 0usize;
    while depth < 32 {
        // The reflective `ClassName.prototype` object
        // (`CLASS_DECL_PROTOTYPE_OBJECTS`) is where a user
        // `Object.defineProperty(ClassName.prototype, name, { get })` installs
        // its accessor — distinct from the #711/#809 synthetic-proto cache
        // (`CLASS_PROTOTYPE_OBJECTS`) that the rest of this walk reads. The
        // instance-read walk historically only consulted the latter, so such a
        // getter was invisible to `instance.name` (winston:
        // `Object.defineProperty(Logger.prototype, 'transports', { get })`,
        // read as `this.transports`, came back `undefined` → `.length` threw).
        // Decl-proto object (`CLASS_DECL_PROTOTYPE_OBJECTS`) is where a user
        // `Object.defineProperty(ClassName.prototype, name, { get })` installs
        // its accessor AND where a runtime `C.prototype[k] = v` (computed /
        // dynamic index write) stores an OWN data field. It is allocated WITH
        // this `class_id` (`js_object_alloc(class_id, 0)`), so routing DATA
        // reads back through `js_object_get_field_by_name` would re-enter this
        // same walk for the same id and recurse infinitely (a Transform
        // subclass's `_read` lookup stack-overflowed → SIGSEGV).
        //
        // Accessors: fire with the instance as receiver.
        // Own data fields: read via `own_data_field_by_name` only (no class-id
        // re-walk) so a computed prototype write is visible as
        // `(new C()).name` (#6945). Class methods / vtable data still come
        // from the vtable + `class_prototype_object` path below.
        let decl_proto = if skip_decl_prototype {
            std::ptr::null_mut()
        } else {
            class_decl_prototype_object(cid)
        };
        if !decl_proto.is_null() {
            if let Some(receiver) = receiver {
                if let Some(value) = inherited_proto_accessor_value(decl_proto, key, receiver) {
                    return Some(value);
                }
            }
            // #6945: own data property written onto the reflective prototype
            // (computed / dynamic-key set) — never re-enter by-name get.
            if let Some(value) =
                unsafe { super::super::field_get_set::own_data_field_by_name(decl_proto, key) }
            {
                if !value.is_undefined() {
                    // Declared methods are mirrored onto the template's shared
                    // reflective prototype. For a ClassExprFresh instance,
                    // substitute the method closure owned by this particular
                    // class evaluation; otherwise separate factory calls share
                    // a lexical private brand and cross-calls incorrectly pass.
                    if let Some(receiver) = receiver {
                        let key_ptr = crate::string::string_data(key);
                        let key_len = (*key).byte_len as usize;
                        if let Ok(name) =
                            std::str::from_utf8(std::slice::from_raw_parts(key_ptr, key_len))
                        {
                            let name = name.to_string();
                            let scope = crate::gc::RuntimeHandleScope::new();
                            let value = scope.root_nanbox_u64(value.bits());
                            let receiver = scope.root_nanbox_u64(receiver.to_bits());
                            if super::super::native_module::class_has_own_method(cid, &name)
                                && value.get_nanbox_u64()
                                    == super::super::native_module::class_prototype_method_value_for_name(
                                        cid, &name,
                                    )
                                    .to_bits()
                            {
                                let receiver = f64::from_bits(receiver.get_nanbox_u64());
                                if let Some(brand) =
                                    super::super::private_evaluation_brand_value(receiver)
                                {
                                    let brand_obj = crate::value::JSValue::from_bits(brand.to_bits())
                                        .as_pointer::<ObjectHeader>();
                                    if !brand_obj.is_null()
                                        && js_object_get_class_id(brand_obj) == cid
                                    {
                                        let method = super::super::native_module::class_evaluation_method_value_for_name(
                                            cid, &name, brand,
                                        );
                                        return Some(JSValue::from_bits(method.to_bits()));
                                    }
                                }
                            }
                            return Some(JSValue::from_bits(value.get_nanbox_u64()));
                        }
                    }
                    return Some(value);
                }
            }
        }
        // `Object.setPrototypeOf(C.prototype, X)` relinks the declared
        // prototype. The registered parent class id no longer names the next
        // hop: the recorded link does, and the rest of the chain is X's.
        if let (false, Some(receiver)) = (decl_proto.is_null(), receiver) {
            match relinked_decl_prototype_field(cid, decl_proto, key, receiver) {
                RelinkedRead::NotRelinked => {}
                RelinkedRead::Answered(value) => return Some(value),
                RelinkedRead::Missed => return None,
            }
        }
        let mut proto_obj = class_prototype_object(cid);
        if let Some(receiver) = receiver {
            if let Some(parent_class) = declared_parent_class_object(cid, proto_obj) {
                // #10890: for a DECLARED class id this entry is the parent
                // CLASS OBJECT (#1788), which is on the constructor's static
                // chain and never on an instance's. Reading it for an instance
                // returned the parent's statics, and `name` returned the
                // class binding's own name (Effect's `out` / `Base`) instead
                // of the `name` written to that class's `prototype`. An
                // instance read goes to that evaluation's prototype object.
                if let Some(evaluated) =
                    super::super::field_get_set::class_object_materialized_prototype(parent_class)
                {
                    if let Some(value) = evaluated_parent_instance_field(evaluated, key, receiver) {
                        return Some(value);
                    }
                }
                proto_obj = std::ptr::null_mut();
            }
        }
        if !proto_obj.is_null() {
            if let Some(receiver) = receiver {
                if let Some(value) = inherited_proto_accessor_value(proto_obj, key, receiver) {
                    return Some(value);
                }
            }
            let field_val = if let Some(receiver) = receiver {
                // The recursive `get_field(proto_obj, key)` re-derives a class
                // getter's `this` from `proto_obj`; stash the real instance so an
                // inherited getter (object-literal `get x()` on an
                // `Object.create(proto)` prototype) binds `this` to the instance.
                let prev_override =
                    super::super::field_get_set::accessor_receiver_override_begin(receiver);
                let value = js_object_get_field_by_name(proto_obj as *const _, key);
                super::super::field_get_set::accessor_receiver_override_end(prev_override);
                value
            } else {
                js_object_get_field_by_name(proto_obj as *const _, key)
            };
            if !field_val.is_undefined() && !field_val.is_null() {
                return Some(field_val);
            }
            if field_val.is_undefined() {
                if let Some(read_miss) = read_miss.as_deref_mut() {
                    // The read can collect; re-read the address it now names.
                    let proto_now = class_prototype_object(cid);
                    if proto_now == proto_obj {
                        *read_miss = JSValue::pointer(proto_now as *const u8).bits();
                    }
                }
            }
        }
        match get_parent_class_id(cid) {
            Some(p) if p != 0 && p != cid => {
                cid = p;
                depth += 1;
            }
            _ => {
                if let Some(receiver) = receiver {
                    return bound_native_parent_prototype_field(cid, key, receiver);
                }
                break;
            }
        }
    }
    // A native parent is represented by its actual prototype rather than a
    // synthetic class-registry entry. Finish the declared prototype's real
    // chain using own descriptors; recursively entering the generic getter
    // can revisit this same class walk and mistake that recursion for a miss.
    if let Some(receiver) = receiver {
        return evaluated_parent_instance_field(
            class_decl_prototype_object(class_id),
            key,
            receiver,
        );
    }
    None
}

/// #10454: the class chain ended at `cid`, and `cid`'s heritage may be a
/// bound native-module export (`class Sub extends http.ServerResponse`).
/// `js_register_class_parent_dynamic` deliberately registers no parent class
/// id for those, so the walk above never reaches the export's `.prototype` —
/// an instance read of an inherited method (or of anything a user added to
/// `ServerResponse.prototype`) came back `undefined`, while
/// `Object.getPrototypeOf` (which follows the dynamic parent value) saw it.
/// Continue the read on that prototype object, with the instance as receiver.
unsafe fn bound_native_parent_prototype_field(
    cid: u32,
    key: *const crate::StringHeader,
    receiver: f64,
) -> Option<JSValue> {
    let parent = super::parent_static::template_dynamic_parent_value(cid);
    let parent_js = JSValue::from_bits(parent.to_bits());
    if !parent_js.is_pointer() {
        return None;
    }
    let parent_addr = (parent.to_bits() & crate::value::POINTER_MASK) as usize;
    if !crate::closure::is_closure_ptr(parent_addr)
        || crate::object::native_module::bound_native_callable_module_and_method(parent).is_none()
    {
        return None;
    }
    let proto = JSValue::from_bits(
        crate::closure::closure_get_dynamic_prop(parent_addr, "prototype").to_bits(),
    );
    if !proto.is_pointer() {
        return None;
    }
    let proto_obj = proto.as_pointer::<ObjectHeader>();
    if crate::value::addr_class::try_read_gc_header(proto_obj as usize)
        .is_none_or(|h| h.obj_type != crate::gc::GC_TYPE_OBJECT)
    {
        return None;
    }
    let prev_override = super::super::field_get_set::accessor_receiver_override_begin(receiver);
    let value = js_object_get_field_by_name(proto_obj as *const _, key);
    super::super::field_get_set::accessor_receiver_override_end(prev_override);
    (!value.is_undefined() && !value.is_null()).then_some(value)
}

/// #1758: symbol-keyed analogue of [`resolve_proto_chain_field`]. Walks the
/// `CLASS_PROTOTYPE_OBJECTS` chain and, at each prototype object (a POINTER
/// class-object), looks up its OWN symbol property via `own_symbol_property`.
/// Lets a subclass whose parent is a class-expression value inherit the
/// parent's static *symbol* statics — e.g. effect's
/// `class BigIntFromSelf extends make(bigIntKeyword) {}` inheriting
/// `static [TypeId]`, which `Predicate.hasProperty(.., TypeId)` (`isSchema`)
/// and `u[TypeId]` both read. Returns the first defined value found.
///
/// #26 / #321: the walk must advance along TWO axes, because a synthetic
/// `Object.create(proto)` class id links to its prototype via the *proto
/// object's own class id*, not via `parent_class_id` (which only models the
/// `class A extends B` axis). effect's `Either.right(x)` builds
/// `Object.create(RightProto)` where `RightProto = Object.create(CommonProto)`
/// and `CommonProto[TypeId]` carries the brand. With only the
/// `parent_class_id` axis the walk stopped after the first prototype object
/// (`RightProto`), so `TypeId in either` / `either[TypeId]` missed the brand
/// two links up — making `ParseResult.isEither(...)` false for every struct
/// property parse (`S.is`/`decodeUnknownSync`/`encodeSync` on a `Struct`).
/// At each node we follow the proto object's own class id (the
/// `Object.create` prototype link) first, then fall back to
/// `parent_class_id` (the `extends` link); a `visited` set bounds cycles.
///
/// #10481: an accessor found on a prototype object runs with `this ===
/// receiver`, the object the read started from — never the prototype object
/// that holds it.
pub(crate) unsafe fn resolve_proto_chain_symbol(
    class_id: u32,
    sym_f64: f64,
    receiver: f64,
) -> Option<f64> {
    proto_chain_symbol_slot(class_id, sym_f64).map(|slot| slot.read(receiver))
}

/// The walk behind [`resolve_proto_chain_symbol`], stopping at the nearest
/// prototype object that owns `sym_f64` without invoking an accessor there.
pub(crate) unsafe fn proto_chain_symbol_slot(
    class_id: u32,
    sym_f64: f64,
) -> Option<crate::symbol::OwnSymbolSlot> {
    proto_chain_symbol_slot_inner(class_id, sym_f64, false)
}

/// [`proto_chain_symbol_slot`] for a read on `obj` itself. A class object
/// inherits its parent class object's statics. Any other object (an instance
/// or a prototype) reads that parent's evaluated prototype instead (#10890).
pub(crate) unsafe fn object_proto_chain_symbol_slot(
    obj: *const ObjectHeader,
    sym_f64: f64,
) -> Option<crate::symbol::OwnSymbolSlot> {
    let class_id = crate::object::js_object_get_class_id(obj);
    if class_id == 0 {
        return None;
    }
    let instance = !is_class_object_ptr(obj as *const u8);
    proto_chain_symbol_slot_inner(class_id, sym_f64, instance)
}

unsafe fn proto_chain_symbol_slot_inner(
    class_id: u32,
    sym_f64: f64,
    instance: bool,
) -> Option<crate::symbol::OwnSymbolSlot> {
    let mut cid = class_id;
    let mut depth = 0usize;
    let mut visited: [u32; 32] = [0; 32];
    while depth < 32 {
        if visited[..depth].contains(&cid) {
            break;
        }
        visited[depth] = cid;
        let mut proto_obj = class_prototype_object(cid);
        let mut next_cid: u32 = 0;
        if instance {
            if let Some(parent_class) = declared_parent_class_object(cid, proto_obj) {
                if let Some(evaluated) =
                    super::super::field_get_set::class_object_materialized_prototype(parent_class)
                {
                    let evaluated = f64::from_bits(JSValue::pointer(evaluated as *const u8).bits());
                    if let Some(slot) = crate::symbol::own_symbol_slot(evaluated, sym_f64) {
                        return Some(slot);
                    }
                }
                // The `extends` axis below reaches the parent's template.
                proto_obj = std::ptr::null_mut();
            }
        }
        if !proto_obj.is_null() {
            let proto_f64 = f64::from_bits(JSValue::pointer(proto_obj as *const u8).bits());
            // OWN lookup only — this fn IS the chain walk, so recursing into
            // the full chain-walking getter would re-walk per prototype.
            if let Some(slot) = crate::symbol::own_symbol_slot(proto_f64, sym_f64) {
                return Some(slot);
            }
            // Prefer the `Object.create` prototype link: the next chain node
            // is the proto object's own class id (which maps to ITS proto in
            // CLASS_PROTOTYPE_OBJECTS). Falls back to `parent_class_id` below.
            next_cid = crate::object::js_object_get_class_id(proto_obj as *const ObjectHeader);
        }
        if next_cid != 0 && next_cid != cid {
            cid = next_cid;
            depth += 1;
            continue;
        }
        match get_parent_class_id(cid) {
            Some(p) if p != 0 && p != cid => {
                cid = p;
                depth += 1;
            }
            _ => break,
        }
    }
    None
}

/// Static layout of a bound events constructor's canonical prototype.
pub(crate) unsafe fn nm_ee_prototype_layout(
    func_value: f64,
) -> *const super::super::layout_desc::PerryLayoutDesc {
    match super::super::native_module::bound_native_callable_module_and_method(func_value) {
        Some((module, method)) if module.trim_start_matches("node:") == "events" => {
            match method.as_str() {
                "EventEmitter" => {
                    std::ptr::addr_of!(super::super::runtime_layouts::EVENT_EMITTER_PROTOTYPE)
                }
                "EventEmitterAsyncResource" => {
                    std::ptr::addr_of!(super::super::runtime_layouts::EE_ASYNC_RESOURCE_PROTO)
                }
                _ => std::ptr::null(),
            }
        }
        _ => std::ptr::null(),
    }
}

/// #5477: when `func_value` is the bound `events.EventEmitter` /
/// `EventEmitterAsyncResource` export, its synthetic prototype must carry the
/// EventEmitter methods (the `Object.setPrototypeOf(x, EventEmitter.prototype)`
/// mixin pattern — pino). Extracted verbatim; reached ONLY through
/// `NmEeOps::ee_prototype_install`.
pub(crate) unsafe fn nm_ee_prototype_install(
    func_value: f64,
    proto: *mut crate::object::ObjectHeader,
) {
    let scope = crate::gc::RuntimeHandleScope::new();
    let func_value = scope.root_nanbox_f64(func_value);
    let proto = scope.root_raw_mut_ptr(proto);
    if let Some((module, method)) =
        super::super::native_module::bound_native_callable_module_and_method(
            func_value.get_nanbox_f64(),
        )
    {
        if module.trim_start_matches("node:") == "events"
            && matches!(
                method.as_str(),
                "EventEmitter" | "EventEmitterAsyncResource"
            )
        {
            if method == "EventEmitterAsyncResource" {
                // node: `class EventEmitterAsyncResource extends EventEmitter`,
                // so its prototype inherits the emitter methods from the one
                // shared `EventEmitter.prototype` and adds only its own.
                let parent = scope.root_nanbox_f64(
                    crate::node_stream::event_emitter_prototype_value("EventEmitter"),
                );
                if crate::value::JSValue::from_bits(parent.get_nanbox_u64()).is_pointer() {
                    proto.with_mut_ptr::<crate::object::ObjectHeader, _>(|proto| {
                        super::super::prototype_chain::object_set_static_prototype(
                            proto as usize,
                            parent.get_nanbox_u64(),
                        )
                    });
                }
                proto.with_mut_ptr::<crate::object::ObjectHeader, _>(|proto| {
                    crate::node_stream::install_event_emitter_async_resource_prototype(proto)
                });
            } else {
                // node's `EventEmitter.prototype`: the state defaults, the
                // methods, and `kCapture: false`, filling its layout; then
                // sealed, so a program write sets its shadow bit.
                proto.with_mut_ptr::<crate::object::ObjectHeader, _>(|proto| {
                    crate::node_stream::install_event_emitter_prototype(proto)
                });
                let layout = super::super::runtime_layouts::NodePrototype::of(std::ptr::addr_of!(
                    super::super::runtime_layouts::EVENT_EMITTER_PROTOTYPE
                ));
                proto.with_mut_ptr::<crate::object::ObjectHeader, _>(|proto| {
                    layout.symbol_data(
                        proto,
                        std::ptr::addr_of!(super::super::runtime_layouts::NP_SYMBOL_K_CAPTURE),
                        f64::from_bits(crate::value::TAG_FALSE),
                    );
                    layout.seal(
                        proto,
                        crate::closure::shape::INTRINSIC_SERIAL_EVENT_EMITTER_PROTOTYPE,
                    );
                });
            }
        }
    }
}

#[cfg(test)]
mod born_old_tests {
    use super::*;

    extern "C" fn body(
        _: *const crate::closure::ClosureHeader,
        _this: crate::closure::JsThis,
    ) -> f64 {
        0.0
    }

    unsafe fn is_born_old(proto: *mut ObjectHeader) -> bool {
        let header =
            (proto as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
        crate::gc::never_move::is_never_move(header)
            && crate::gc::never_move::lives_old(proto as usize)
    }

    /// Every default prototype is stationary from birth, including one
    /// owned by a young function. Its ordinary owner edge still determines liveness.
    #[test]
    fn every_default_prototype_is_born_old() {
        let _copying = crate::gc::CopyingNurseryTestGuard::new(0);
        let _age = crate::gc::pin_tenuring_survivals_for_test(1);
        crate::gc::register_runtime_handle_root_scanner_for_tests();
        crate::gc::gc_register_mutable_root_scanner(
            crate::object::shapes::scan_shape_table_rekey_mut,
        );
        let scope = crate::gc::RuntimeHandleScope::new();
        let make = || {
            crate::value::js_nanbox_pointer(crate::closure::js_closure_alloc(
                crate::fn_info!(body, 0),
                0,
            ) as i64)
        };
        let young = scope.root_nanbox_f64(make());
        let young_addr =
            JSValue::from_bits(young.get_nanbox_f64().to_bits()).as_pointer::<u8>() as usize;
        assert!(
            unsafe { !crate::gc::never_move::lives_old(young_addr) },
            "premise: the function is young"
        );
        let young_proto = ensure_function_prototype_object(young.get_nanbox_f64());
        assert!(!young_proto.is_null());
        assert!(
            unsafe { is_born_old(young_proto) },
            "a young function's prototype is born old and never-move"
        );
        let old = scope.root_nanbox_f64(make());
        let _ = crate::gc::gc_collect_minor();
        let old_addr =
            JSValue::from_bits(old.get_nanbox_f64().to_bits()).as_pointer::<u8>() as usize;
        assert!(
            unsafe { crate::gc::never_move::lives_old(old_addr) },
            "premise: the minor promoted the function"
        );
        let old_proto = ensure_function_prototype_object(old.get_nanbox_f64());
        assert!(!old_proto.is_null());
        assert!(
            unsafe { is_born_old(old_proto) },
            "an old function's prototype is born old and never-move"
        );
    }
}
