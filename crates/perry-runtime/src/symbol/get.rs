//! Symbol-keyed property reads: the `js_object_get_symbol_property` resolver
//! and its prototype-chain / well-known-symbol / handle helpers.

use super::*;
use crate::string::js_string_from_bytes;

/// Does `obj` carry an OWN symbol-keyed property under `sym`, **without
/// invoking** an accessor for it?
///
/// [`own_symbol_property`] answers the same question by performing the read,
/// which for an accessor property means calling the user getter. A caller that
/// only wants to know whether the builtin behaviour has been shadowed — the
/// `[...arr]` dense fast path in `array::flat_clone` — must not run that getter,
/// because the slow path it falls back to will read the property again and the
/// getter would then observe two calls where the spec mandates one.
///
/// Deliberately mirrors [`own_symbol_property`]'s two lookups in the same order
/// (accessor table, then the raw `SYMBOL_PROPERTIES` data table) so the two can
/// never disagree about existence; only the *invocation* differs.
pub(crate) unsafe fn has_own_symbol_property(obj_f64: f64, sym_f64: f64) -> bool {
    if accessors::symbol_accessor_property(obj_f64, sym_f64).is_some() {
        return true;
    }
    let obj_key = obj_key_from_f64(obj_f64);
    let sym_key = sym_key_from_f64(sym_f64);
    if obj_key == 0 || sym_key == 0 {
        return false;
    }
    if crate::object::shaped_symbols::owner(obj_key).is_some() {
        return crate::object::shaped_symbols::get(obj_key, sym_key).is_some();
    }
    false
}

/// #1758: the OWN symbol-property lookup — the raw `SYMBOL_PROPERTIES`
/// side-table read keyed by the object's address (no class-ref / no prototype
/// chain). Used by `js_object_get_symbol_property` and by
/// `resolve_proto_chain_symbol`, which walks prototype objects itself and must
/// therefore NOT recurse into the full chain-walking getter.
pub(crate) unsafe fn own_symbol_property(obj_f64: f64, sym_f64: f64) -> Option<f64> {
    own_symbol_property_for_receiver(obj_f64, sym_f64, obj_f64)
}

/// #10481: [`own_symbol_property`] on `obj_f64` for a `[[Get]]` whose
/// receiver is `receiver` — the object the read started from, which differs
/// from `obj_f64` whenever a prototype walk found the property on an ancestor.
/// An accessor's getter runs with `this === receiver` (spec `[[Get]](P,
/// Receiver)`). The getter used to be called with no receiver at all, so it
/// observed whatever `IMPLICIT_THIS` the caller happened to leave behind:
/// fastify's inherited `Reply.prototype[kRouteContext]` getter saw
/// `this === undefined` on every request.
pub(crate) unsafe fn own_symbol_property_for_receiver(
    obj_f64: f64,
    sym_f64: f64,
    receiver: f64,
) -> Option<f64> {
    own_symbol_slot(obj_f64, sym_f64).map(|slot| slot.read(receiver))
}

/// An own symbol-keyed property as stored, before any accessor runs (#10481).
/// Lets a prototype walk locate the holder once and leave the decision to its
/// caller: a `[[Get]]` reads it for the original receiver, a `[[Set]]` runs
/// an accessor's setter or stops at a data property.
#[derive(Clone, Copy)]
pub(crate) enum OwnSymbolSlot {
    Accessor { get: u64, set: u64 },
    Data(u64),
}

impl OwnSymbolSlot {
    /// `[[Get]]` of this property with `this === receiver`.
    pub(crate) unsafe fn read(self, receiver: f64) -> f64 {
        match self {
            OwnSymbolSlot::Accessor { get, .. } => {
                accessors::invoke_symbol_accessor_getter(get, receiver)
            }
            OwnSymbolSlot::Data(bits) => f64::from_bits(bits),
        }
    }
}

mod own;
pub(crate) use own::own_symbol_slot;

unsafe fn object_header_ptr_from_value_bits(bits: u64) -> Option<usize> {
    let (raw, obj_type) = heap_ptr_and_type_from_value_bits(bits)?;
    if crate::gc_type_is_object!(obj_type) {
        Some(raw)
    } else {
        None
    }
}

/// #9192: [`object_header_ptr_from_value_bits`] for the RECEIVER position,
/// which may be a real array (`Object.setPrototypeOf(arr, {[S]: v})`). Only the
/// ADDRESS is used here — as the key of the recorded-`[[Prototype]]` lookup —
/// so an `ArrayHeader` is safe, while the chain HOPS still require a genuine
/// `GC_TYPE_OBJECT` before anything is dereferenced as one.
unsafe fn receiver_ptr_from_value_bits(bits: u64) -> Option<usize> {
    let (raw, obj_type) = heap_ptr_and_type_from_value_bits(bits)?;
    if crate::object::prototype_chain::owned_prototype_receiver_type(obj_type) {
        Some(raw)
    } else {
        None
    }
}

/// Validate a value's bits as a live tracked heap allocation and report its
/// address together with its GC type byte.
unsafe fn heap_ptr_and_type_from_value_bits(bits: u64) -> Option<(usize, u8)> {
    let top16 = bits >> 48;
    let raw = if top16 == 0x7FFD {
        (bits & POINTER_MASK) as usize
    } else {
        return None;
    };
    let header = crate::value::addr_class::try_read_tracked_gc_header(raw)?;
    Some((raw, header.as_ref().obj_type))
}

/// Walk the prototype chains to find an inherited symbol property. Used by
/// `Object.prototype.toString` to implement the spec's `Get(O, @@toStringTag)`
/// prototype-chain walk; an accessor runs with `this === obj_f64` (#10481).
pub(crate) unsafe fn inherited_symbol_property(obj_f64: f64, sym_f64: f64) -> Option<f64> {
    inherited_symbol_slot(obj_f64, sym_f64).map(|slot| slot.read(obj_f64))
}

/// `receiver` is the `this` an inherited accessor runs with (#10481).
unsafe fn resolve_explicit_object_prototype_symbol(
    obj_f64: f64,
    sym_f64: f64,
    receiver: f64,
) -> Option<f64> {
    explicit_prototype_symbol_lookup(obj_f64, sym_f64).map(|lookup| match lookup {
        PrototypeSymbolLookup::Slot(slot) => slot.read(receiver),
        PrototypeSymbolLookup::Proxy(proxy) => {
            crate::proxy_hooks::proxy_get_from_prototype(proxy, sym_f64, receiver)
        }
    })
}

enum PrototypeSymbolLookup {
    Slot(OwnSymbolSlot),
    Proxy(f64),
}

unsafe fn explicit_prototype_symbol_slot(obj: f64, sym: f64) -> Option<OwnSymbolSlot> {
    match explicit_prototype_symbol_lookup(obj, sym)? {
        PrototypeSymbolLookup::Slot(slot) => Some(slot),
        PrototypeSymbolLookup::Proxy(_) => None,
    }
}

/// The recorded and intrinsic-array prototype walk behind
/// [`resolve_explicit_object_prototype_symbol`], stopping at the nearest
/// holder without invoking it.
unsafe fn explicit_prototype_symbol_lookup(
    obj_f64: f64,
    sym_f64: f64,
) -> Option<PrototypeSymbolLookup> {
    const TAG_NULL: u64 = 0x7FFC_0000_0000_0002;
    let mut owner = receiver_ptr_from_value_bits(obj_f64.to_bits())?;
    let mut visited_buf = [0usize; 16];
    let mut visited_len = 0usize;
    let mut visited_overflow: Option<std::collections::HashSet<usize>> = None;
    loop {
        let proto_bits = match symbol_prototype_word(owner) {
            Some(bits) => bits,
            None => {
                let (_, kind) = heap_ptr_and_type_from_value_bits(owner as u64)?;
                if crate::gc_type_is_not_array!(kind) {
                    return None;
                }
                // Resolving an intrinsic can allocate; keep the current
                // chain address stable while obtaining its parent.
                let _no_move = crate::gc::GcSuppressScope::new();
                // Use the intrinsic root, independent of globalThis.Array.
                // Array.prototype itself inherits from Object.prototype.
                let array_proto = crate::array::array_prototype_addr();
                let proto = if array_proto == owner {
                    crate::array::object_prototype_addr()
                } else {
                    array_proto
                };
                if proto == 0 {
                    return None;
                }
                crate::value::js_nanbox_pointer(proto as i64).to_bits()
            }
        };
        if proto_bits == TAG_NULL {
            return None;
        }
        let proto = f64::from_bits(proto_bits);
        if crate::proxy_hooks::proxy_is_proxy(proto) != 0 {
            return Some(PrototypeSymbolLookup::Proxy(proto));
        }
        if let Some(slot) = own_symbol_slot(f64::from_bits(proto_bits), sym_f64) {
            return Some(PrototypeSymbolLookup::Slot(slot));
        }
        let proto_ptr = receiver_ptr_from_value_bits(proto_bits)?;
        // Cycle detection.
        let cycle = if visited_len < visited_buf.len() {
            visited_buf[..visited_len].contains(&proto_ptr)
        } else {
            let set = visited_overflow.get_or_insert_with(|| visited_buf.iter().copied().collect());
            !set.insert(proto_ptr)
        };
        if cycle {
            return None;
        }
        if visited_len < visited_buf.len() {
            visited_buf[visited_len] = owner;
            visited_len += 1;
        } else if let Some(set) = &mut visited_overflow {
            set.insert(owner);
        }
        if object_header_ptr_from_value_bits(proto_bits).is_some()
            && !crate::object::is_class_object_ptr(proto_ptr as *const u8)
        {
            let proto_obj = proto_ptr as *const crate::object::ObjectHeader;
            if let Some(slot) = crate::object::object_proto_chain_symbol_slot(proto_obj, sym_f64) {
                return Some(PrototypeSymbolLookup::Slot(slot));
            }
        }
        owner = proto_ptr;
    }
}

/// The next symbol-lookup holder is the actual constructor evaluation, not
/// its template's latest heritage. Functions and fresh class objects share
/// this walk with ordinary objects; their own traced prototype edges win.
unsafe fn symbol_prototype_word(owner: usize) -> Option<u64> {
    if crate::closure::is_closure_ptr(owner) {
        if let Some(bits) = crate::closure::closure_static_prototype(owner) {
            return Some(bits);
        }
        if let Some(cid) = crate::object::class_value::class_value_id_bits(
            crate::value::js_nanbox_pointer(owner as i64).to_bits(),
        ) {
            // An intrinsic parent may need materializing. The chain walker
            // holds raw addresses and does not invoke user code here.
            let _no_move = crate::gc::GcSuppressScope::new();
            let evaluated = crate::object::class_prototype_object(cid);
            let parent = if !evaluated.is_null()
                && crate::object::is_class_object_ptr(evaluated as *const u8)
            {
                evaluated as usize
            } else {
                crate::object::class_value::class_prototype_addr(cid)
            };
            return (parent != 0).then(|| crate::value::js_nanbox_pointer(parent as i64).to_bits());
        }
        // A function's default parent is a fact of its body kind, even when
        // it has no explicit prototype override. Resolve the same intrinsic
        // chain as Object.getPrototypeOf; the walker then reads the holder's
        // live symbol slot, including user definitions and deletes.
        let _no_move = crate::gc::GcSuppressScope::new();
        if let Some(proto) = crate::object::global_this::generator_function_proto_of(owner) {
            return Some(proto.to_bits());
        }
        let proto = crate::closure::shape::function_prototype_ptr_materialized();
        return (proto != 0).then(|| crate::value::js_nanbox_pointer(proto as i64).to_bits());
    }
    crate::object::prototype_chain::object_static_prototype(owner).or_else(|| {
        crate::object::is_class_object_ptr(owner as *const u8)
            .then(|| crate::object::class_object_pinned_parent(owner as *const crate::ObjectHeader))
            .flatten()
            .map(f64::to_bits)
    })
}

unsafe fn web_stream_symbol_property(obj_f64: f64, sym_f64: f64) -> Option<f64> {
    let id = crate::object::stream_handle_id(obj_f64)?;
    let kind_probe = crate::object::stream_handle_kind_probe()?;
    let kind = kind_probe(id);
    if kind == 0 {
        return None;
    }

    let sym_key = sym_key_from_f64(sym_f64);
    if sym_key == 0 {
        return Some(f64::from_bits(TAG_UNDEFINED));
    }

    let iterator = well_known_symbol("iterator");
    if !iterator.is_null() {
        let iterator_f64 =
            f64::from_bits(crate::value::JSValue::pointer(iterator as *const u8).bits());
        if sym_key == sym_key_from_f64(iterator_f64) {
            return Some(f64::from_bits(TAG_UNDEFINED));
        }
    }

    let async_iterator = well_known_symbol("asyncIterator");
    if !async_iterator.is_null() {
        let async_iterator_f64 =
            f64::from_bits(crate::value::JSValue::pointer(async_iterator as *const u8).bits());
        if sym_key == sym_key_from_f64(async_iterator_f64) {
            // RSTREAM R5: the prototype's own member (`=== P.values`).
            if kind == 1 {
                let proto = crate::object::builtin_prototype_value("ReadableStream");
                if crate::value::JSValue::from_bits(proto.to_bits()).is_pointer() {
                    return Some(js_object_get_symbol_property(proto, sym_f64));
                }
            }
            return Some(f64::from_bits(TAG_UNDEFINED));
        }
    }

    let to_string_tag = well_known_symbol("toStringTag");
    if !to_string_tag.is_null() {
        let to_string_tag_f64 =
            f64::from_bits(crate::value::JSValue::pointer(to_string_tag as *const u8).bits());
        if sym_key == sym_key_from_f64(to_string_tag_f64) {
            let Some(tag) = crate::object::web_stream_interface_name(id) else {
                return Some(f64::from_bits(TAG_UNDEFINED));
            };
            let str_ptr = js_string_from_bytes(tag.as_ptr(), tag.len() as u32);
            return Some(f64::from_bits(STRING_TAG | (str_ptr as u64 & POINTER_MASK)));
        }
    }

    Some(f64::from_bits(TAG_UNDEFINED))
}

/// `Symbol.toStringTag` for Perry's Web/runtime built-ins that have no
/// registered prototype-chain or class-id hook reachable from the generic
/// resolvers below (#10555) -- see `web_builtin_to_string_tag`'s doc comment
/// for the full inventory and why each kind needs this. An own override
/// (`Object.defineProperty(x, Symbol.toStringTag, …)`) still wins: the
/// side-table read is a pointer-KEYED lookup, safe even for the
/// handle-backed kinds since it never dereferences `obj_f64` as a pointer.
unsafe fn web_builtin_to_string_tag_symbol_property(obj_f64: f64, sym_f64: f64) -> Option<f64> {
    let sym_key = sym_key_from_f64(sym_f64);
    if sym_key == 0 {
        return None;
    }
    let to_string_tag = well_known_symbol("toStringTag");
    if to_string_tag.is_null() {
        return None;
    }
    let ts_f64 = f64::from_bits(crate::value::JSValue::pointer(to_string_tag as *const u8).bits());
    if sym_key != sym_key_from_f64(ts_f64) {
        return None;
    }
    if let Some(v) = own_symbol_property(obj_f64, sym_f64) {
        return Some(v);
    }
    let tag = crate::object::web_builtin_to_string_tag(obj_f64)?;
    let str_ptr = js_string_from_bytes(tag.as_ptr(), tag.len() as u32);
    Some(f64::from_bits(STRING_TAG | (str_ptr as u64 & POINTER_MASK)))
}

/// Native core instances resolve their intrinsic prototype reflectively, not
/// through the ordinary symbol-property chain. Consult the actual prototype
/// for @@toStringTag after own and explicit-chain reads have missed.
unsafe fn reflected_prototype_to_string_tag_symbol_property(
    obj_f64: f64,
    sym_f64: f64,
    receiver_f64: f64,
) -> Option<f64> {
    let value = crate::value::JSValue::from_bits(obj_f64.to_bits());
    if !value.is_pointer() {
        return None;
    }
    let tag = well_known_symbol("toStringTag");
    if tag.is_null() {
        return None;
    }
    let tag_value = f64::from_bits(crate::value::JSValue::pointer(tag as *const u8).bits());
    if sym_key_from_f64(sym_f64) != sym_key_from_f64(tag_value) {
        return None;
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let obj = scope.root_nanbox_f64(obj_f64);
    let sym = scope.root_nanbox_f64(sym_f64);
    let receiver = scope.root_nanbox_f64(receiver_f64);
    let proto = scope.root_nanbox_f64(crate::object::js_object_get_prototype_of(
        obj.get_nanbox_f64(),
    ));
    if !crate::value::JSValue::from_bits(proto.get_nanbox_f64().to_bits()).is_pointer() {
        return None;
    }
    own_symbol_property_for_receiver(
        proto.get_nanbox_f64(),
        sym.get_nanbox_f64(),
        receiver.get_nanbox_f64(),
    )
    .or_else(|| {
        resolve_explicit_object_prototype_symbol(
            proto.get_nanbox_f64(),
            sym.get_nanbox_f64(),
            receiver.get_nanbox_f64(),
        )
    })
}

#[no_mangle]
pub unsafe extern "C" fn js_object_get_symbol_property(obj_f64: f64, sym_f64: f64) -> f64 {
    js_object_get_symbol_property_with_receiver(obj_f64, sym_f64, obj_f64)
}

/// `[[Get]](sym_f64, receiver_f64)` on `obj_f64`: the property is resolved on
/// `obj_f64` and its prototype chain, and every accessor found along the way
/// runs with `this === receiver_f64` (#10481). `Reflect.get(target, sym,
/// receiver)` is the caller whose receiver differs from the holder of the
/// lookup; every ordinary `obj[sym]` read passes `obj_f64` itself.
pub(crate) unsafe fn js_object_get_symbol_property_with_receiver(
    obj_f64: f64,
    sym_f64: f64,
    receiver_f64: f64,
) -> f64 {
    // A Number holder with clear top 16 bits (a denormal, or +0) is not a
    // raw object pointer: see `number_receiver_for_dispatch`. `receiver_f64`
    // keeps the Number itself.
    let obj_f64 = crate::value::addr_class::number_receiver_for_dispatch(obj_f64);
    // A Proxy is a small registered id (its band overlaps the small-handle
    // band); dereferencing it as a heap object to read a symbol-keyed property
    // is an EXC_BAD_ACCESS. Route a SYMBOL-keyed read through the proxy `get`
    // trap (which forwards to the target). drizzle's aliased-column proxies are
    // read with symbol keys (`col[entityKind]`, `col[Table.Symbol.*]`) while
    // building a relational query.
    if crate::proxy_hooks::proxy_is_proxy(obj_f64) != 0 {
        return crate::proxy_hooks::proxy_get_with_receiver(obj_f64, sym_f64, receiver_f64);
    }
    // A `Date` is a `DateCell` (NaN-boxed pointer, NOT an `ObjectHeader`). A
    // symbol-keyed read (`d[Symbol.toPrimitive]`, `d[Symbol.iterator]`) must
    // NOT reach the pointer-deref paths below — they reinterpret the cell as an
    // `ObjectHeader` (`js_object_get_class_id`, prototype walk) and read
    // garbage. Resolve OWN symbol props from the side table first (a user may
    // `d[sym] = x`), then inherit from `Date.prototype` (which carries the
    // installed `[Symbol.toPrimitive]`). This is the DateCell analogue of the
    // ordinary object's own-then-prototype symbol walk.
    if crate::date::is_date_value(obj_f64) {
        if let Some(v) = own_symbol_property_for_receiver(obj_f64, sym_f64, receiver_f64) {
            return v;
        }
        // Materializing `Date.prototype` can allocate; the receiver outlives it.
        let scope = crate::gc::RuntimeHandleScope::new();
        let receiver_h = scope.root_nanbox_f64(receiver_f64);
        let proto = crate::object::builtin_prototype_value("Date");
        if (proto.to_bits() >> 48) == 0x7FFD {
            if let Some(v) =
                own_symbol_property_for_receiver(proto, sym_f64, receiver_h.get_nanbox_f64())
            {
                return v;
            }
        }
        return f64::from_bits(TAG_UNDEFINED);
    }
    // Check the class's static symbols first when receiver is a class
    // constructor.
    let bits = obj_f64.to_bits();
    if let Some(class_id) = crate::object::class_value::class_ref_word(bits) {
        // A legacy class word names the same function object as its minted
        // pointer. Resolve its own bag through the ordinary symbol slot
        // evaluator before consulting declaration metadata.
        if let Some(owner) = crate::object::class_value::class_value_if_minted(class_id) {
            let owner = f64::from_bits(crate::JSValue::pointer(owner.cast()).bits());
            if let Some(value) = own_symbol_property_for_receiver(owner, sym_f64, receiver_f64) {
                return value;
            }
            if let Some(value) =
                resolve_explicit_object_prototype_symbol(owner, sym_f64, receiver_f64)
            {
                return value;
            }
        }
        if let Some(vb) = super::class_static_symbol_lookup_in_chain(class_id, sym_f64) {
            return f64::from_bits(vb);
        }
        // A constructor whose heritage was a runtime value inherits from that
        // exact value, the same edge `Object.getPrototypeOf(C)` reports. A
        // fresh class evaluation is not reachable through the template-id
        // tables below, so continue the ordinary [[Get]] on the actual parent.
        let parent = crate::object::js_get_dynamic_parent_value(class_id);
        if crate::object::is_class_object_value(parent) {
            return js_object_get_symbol_property_with_receiver(parent, sym_f64, receiver_f64);
        }
        // #1758: a class ref whose own static symbols miss may inherit the
        // symbol from a class-expression parent (`class Sub extends make(...) {}`
        // → `Sub[TypeId]`). Walk the CLASS_PROTOTYPE_OBJECTS chain.
        if let Some(v) = crate::object::resolve_proto_chain_symbol(class_id, sym_f64, receiver_f64)
        {
            return v;
        }
        // #36 / #321: the subclass extends a FUNCTION value
        // (`class Svc extends Context.Tag(id)<...>() {}`). Read the symbol off
        // the parent closure — own symbol props plus, via the closure symbol
        // getter, its static prototype (`Svc[TagTypeId]`/`Svc[EffectTypeId]`
        // live on TagProto). Recurse into the closure-aware getter so its proto
        // walk fires. #10210: the edge may sit on an ancestor class — walk
        // the chain.
        if let Some(closure_ptr) = crate::object::parent_closure_in_chain(class_id) {
            let closure_f64 =
                f64::from_bits(crate::value::js_nanbox_pointer(closure_ptr as i64).to_bits());
            // #10481: preserve the caller's receiver here too — without it, an
            // accessor reached through the parent closure's own symbol walk
            // would see the closure as `this` instead of the original
            // receiver (e.g. `Reflect.get(Child, sym, other)`).
            let v = js_object_get_symbol_property_with_receiver(closure_f64, sym_f64, receiver_f64);
            if v.to_bits() != TAG_UNDEFINED {
                return v;
            }
        }
        // #11193: the subclass (or an ancestor) extends a built-in constructor
        // (`class X extends Array`). The built-in's own symbol statics — the
        // `get [Symbol.species]` accessor — live on its constructor closure,
        // which the chain edge (a reserved class id) does not reach. Read it
        // there with the original receiver, so the inherited species getter
        // answers `X`, not `Array`.
        if let Some(parent_ctor) = crate::object::builtin_parent_ctor_in_chain(class_id) {
            return js_object_get_symbol_property_with_receiver(parent_ctor, sym_f64, receiver_f64);
        }
        return f64::from_bits(TAG_UNDEFINED);
    }
    // #1545: Web Stream handles are registry ids, not heap objects. Resolve
    // their well-known symbol surface before pointer-oriented fallback paths
    // treat the payload as an address. ReadableStream is
    // async-iterable only; none of the Web Stream handles expose
    // `Symbol.iterator`.
    if let Some(v) = web_stream_symbol_property(obj_f64, sym_f64) {
        return v;
    }
    if let Some(v) = web_builtin_to_string_tag_symbol_property(obj_f64, sym_f64) {
        return v;
    }
    // #340/#341: the Timeout/Immediate `Symbol.dispose` arm is gone — the
    // timer prototypes carry a real symbol-keyed method, so `using t =
    // setTimeout(...)` (#1213) resolves it through the ordinary symbol-property
    // walk instead of a small-id special case.
    // Generic small-handle `Symbol.dispose` support. Subsystems that expose
    // a dispose method through HANDLE_PROPERTY_DISPATCH can bind it here
    // without adding a runtime-specific special case.
    if (bits >> 48) == (crate::value::HANDLE_TAG >> 48) {
        let id = (bits & 0x0000_FFFF_FFFF_FFFF) as i64;
        if crate::value::addr_class::is_small_handle(id as usize) {
            let dispose = well_known_symbol("dispose");
            if !dispose.is_null() {
                let dispose_f64 =
                    f64::from_bits(crate::value::JSValue::pointer(dispose as *const u8).bits());
                if sym_key_from_f64(sym_f64) == sym_key_from_f64(dispose_f64) {
                    if let Some(dispatch) = crate::object::handle_property_dispatch() {
                        let method = b"@@__perry_wk_dispose";
                        let v = dispatch(id, method.as_ptr(), method.len());
                        if v.to_bits() != TAG_UNDEFINED {
                            return v;
                        }
                    }
                }
            }
        }
    }
    // Generic small-handle `Symbol.asyncDispose` support. This must run before
    // pointer-backed symbol property lookup so small native handles are not
    // interpreted as heap pointers when the dispatcher owns the method.
    if (bits >> 48) == (crate::value::HANDLE_TAG >> 48) {
        let id = (bits & 0x0000_FFFF_FFFF_FFFF) as i64;
        if crate::value::addr_class::is_small_handle(id as usize) {
            let async_dispose = well_known_symbol("asyncDispose");
            if !async_dispose.is_null() {
                let async_dispose_f64 = f64::from_bits(
                    crate::value::JSValue::pointer(async_dispose as *const u8).bits(),
                );
                if sym_key_from_f64(sym_f64) == sym_key_from_f64(async_dispose_f64) {
                    if let Some(dispatch) = crate::object::handle_property_dispatch() {
                        let method = b"@@__perry_wk_asyncDispose";
                        let v = dispatch(id, method.as_ptr(), method.len());
                        if v.to_bits() != TAG_UNDEFINED {
                            return v;
                        }
                    }
                }
            }
        }
    }
    // Generic small-handle `Symbol.asyncIterator` support — lets a handle-backed
    // readable drive `for await (const chunk of x)`. Mirrors the
    // `Symbol.asyncDispose` block above: resolve the bound `@@asyncIterator`
    // method through the handle property dispatcher, so a subsystem that owns the
    // method (perry's http `IncomingMessage`) can expose it without a
    // runtime-specific special case. #6432 — `for await…of req` used to throw
    // `is not iterable`, so Next.js read an empty POST body → Auth.js MissingCSRF.
    if (bits >> 48) == (crate::value::HANDLE_TAG >> 48) {
        let id = (bits & 0x0000_FFFF_FFFF_FFFF) as i64;
        if crate::value::addr_class::is_small_handle(id as usize) {
            let async_iterator = well_known_symbol("asyncIterator");
            if !async_iterator.is_null() {
                let async_iterator_f64 = f64::from_bits(
                    crate::value::JSValue::pointer(async_iterator as *const u8).bits(),
                );
                if sym_key_from_f64(sym_f64) == sym_key_from_f64(async_iterator_f64) {
                    if let Some(dispatch) = crate::object::handle_property_dispatch() {
                        let method = b"@@asyncIterator";
                        let v = dispatch(id, method.as_ptr(), method.len());
                        if v.to_bits() != TAG_UNDEFINED {
                            return v;
                        }
                    }
                }
            }
        }
    }
    // Web Fetch and other stdlib handle-backed values are small ids
    // NaN-boxed as POINTER. A computed `handle[Symbol.iterator]` reaches the
    // symbol resolver directly, bypassing the normal string-key handle
    // property dispatcher. Map the well-known symbol back to the dispatcher so
    // `Headers` can expose its `entries` method as the iterator function.
    if (bits >> 48) == (crate::value::HANDLE_TAG >> 48) {
        let id = (bits & 0x0000_FFFF_FFFF_FFFF) as i64;
        if crate::value::addr_class::is_small_handle(id as usize) {
            let iter_wk = well_known_symbol("iterator");
            if !iter_wk.is_null() {
                let iter_f64 =
                    f64::from_bits(crate::value::JSValue::pointer(iter_wk as *const u8).bits());
                if sym_key_from_f64(sym_f64) == sym_key_from_f64(iter_f64) {
                    if let Some(dispatch) = crate::object::handle_property_dispatch() {
                        let prop = b"@@iterator";
                        let value = dispatch(id, prop.as_ptr(), prop.len());
                        if value.to_bits() != TAG_UNDEFINED {
                            return value;
                        }
                    }
                }
            }
        }
    }
    // Small native handles (HTTP IncomingMessage/socket, fetch bodies, etc.)
    // NaN-boxed as POINTER are NOT heap objects: the well-known-symbol dispatch
    // above already handled the symbols they expose. Any OTHER symbol read must
    // return undefined rather than falling through to the pointer-deref paths
    // below (`symbol_accessor_property` / `own_symbol_property` /
    // `resolve_explicit_object_prototype_symbol`), which reinterpret the tiny
    // handle id as an ObjectHeader and read `id + offset` → EXC_BAD_ACCESS.
    // @hono/node-server reads symbols off the IncomingMessage handle while
    // adapting it to a web Request. Proxies share the small-id band
    // (0xF0000..0x100000) but have real symbol semantics, so exclude them.
    if (bits >> 48) == (crate::value::HANDLE_TAG >> 48) {
        let id = (bits & 0x0000_FFFF_FFFF_FFFF) as usize;
        // Only short-circuit values that are NOT real heap objects. A genuine
        // ObjectHeader can live at a low address in a small program, so gate on
        // `is_valid_obj_ptr` (validates the GcHeader) rather than the address
        // band alone — otherwise a symbol read on a low-address object returned
        // undefined. Proxies (registered small ids) keep their own semantics.
        if crate::value::addr_class::is_small_handle(id)
            && !crate::object::is_valid_obj_ptr(id as *const u8)
            && crate::proxy_hooks::proxy_is_proxy(obj_f64) == 0
        {
            // A user-stored symbol property (set via the symbol side table,
            // keyed by the handle pointer — e.g. @hono/node-server's
            // `incoming[wrapBodyStream] = true`) round-trips here. The side
            // table is a pointer-keyed map, so this read does NOT dereference
            // the small handle id as an ObjectHeader (which would EXC_BAD_ACCESS
            // / segfault); it is safe for native handles.
            if let Some(v) = own_symbol_property(obj_f64, sym_f64) {
                return v;
            }
            return f64::from_bits(TAG_UNDEFINED);
        }
    }
    if let Some(acc) = accessors::symbol_accessor_property(obj_f64, sym_f64) {
        return accessors::invoke_symbol_accessor_getter(acc.get, receiver_f64);
    }
    if let Some(v) = own_symbol_property(obj_f64, sym_f64) {
        return v;
    }
    // Later hops allocate (`Function.prototype`, the reflected toStringTag):
    // root the receiver, the symbol and the receiver value across them.
    let scope = crate::gc::RuntimeHandleScope::new();
    let obj_h = scope.root_nanbox_f64(obj_f64);
    let sym_h = scope.root_nanbox_f64(sym_f64);
    let receiver_h = scope.root_nanbox_f64(receiver_f64);
    let bits = obj_f64.to_bits();
    let sym_key = sym_key_from_f64(sym_f64);
    if sym_key != 0 {
        let jsval = crate::value::JSValue::from_bits(bits);
        if jsval.is_pointer() {
            let ptr = jsval.as_pointer::<crate::object::ObjectHeader>();
            if !ptr.is_null() && crate::object::is_valid_obj_ptr(ptr as *const u8) {
                let class_id = crate::object::js_object_get_class_id(ptr);
                if class_id != 0 && crate::object::shapes::receiver_has_class_identity(ptr) {
                    // An evaluation's recorded prototype is its complete surface.
                    if let Some(value) = resolve_explicit_object_prototype_symbol(
                        obj_h.get_nanbox_f64(),
                        sym_h.get_nanbox_f64(),
                        receiver_h.get_nanbox_f64(),
                    ) {
                        return value;
                    }
                    if !crate::object::prototype_chain::object_prototype_is_foreign(
                        crate::value::js_nanbox_get_pointer(obj_h.get_nanbox_f64()) as usize,
                    ) {
                        let _ = crate::object::class_decl_prototype_value(class_id);
                        if let Some(value) = declared_prototype_chain_symbol(
                            obj_h.get_nanbox_f64(),
                            sym_h.get_nanbox_f64(),
                            class_id,
                            receiver_h.get_nanbox_f64(),
                        ) {
                            return value;
                        }
                    }
                }
            }
        }
    }
    if let Some(v) = resolve_explicit_object_prototype_symbol(obj_f64, sym_f64, receiver_f64) {
        return v;
    }
    // An `ArrayBuffer` / `SharedArrayBuffer` / `DataView` has no symbol
    // members of its own beyond the ones read above, and none of the
    // built-in arms below describe it: the read continues on its
    // [[Prototype]] (the intrinsic buffer prototype, or the newTarget
    // prototype it was constructed with), so a member the program wrote
    // there (`ArrayBuffer.prototype[Symbol.iterator] = function* () {}`)
    // is found, with the original receiver.
    let raw_view = crate::value::js_nanbox_get_pointer(obj_h.get_nanbox_f64()) as usize;
    if raw_view >= 0x10000 && crate::buffer::is_non_indexed_buffer_view(raw_view) {
        // Materializing the intrinsic prototype can allocate; `obj_h`,
        // `sym_h` and `receiver_h` keep the operands rooted across it.
        let proto = crate::object::js_object_get_prototype_of(obj_h.get_nanbox_f64());
        if crate::value::JSValue::from_bits(proto.to_bits()).is_null() {
            return f64::from_bits(TAG_UNDEFINED);
        }
        return js_object_get_symbol_property_with_receiver(
            proto,
            sym_h.get_nanbox_f64(),
            receiver_h.get_nanbox_f64(),
        );
    }
    // `class X extends Map | Set` instance — its default `[Symbol.iterator]`
    // is inherited from Map/Set.prototype, so it is NOT an own symbol prop.
    // Reading the property (e.g. `typeof obj[Symbol.iterator] === 'function'`,
    // as `iterare`'s `isIterable` / `toIterator` do for NestJS's
    // `ModulesContainer extends Map`) must still resolve to a callable.
    // Return a bound method that, when invoked, produces the backing
    // collection's default iterator (entries for Map, values for Set — see
    // the `"Symbol.iterator"` arms in `collection_methods.rs`).
    //
    // This runs AFTER the class/prototype symbol walk and the explicit-prototype
    // lookup above, so a user override — `class M extends Map {
    // *[Symbol.iterator]() {} }` (registered as `@@iterator` and resolved at the
    // class/proto walk) or `Object.setPrototypeOf(m, { [Symbol.iterator]: … })`
    // — wins, and we only synthesize the built-in default when the normal lookup
    // would have fallen through.
    if sym_key != 0 {
        let iter_wk = well_known_symbol("iterator");
        if !iter_wk.is_null() {
            let iter_f64 =
                f64::from_bits(crate::value::JSValue::pointer(iter_wk as *const u8).bits());
            if sym_key == sym_key_from_f64(iter_f64)
                && crate::object::map_set_subclass::subclass_backing_of(obj_f64).is_some()
            {
                let mname = b"Symbol.iterator";
                return crate::object::js_class_method_bind(obj_f64, mname.as_ptr(), mname.len());
            }
        }
    }
    if sym_key != 0 {
        let iter_wk = well_known_symbol("iterator");
        if !iter_wk.is_null() {
            let iter_f64 =
                f64::from_bits(crate::value::JSValue::pointer(iter_wk as *const u8).bits());
            if sym_key == sym_key_from_f64(iter_f64) {
                let raw_iter_ptr = crate::value::js_nanbox_get_pointer(obj_f64) as usize;
                if raw_iter_ptr >= 0x10000
                    && crate::array::is_builtin_iterator_class_id(raw_iter_ptr)
                {
                    let receiver = if (bits >> 48) == 0x7FFD {
                        obj_f64
                    } else {
                        crate::value::js_nanbox_pointer(raw_iter_ptr as i64)
                    };
                    let method = b"Symbol.iterator";
                    return crate::object::js_class_method_bind(
                        receiver,
                        method.as_ptr(),
                        method.len(),
                    );
                }
            }
        }
    }
    // Buffer extends Uint8Array in Node, so Buffer values must expose
    // @@iterator as values(). Perry's direct Buffer.from() paths often
    // materialize through array-clone fast paths, but runtime-produced
    // Buffers can reach generic iterator lookup first.
    let raw_ptr = crate::value::js_nanbox_get_pointer(obj_f64) as usize;
    if raw_ptr >= 0x10000 && crate::buffer::is_byte_indexed_buffer(raw_ptr) {
        let iter_wk = well_known_symbol("iterator");
        if !iter_wk.is_null() {
            let iter_f64 =
                f64::from_bits(crate::value::JSValue::pointer(iter_wk as *const u8).bits());
            if sym_key_from_f64(sym_f64) == sym_key_from_f64(iter_f64) {
                let mname = b"values";
                return crate::object::js_class_method_bind(obj_f64, mname.as_ptr(), mname.len());
            }
        }
    }
    // #4102: every function value inherits `%Function.prototype%`, so reading a
    // well-known symbol off a constructor *value* whose own / explicit-prototype
    // lookups missed must fall back to Function.prototype's own symbols. Most
    // importantly this exposes `@@hasInstance` (#4098), so
    // `(Array as any)[Symbol.hasInstance]([])` resolves the installed
    // `OrdinaryHasInstance` thunk instead of `undefined`. Perry does not link a
    // closure's static prototype to Function.prototype, so this is the hop that
    // models that inheritance for the symbol-read path.
    if (bits >> 48) == 0x7FFD {
        let ptr = crate::value::js_nanbox_get_pointer(obj_f64) as usize;
        if ptr != 0 && crate::closure::is_closure_ptr(ptr) {
            let func_proto = crate::object::builtin_prototype_value("Function");
            if (func_proto.to_bits() >> 48) == 0x7FFD {
                // Re-read: materializing `Function.prototype` can allocate.
                let receiver = receiver_h.get_nanbox_f64();
                if let Some(v) = own_symbol_property_for_receiver(func_proto, sym_f64, receiver) {
                    return v;
                }
            }
        }
    }
    // Buffers inherit TypedArray iteration semantics in Node: the default
    // iterator is `values()`, yielding numeric bytes.
    let is_pointer = (bits >> 48) == 0x7FFD;
    let raw_addr = if is_pointer {
        (bits & POINTER_MASK) as usize
    } else {
        bits as usize
    };
    // Module namespace exotic objects expose an own @@toStringTag. Perry's
    // namespaces share one synthetic class and virtualize their exports, so
    // resolve the tag from the stored module name instead of duplicating a
    // physical symbol property on every namespace instance.
    if is_pointer
        && crate::value::addr_class::is_above_handle_band(raw_addr)
        && crate::object::is_valid_obj_ptr(raw_addr as *const u8)
    {
        let obj = raw_addr as *const crate::object::ObjectHeader;
        if unsafe { (*obj).class_id } == crate::object::NATIVE_MODULE_CLASS_ID {
            let tag_wk = well_known_symbol("toStringTag");
            if !tag_wk.is_null() {
                let tag_f64 =
                    f64::from_bits(crate::value::JSValue::pointer(tag_wk as *const u8).bits());
                if sym_key_from_f64(sym_f64) == sym_key_from_f64(tag_f64)
                    && matches!(
                        crate::object::read_native_module_name(obj).as_deref(),
                        Some("module" | "async_hooks")
                    )
                {
                    let tag = b"Module";
                    let value = js_string_from_bytes(tag.as_ptr(), tag.len() as u32);
                    return f64::from_bits(STRING_TAG | (value as u64 & POINTER_MASK));
                }
            }
        }
    }
    if raw_addr >= 0x1000 && crate::buffer::is_byte_indexed_buffer(raw_addr) {
        let iter_wk = well_known_symbol("iterator");
        if !iter_wk.is_null() {
            let iter_f64 =
                f64::from_bits(crate::value::JSValue::pointer(iter_wk as *const u8).bits());
            if sym_key_from_f64(sym_f64) == sym_key_from_f64(iter_f64) {
                let this_f64 =
                    f64::from_bits(crate::value::js_nanbox_pointer(raw_addr as i64).to_bits());
                let mname = b"values";
                return crate::object::js_class_method_bind(this_f64, mname.as_ptr(), mname.len());
            }
        }
    }
    if raw_addr >= 0x1000
        && crate::typedarray_hook_slots::lookup_typed_array_kind(raw_addr).is_some()
    {
        let iter_wk = well_known_symbol("iterator");
        if !iter_wk.is_null() {
            let iter_f64 =
                f64::from_bits(crate::value::JSValue::pointer(iter_wk as *const u8).bits());
            if sym_key_from_f64(sym_f64) == sym_key_from_f64(iter_f64) {
                let this_f64 =
                    f64::from_bits(crate::value::js_nanbox_pointer(raw_addr as i64).to_bits());
                let mname = b"values";
                return crate::object::js_class_method_bind(this_f64, mname.as_ptr(), mname.len());
            }
        }
    }
    // `(new Int8Array())[Symbol.toStringTag]` → `"Int8Array"` (and Node
    // `Buffer`/`Uint8Array` → `"Uint8Array"`). The accessor lives on the
    // `%TypedArray%.prototype` intrinsic, not the instance, so the OWN-accessor
    // lookup above missed it; resolve the constructor name directly off the
    // receiver here (the intrinsic getter does the same via its `this`). Covers
    // both the raw-pointer typed-array form and Perry's buffer-backed
    // `Uint8Array`. `safe-stable-stringify` (a pino dep) relies on this.
    if raw_addr >= 0x1000 {
        let tag_wk = well_known_symbol("toStringTag");
        if !tag_wk.is_null() {
            let tag_f64 =
                f64::from_bits(crate::value::JSValue::pointer(tag_wk as *const u8).bits());
            if sym_key_from_f64(sym_f64) == sym_key_from_f64(tag_f64) {
                if let Some(name) = crate::object::typed_array_to_string_tag_name(obj_f64) {
                    let s = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
                    return f64::from_bits(crate::js_nanbox_string(s as i64).to_bits());
                }
            }
        }
    }
    // Return the actual prototype iterator, whose thunk reads call-time this.
    // Binding `values` to this array makes a Proxy get trap returning
    // target[Symbol.iterator] iterate the target, bypassing length/index traps
    // even when GetIterator calls the method with the Proxy receiver (#10270).
    if crate::array::js_array_is_array(obj_f64).to_bits() == crate::value::TAG_TRUE {
        let iter_wk = well_known_symbol("iterator");
        if !iter_wk.is_null() {
            let iter_f64 =
                f64::from_bits(crate::value::JSValue::pointer(iter_wk as *const u8).bits());
            if sym_key_from_f64(sym_f64) == sym_key_from_f64(iter_f64) {
                // Use the actual chain and the same symbol Get. Array.prototype
                // is itself an Array: after deletion, its parent may supply the
                // method, and an inherited getter receives the original source.
                let scope = crate::gc::RuntimeHandleScope::new();
                let object = scope.root_nanbox_f64(obj_f64);
                let symbol = scope.root_nanbox_f64(sym_f64);
                let receiver = scope.root_nanbox_f64(receiver_f64);
                let proto = crate::object::js_object_get_prototype_of(object.get_nanbox_f64());
                if crate::value::JSValue::from_bits(proto.to_bits()).is_null() {
                    return f64::from_bits(TAG_UNDEFINED);
                }
                return js_object_get_symbol_property_with_receiver(
                    proto,
                    symbol.get_nanbox_f64(),
                    receiver.get_nanbox_f64(),
                );
            }
        }
    }
    // #2856: `Map.prototype[Symbol.iterator]` aliases `entries`, and
    // `Set.prototype[Symbol.iterator]` aliases `values`. Bind the matching
    // method so `m[Symbol.iterator]()` returns a real iterator object (and
    // `Symbol.iterator in m` / `typeof m[Symbol.iterator]` are correct).
    if raw_addr >= 0x10000 {
        let iter_wk = well_known_symbol("iterator");
        if !iter_wk.is_null() {
            let iter_f64 =
                f64::from_bits(crate::value::JSValue::pointer(iter_wk as *const u8).bits());
            if sym_key_from_f64(sym_f64) == sym_key_from_f64(iter_f64) {
                if crate::map::is_registered_map(raw_addr) {
                    let mname = b"entries";
                    return crate::object::js_class_method_bind(
                        obj_f64,
                        mname.as_ptr(),
                        mname.len(),
                    );
                }
                if crate::set::is_registered_set(raw_addr) {
                    let mname = b"values";
                    return crate::object::js_class_method_bind(
                        obj_f64,
                        mname.as_ptr(),
                        mname.len(),
                    );
                }
            }
        }
    }
    // #1758: a POINTER class-object whose OWN symbol props miss may inherit
    // the symbol through its class_id prototype chain. (The SYMBOL_PROPERTIES
    // lock is released above before recursing into the resolver, which takes
    // it again per prototype object.)
    if (bits >> 48) == 0x7FFD {
        let obj_ptr =
            crate::value::JSValue::from_bits(bits).as_pointer::<crate::object::ObjectHeader>();
        if !obj_ptr.is_null() {
            let cid = crate::object::js_object_get_class_id(obj_ptr);
            if cid != 0 {
                if let Some(slot) = crate::object::object_proto_chain_symbol_slot(obj_ptr, sym_f64)
                {
                    return slot.read(receiver_f64);
                }
                // A class OBJECT (a pointer, e.g. a class declared in a function
                // body) extending a built-in constructor inherits that
                // constructor's symbol statics, as the class-ref form does.
                if crate::object::is_class_object_ptr(obj_ptr as *const u8) {
                    if let Some(parent_ctor) = crate::object::builtin_parent_ctor_in_chain(cid) {
                        let v = js_object_get_symbol_property_with_receiver(
                            parent_ctor,
                            sym_f64,
                            receiver_f64,
                        );
                        if v.to_bits() != TAG_UNDEFINED {
                            return v;
                        }
                    }
                }
                // A symbol-keyed property added to a DECLARED class's
                // `.prototype` after the declaration — `C.prototype[S] = f`,
                // `Object.defineProperty(C.prototype, S, ...)`, or
                // `Object.assign(C.prototype, { [Symbol.iterator]() {} })` —
                // is stored on the declared prototype object, which the
                // CLASS_PROTOTYPE_OBJECTS walk above never visits (that table
                // holds `Object.create` / class-expression parents). String
                // keys already resolve there; symbol keys returned undefined,
                // so drizzle-orm's `applyEffectWrapper` (effect's
                // `Effectable.Prototype` assigned onto query classes) left
                // `yield* query` with no iterator ("next is not a function").
                if let Some(v) =
                    declared_prototype_chain_symbol(obj_f64, sym_f64, cid, receiver_f64)
                {
                    return v;
                }
            }
        }
    }
    if let Some(value) = reflected_prototype_to_string_tag_symbol_property(
        obj_h.get_nanbox_f64(),
        sym_h.get_nanbox_f64(),
        receiver_h.get_nanbox_f64(),
    ) {
        return value;
    }
    f64::from_bits(TAG_UNDEFINED)
}

/// Walk the declared class prototype objects (`C.prototype` for `C` and each
/// ancestor) for a symbol-keyed property written after the class declaration.
/// Accessors run with the original receiver; data properties are returned as
/// stored. Nearest class first, so a subclass's prototype write shadows a
/// base class's.
unsafe fn declared_prototype_chain_symbol(
    obj: f64,
    sym: f64,
    class_id: u32,
    receiver: f64,
) -> Option<f64> {
    let holder = declared_prototype_symbol_holder(obj, sym, class_id)?;
    own_symbol_property_for_receiver(holder, sym, receiver)
}

/// Locate a declared prototype property without invoking its getter. An
/// explicit prototype replaces the class default, including when it is null.
unsafe fn declared_prototype_symbol_holder(receiver: f64, sym: f64, class_id: u32) -> Option<f64> {
    let receiver_addr = (receiver.to_bits() & crate::value::POINTER_MASK) as usize;
    // An explicit receiver prototype is handled by the ordinary recorded walk.
    if crate::object::prototype_chain::object_prototype_is_foreign(receiver_addr) {
        return None;
    }
    let mut declared = crate::object::class_decl_prototype_object(class_id);
    // The declaration prototype's shape is the heritage authority, including
    // native parents and a user relink. A parent class id cannot describe a
    // native prototype's ordinary symbol properties.
    for _ in 0..32 {
        if declared.is_null() {
            return None;
        }
        let proto_value = crate::value::js_nanbox_pointer(declared as i64);
        if has_own_symbol_property(proto_value, sym) {
            return Some(proto_value);
        }
        let bits = crate::object::prototype_chain::object_static_prototype(declared as usize)?;
        declared = object_header_ptr_from_value_bits(bits)? as *mut crate::object::ObjectHeader;
    }
    None
}

/// #10481: the accessor an ordinary `[[Set]]` of `sym` on `obj` must run when
/// `obj` has no own property under it — the nearest inherited holder along
/// the chains the getter reads (the recorded `[[Prototype]]` chain, the
/// synthetic class-id prototype chain, the declared class prototypes), as its
/// `(get, set)` bits. `None` when that holder is a data property or nothing on
/// those chains carries `sym`. Nothing is invoked.
pub(crate) unsafe fn inherited_symbol_accessor(obj: f64, sym: f64) -> Option<(u64, u64)> {
    match inherited_symbol_slot(obj, sym)? {
        OwnSymbolSlot::Accessor { get, set } => Some((get, set)),
        OwnSymbolSlot::Data(_) => None,
    }
}

/// The nearest inherited holder of `sym` for an ordinary object `obj`, in the
/// order the getter consults these chains, without invoking it.
unsafe fn inherited_symbol_slot(obj: f64, sym: f64) -> Option<OwnSymbolSlot> {
    if let Some(slot) = explicit_prototype_symbol_slot(obj, sym) {
        return Some(slot);
    }
    let ptr = object_header_ptr_from_value_bits(obj.to_bits())?;
    let class_id = crate::object::js_object_get_class_id(ptr as *const _);
    if class_id == 0 {
        return None;
    }
    if let Some(slot) = crate::object::object_proto_chain_symbol_slot(ptr as *const _, sym) {
        return Some(slot);
    }
    own_symbol_slot(declared_prototype_symbol_holder(obj, sym, class_id)?, sym)
}

/// Presence of the declared-prototype properties handled above, including
/// accessors and data properties whose value is undefined.
pub(crate) unsafe fn has_declared_prototype_symbol_property(receiver: f64, sym: f64) -> bool {
    let value = crate::value::JSValue::from_bits(receiver.to_bits());
    if !value.is_pointer() {
        return false;
    }
    let ptr = value.as_pointer::<crate::object::ObjectHeader>();
    // Pointer tags also carry handles and headerless storage. Require allocator
    // ownership before reading a header, then admit only live object layouts.
    let Some(header) = crate::value::addr_class::try_read_tracked_gc_header(ptr as usize) else {
        return false;
    };
    let header = header.as_ref();
    if crate::gc_type_is_not_object!(header.obj_type)
        || header.gc_flags & crate::gc::GC_FLAG_FORWARDED != 0
    {
        return false;
    }
    let class_id = (*ptr).class_id;
    class_id != 0 && declared_prototype_symbol_holder(receiver, sym, class_id).is_some()
}

#[cfg(test)]
mod handle_meta_share_tests {
    //! #5437: a native handle's symbol-keyed metadata must be shared by
    //! reference across heap wrappers that alias it (Next.js NodeNextRequest
    //! over an IncomingMessage), and must survive an `undefined` write-back.
    use super::*;

    // A registered `Symbol.for(key)` as a NaN-boxed f64.
    unsafe fn registered_symbol(key: &str) -> f64 {
        let kh = js_string_from_bytes(key.as_ptr(), key.len() as u32);
        let key_f64 = crate::value::js_nanbox_string(kh as i64);
        super::constructors::js_symbol_for(key_f64)
    }

    // The exact registered symbol Next.js uses; the undefined-write wipe guard
    // is gated to THIS symbol, so the metadata tests must use it (not a
    // test-suffixed variant) for the no-op behaviour to fire.
    unsafe fn next_request_meta_symbol() -> f64 {
        registered_symbol("NextInternalRequestMeta")
    }

    // Handle ids MUST stay below 0x1000: `is_valid_obj_ptr` uses
    // HEAP_MIN=0x1000 on Linux, so a larger id is misclassified as a heap
    // object and breaks the handle-band gate. Each test's id is backed by a
    // real native owner record (the stdlib registry's shape), whose bag holds
    // the handle's properties.

    // An IMMOVABLE metadata value: a plain NaN-boxed number. Unlike a heap
    // object, a number is never a pointer, so no GC move rewrites it — its
    // bits are invariant across any collection. The #5437 no-op-on-undefined logic
    // is value-agnostic, so a number tests it faithfully without depending on
    // GC suppression to keep a heap `meta` from moving.
    fn immovable_meta() -> f64 {
        // A normal finite double whose bits are stable and unambiguous.
        1234.5_f64
    }

    #[test]
    fn wrapper_reads_share_underlying_handle_meta() {
        // Node semantics (DEC 243(3)): a wrapper whose `_req` is the handle
        // reads only its OWN `[SYM]`. The wrapper ctor's
        // `this[SYM] = this._req[SYM] || {}` is what shares the handle's
        // object; an unseeded wrapper reads undefined, never the handle's
        // value through `_req`.
        unsafe {
            crate::gc::js_gc_collect();
            crate::gc::gc_suppress();
            let sym = next_request_meta_symbol();
            let owner =
                crate::object::handle_expando::test_support::TestSmallHandleOwner::new(0x321);
            let handle = owner.value();
            let meta_obj = crate::object::js_object_alloc(0, 0);
            let meta = crate::value::js_nanbox_pointer(meta_obj as i64);
            super::properties::js_object_set_symbol_property(handle, sym, meta);
            let req_key = js_string_from_bytes(b"_req".as_ptr(), 4);

            let unseeded_obj = crate::object::js_object_alloc(0, 1);
            crate::object::js_object_set_field_by_name(unseeded_obj, req_key, handle);
            let unseeded = crate::value::js_nanbox_pointer(unseeded_obj as i64);
            let unseeded_got = js_object_get_symbol_property(unseeded, sym);

            let seeded_obj = crate::object::js_object_alloc(0, 1);
            crate::object::js_object_set_field_by_name(seeded_obj, req_key, handle);
            let seeded = crate::value::js_nanbox_pointer(seeded_obj as i64);
            let shared = js_object_get_symbol_property(handle, sym);
            super::properties::js_object_set_symbol_property(seeded, sym, shared);
            let seeded_got = js_object_get_symbol_property(seeded, sym);
            let handle_got = js_object_get_symbol_property(handle, sym);
            crate::gc::gc_unsuppress();

            assert_eq!(
                unseeded_got.to_bits(),
                TAG_UNDEFINED,
                "an unseeded wrapper reads undefined, as in node"
            );
            assert_eq!(
                shared.to_bits(),
                meta.to_bits(),
                "the handle holds the meta object"
            );
            assert_eq!(
                seeded_got.to_bits(),
                meta.to_bits(),
                "a seeded wrapper shares the handle's object"
            );
            assert_eq!(handle_got.to_bits(), meta.to_bits(), "the handle keeps it");
        }
    }

    #[test]
    fn undefined_write_does_not_clobber_handle_meta() {
        unsafe {
            // Immovable number `meta` → no heap object to move → assertion is
            // GC-invariant; `gc_suppress` is defensive only.
            crate::gc::gc_suppress();
            let sym = next_request_meta_symbol();
            let owner =
                crate::object::handle_expando::test_support::TestSmallHandleOwner::new(0x654);
            let handle = owner.value();
            let meta = immovable_meta();
            super::properties::js_object_set_symbol_property(handle, sym, meta);

            // The `this._req[SYM] = this[SYM]` write-back where `this[SYM]` is
            // undefined must NOT erase the handle's existing meta.
            let undef = f64::from_bits(TAG_UNDEFINED);
            super::properties::js_object_set_symbol_property(handle, sym, undef);

            let got = js_object_get_symbol_property(handle, sym);
            crate::gc::gc_unsuppress();
            assert_eq!(
                got.to_bits(),
                meta.to_bits(),
                "an undefined write must not clobber a handle's existing meta"
            );
        }
    }

    #[test]
    fn undefined_write_to_plain_heap_object_still_clears() {
        unsafe {
            // The wipe-guard is gated to handle-band receivers; a normal heap
            // object setting a symbol prop to undefined must still take effect.
            let sym = registered_symbol("plainObjSym@@test_clear");
            let obj_ptr = crate::object::js_object_alloc(0, 0);
            let obj = crate::value::js_nanbox_pointer(obj_ptr as i64);
            let v = immovable_meta();
            super::properties::js_object_set_symbol_property(obj, sym, v);
            let undef = f64::from_bits(TAG_UNDEFINED);
            super::properties::js_object_set_symbol_property(obj, sym, undef);
            let got = js_object_get_symbol_property(obj, sym);
            assert_eq!(
                got.to_bits(),
                TAG_UNDEFINED,
                "heap-object symbol prop set to undefined must read undefined"
            );
        }
    }

    #[test]
    fn undefined_write_clears_non_metadata_symbol_on_handle() {
        unsafe {
            // The wipe-guard is narrowed to `Symbol.for("NextInternalRequestMeta")`.
            // Any OTHER symbol on a handle must clear normally with `undefined`.
            crate::gc::gc_suppress();
            let sym = registered_symbol("someOtherHandleSym@@test_clear");
            // Distinct handle id so this doesn't alias the metadata tests.
            let owner =
                crate::object::handle_expando::test_support::TestSmallHandleOwner::new(0x789);
            let handle = owner.value();
            let v = immovable_meta();
            super::properties::js_object_set_symbol_property(handle, sym, v);

            // Sanity: the value is observable before the clear.
            let before = js_object_get_symbol_property(handle, sym);
            assert_eq!(
                before.to_bits(),
                v.to_bits(),
                "non-metadata handle symbol should be set before clearing"
            );

            // Writing undefined to a NON-metadata symbol on a handle MUST clear it.
            let undef = f64::from_bits(TAG_UNDEFINED);
            super::properties::js_object_set_symbol_property(handle, sym, undef);
            let got = js_object_get_symbol_property(handle, sym);
            crate::gc::gc_unsuppress();
            assert_eq!(
                got.to_bits(),
                TAG_UNDEFINED,
                "a non-metadata symbol on a handle must be clearable with undefined"
            );
        }
    }
}
