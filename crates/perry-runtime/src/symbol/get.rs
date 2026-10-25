//! Symbol-keyed property reads: the `js_object_get_symbol_property` resolver
//! and its prototype-chain / well-known-symbol / handle helpers.

use super::*;
use crate::string::js_string_from_bytes;

/// #5128: map a well-known-symbol key to the synthetic class-method name used
/// for a symbol-keyed instance *method* (`*[Symbol.iterator]()` →
/// `@@iterator`, `[Symbol.asyncIterator]()` → `@@asyncIterator`). Returns
/// `None` for any other symbol. Used by `js_object_get_symbol_property` to
/// resolve a user class's iterator method off its prototype.
fn well_known_symbol_method_name(sym_key: usize) -> Option<&'static str> {
    for (wk, method) in [
        ("iterator", "@@iterator"),
        ("asyncIterator", "@@asyncIterator"),
    ] {
        let s = well_known_symbol(wk);
        if !s.is_null() {
            let f = f64::from_bits(crate::value::JSValue::pointer(s as *const u8).bits());
            if sym_key == unsafe { sym_key_from_f64(f) } {
                return Some(method);
            }
        }
    }
    None
}

/// An iterator dispatch alias is a fallback for the method declared in source.
/// Check writes on the intervening prototypes first, but stop at the nearest
/// declaration so a subclass method still shadows a replaced base method.
unsafe fn class_iterator_prototype_override(
    receiver: f64,
    sym: f64,
    mut class_id: u32,
    method_owner: u32,
) -> Option<f64> {
    for _ in 0..32 {
        let declared = crate::object::class_decl_prototype_object(class_id);
        let dynamic = crate::object::class_prototype_object(class_id);
        for proto in [declared, dynamic] {
            if proto.is_null() {
                continue;
            }
            let proto_value = crate::value::js_nanbox_pointer(proto as i64);
            if let Some(acc) = accessors::symbol_accessor_property(proto_value, sym) {
                return Some(accessors::invoke_symbol_accessor_getter(acc.get, receiver));
            }
            if let Some(value) = own_symbol_property(proto_value, sym) {
                return Some(value);
            }
        }
        if class_id == method_owner {
            break;
        }
        match crate::object::get_parent_class_id(class_id) {
            Some(parent) if parent != 0 && parent != class_id => class_id = parent,
            _ => break,
        }
    }
    None
}

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

mod generator;
mod own;
pub(crate) use own::own_symbol_slot;

/// #5437: resolve a symbol-keyed read against the underlying native handle a
/// request wrapper aliases via its `_req` field. Returns `None` unless the
/// receiver is a heap object whose `_req` is a small handle (POINTER-tagged,
/// not a heap object) that holds the requested symbol in the side table.
unsafe fn req_handle_symbol_fallback(obj_f64: f64, sym_f64: f64) -> Option<f64> {
    // #10510: the tail below only ever returns what a small native handle
    // holds in the symbol side tables. Until one has ever been given a
    // symbol-keyed property, skip the `"_req"` intern + by-name prototype-chain
    // walk — it dominated every implicit `ToPrimitive` on a plain object.
    if !crate::symbol::small_handle_symbol_owner_ever() {
        return None;
    }
    let bits = obj_f64.to_bits();
    if (bits >> 48) != 0x7FFD {
        return None;
    }
    let raw = (bits & POINTER_MASK) as usize;
    // Only heap-object wrappers carry a `_req` field; skip handle receivers.
    // `is_valid_obj_ptr` already rejects small native handles (it validates the
    // GcHeader) and accepts a genuine heap object even at a low address, so the
    // extra `is_small_handle` pre-check would have wrongly rejected a real heap
    // wrapper that happens to live in the low band.
    if !crate::object::is_valid_obj_ptr(raw as *const u8) {
        return None;
    }
    // #7498: THIS IS THE STALE DEREF `PERRY_GC_PROTECT_FROMSPACE=1` REPORTS
    // for `[...obj.arr]`. `js_string_from_bytes` below ALLOCATES, so a copying
    // minor can relocate the receiver while it exists only in the bare `raw`
    // usize — which the collector cannot see and therefore never rewrites.
    // `js_object_get_field_by_name` then reads that pre-move copy's
    // `keys_array` field out of retired from-space, and the fault lands on a
    // 40-byte `GC_TYPE_ARRAY` two frames down.
    //
    // This helper runs on EVERY heap-object symbol read whose own-symbol
    // lookup missed — including every `[Symbol.iterator]` resolution behind an
    // array or object spread — so the window is unconditional, which is why
    // the reproducer faults 5/5 rather than intermittently.
    let scope = crate::gc::RuntimeHandleScope::new();
    let recv_h = scope.root_nanbox_f64(obj_f64);
    let key = b"_req";
    let kh = crate::string::js_string_from_bytes(key.as_ptr(), key.len() as u32);
    let req = crate::object::js_object_get_field_by_name_f64(
        crate::value::js_nanbox_get_pointer(recv_h.get_nanbox_f64())
            as *const crate::object::ObjectHeader,
        kh as *const crate::StringHeader,
    );
    // The `_req` must be a native handle, never another heap object — a
    // heap `_req` would be served by its own normal symbol path, and recursing
    // into it risks loops.
    if !crate::value::JSValue::from_bits(req.to_bits()).is_handle() {
        return None;
    }
    // Read only what the handle actually holds in the side table (no deref of
    // the handle id as a heap object).
    own_symbol_property(req, sym_f64)
}

unsafe fn object_header_ptr_from_value_bits(bits: u64) -> Option<usize> {
    let (raw, obj_type) = heap_ptr_and_type_from_value_bits(bits)?;
    if obj_type == crate::gc::GC_TYPE_OBJECT {
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
    } else if top16 == 0 {
        bits as usize
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
            crate::proxy::proxy_get_from_prototype(proxy, sym_f64, receiver)
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
                if kind != crate::gc::GC_TYPE_ARRAY {
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
        if crate::proxy::js_proxy_is_proxy(proto) != 0 {
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
            if kind == 1 {
                let mname = b"values";
                return Some(crate::object::js_class_method_bind(
                    obj_f64,
                    mname.as_ptr(),
                    mname.len(),
                ));
            }
            return Some(f64::from_bits(TAG_UNDEFINED));
        }
    }

    let to_string_tag = well_known_symbol("toStringTag");
    if !to_string_tag.is_null() {
        let to_string_tag_f64 =
            f64::from_bits(crate::value::JSValue::pointer(to_string_tag as *const u8).bits());
        if sym_key == sym_key_from_f64(to_string_tag_f64) {
            let tag = match kind {
                1 => "ReadableStream",
                2 => "WritableStream",
                5 => "TransformStream",
                _ => return Some(f64::from_bits(TAG_UNDEFINED)),
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
    // A Proxy is a small registered id (its band overlaps the small-handle
    // band); dereferencing it as a heap object to read a symbol-keyed property
    // is an EXC_BAD_ACCESS. Route a SYMBOL-keyed read through the proxy `get`
    // trap (which forwards to the target). drizzle's aliased-column proxies are
    // read with symbol keys (`col[entityKind]`, `col[Table.Symbol.*]`) while
    // building a relational query.
    if crate::proxy::js_proxy_is_proxy(obj_f64) != 0 {
        return crate::proxy::proxy_get_with_receiver(obj_f64, sym_f64, receiver_f64);
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
    // Check the class's static symbols first when receiver is a class ref
    // (top16 == 0x7FFE, INT32_TAG).
    let bits = obj_f64.to_bits();
    if let Some(class_id) = crate::object::class_value::legacy_class_value_word(bits) {
        // A legacy class word names the same function object as its minted
        // pointer. Resolve its own bag through the ordinary symbol slot
        // evaluator before consulting declaration metadata.
        if let Some(owner) = crate::object::class_value::class_value_if_minted(class_id) {
            let owner = f64::from_bits(crate::JSValue::pointer(owner.cast()).bits());
            if let Some(value) = own_symbol_property_for_receiver(owner, sym_f64, receiver_f64) {
                return value;
            }
        }
        let sym_key = sym_key_from_f64(sym_f64);
        if sym_key != 0 {
            if let Some(v) =
                crate::object::class_symbol_getter_value(class_id, sym_key, receiver_f64, true)
            {
                return v;
            }
        }
        let static_lookup = if crate::object::class_prototype_ref_id(obj_f64).is_some() {
            class_static_symbol_lookup(class_id, sym_f64)
        } else {
            super::class_static_symbol_lookup_in_chain(class_id, sym_f64)
        };
        if let Some(vb) = static_lookup {
            return f64::from_bits(vb);
        }
        // #9101: statically-known well-known-symbol METHODS are registered
        // under synthetic names (`@@toPrimitive`, etc.), rather than in the
        // class-static-symbol data-property table above. Reify a static method
        // value with the constructor ClassRef as its receiver. This also makes
        // an explicit `C[Symbol.toPrimitive]` read agree with the ToPrimitive
        // consumer instead of returning undefined.
        if let Some(method_name) = well_known_symbol_method_key(sym_f64) {
            if crate::object::lookup_static_method_in_chain(class_id, method_name).is_some() {
                return crate::object::js_class_method_bind(
                    obj_f64,
                    method_name.as_ptr(),
                    method_name.len(),
                );
            }
        }
        // #6173: a `static [S]() {}` (and, through a prototype ref, an
        // instance `[S]() {}`) registers in CLASS_SYMBOL_METHODS, which this
        // resolver never consulted — so reading `D[S]` as a VALUE returned
        // undefined even though the direct call `D[S]()` dispatched fine via
        // `js_native_call_method_value`'s independent lookup. Materialize a
        // bound-method closure carrying the resolved target. Runs after the
        // accessor branch (getter priority) and the static symbol-FIELD
        // lookup (a `static [S] = v` initializer runs after method
        // installation and shadows the method, matching class-init order).
        // USER symbols only: well-known symbol methods (`[Symbol.iterator]`,
        // `[Symbol.toPrimitive]`, …) are lowered to synthetic `@@name`
        // members with dedicated consumers (GetIterator, `js_to_primitive`,
        // the using-block desugar) and established name-based resolution —
        // keep them on those paths rather than changing their behavior here.
        //
        // A STATIC well-known-symbol method with no synthetic `@@name` slot
        // (`static *[Symbol.iterator]() {}` — the alias above is instance-only)
        // lives only in this table; the `@@name` probe just above already
        // missed, so consult it too (#11170). Prototype refs keep the
        // user-symbol-only rule.
        let is_proto_ref_receiver = crate::object::class_prototype_ref_id(obj_f64).is_some();
        if sym_key != 0 && (!crate::symbol::is_well_known_symbol(sym_key) || !is_proto_ref_receiver)
        {
            let is_proto_ref = is_proto_ref_receiver;
            if let Some((func_ptr, param_count, has_rest, _entry)) =
                crate::object::lookup_class_symbol_method_in_chain(class_id, sym_key, !is_proto_ref)
            {
                return crate::object::build_symbol_bound_method_closure(
                    obj_f64,
                    func_ptr,
                    param_count,
                    has_rest,
                    !is_proto_ref,
                    &crate::symbol::symbol_function_name(sym_key),
                );
            }
        }
        // A constructor whose heritage was a runtime value inherits from that
        // exact value, the same edge `Object.getPrototypeOf(C)` reports. A
        // fresh class evaluation is not reachable through the template-id
        // tables below, so continue the ordinary [[Get]] on the actual parent.
        if !is_proto_ref_receiver {
            let parent = crate::object::js_get_dynamic_parent_value(class_id);
            if crate::object::is_class_object_value(parent) {
                return js_object_get_symbol_property_with_receiver(parent, sym_f64, receiver_f64);
            }
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
        // answers `X`, not `Array`. Statics only: a prototype ref shares this
        // tag, and `X.prototype` must not see the constructor's symbols.
        if !is_proto_ref_receiver {
            if let Some(parent_ctor) = crate::object::builtin_parent_ctor_in_chain(class_id) {
                return js_object_get_symbol_property_with_receiver(
                    parent_ctor,
                    sym_f64,
                    receiver_f64,
                );
            }
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
            && crate::proxy::js_proxy_is_proxy(obj_f64) == 0
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
    // #5437 (Next.js): a heap request *wrapper* whose own symbol entry misses
    // may be one of several `NodeNextRequest`s wrapping the SAME underlying
    // native IncomingMessage handle (stored in its `_req` field). Node shares
    // one per-request metadata object by reference across every such wrapper —
    // the wrapper's ctor does `this[SYM] = this._req[SYM] || {}`. When that
    // share didn't land on this particular wrapper (a late SSR-bundled copy
    // never had its `[SYM]` seeded), fall through to the underlying handle's
    // symbol meta so the read still observes the shared object, matching Node.
    // Gated tightly: only fires on a side-table MISS, only when `_req` resolves
    // to a small native handle (POINTER-tagged, below HANDLE_BAND_MAX, not a
    // real heap object), and only returns a value the handle actually holds —
    // so ordinary objects (no `_req`, or a heap `_req`) are unaffected.
    // #7498: `req_handle_symbol_fallback` interns a `"_req"` key, so it is the
    // first unconditional allocation on this resolver's path — every
    // `[Symbol.iterator]` read behind a spread reaches it. Root the receiver
    // and the symbol across it and rebind BOTH `obj_f64` and the derived
    // `bits` from the roots afterwards, so no line below can name a
    // pre-collection address.
    let scope = crate::gc::RuntimeHandleScope::new();
    let obj_h = scope.root_nanbox_f64(obj_f64);
    let sym_h = scope.root_nanbox_f64(sym_f64);
    let receiver_h = scope.root_nanbox_f64(receiver_f64);
    if let Some(v) = req_handle_symbol_fallback(obj_h.get_nanbox_f64(), sym_h.get_nanbox_f64()) {
        return v;
    }
    let obj_f64 = obj_h.get_nanbox_f64();
    let sym_f64 = sym_h.get_nanbox_f64();
    let receiver_f64 = receiver_h.get_nanbox_f64();
    let bits = obj_f64.to_bits();
    let sym_key = sym_key_from_f64(sym_f64);
    if sym_key != 0 {
        let jsval = crate::value::JSValue::from_bits(bits);
        if jsval.is_pointer() {
            let ptr = jsval.as_pointer::<crate::object::ObjectHeader>();
            if !ptr.is_null() && crate::object::is_valid_obj_ptr(ptr as *const u8) {
                let class_id = crate::object::js_object_get_class_id(ptr);
                if class_id != 0 && crate::object::shapes::receiver_has_class_identity(ptr) {
                    // Computed Symbol ClassBody members are real properties of
                    // the declared prototype shape. Materialize that shape and
                    // resolve it before the compiled dispatch registries: an
                    // absent entry can then mean the configurable property was
                    // deleted, rather than being silently resurrected here.
                    if crate::object::class_has_symbol_member_in_chain(class_id, sym_key, false) {
                        if let Some(value) = generator::explicit_generator_method(
                            class_id,
                            sym_key,
                            obj_f64,
                            sym_f64,
                            receiver_f64,
                        ) {
                            return value;
                        }
                        let _ = crate::object::class_decl_prototype_value(class_id);
                        if let Some(v) = declared_prototype_chain_symbol(
                            obj_f64,
                            sym_f64,
                            class_id,
                            receiver_f64,
                        ) {
                            return v;
                        }
                        if let Some(v) =
                            resolve_explicit_object_prototype_symbol(obj_f64, sym_f64, receiver_f64)
                        {
                            return v;
                        }
                        return f64::from_bits(TAG_UNDEFINED);
                    }
                    if let Some(v) = crate::object::class_symbol_getter_value(
                        class_id,
                        sym_key,
                        receiver_f64,
                        false,
                    ) {
                        return v;
                    }
                    // #5128: a symbol-keyed instance METHOD — `*[Symbol.iterator]()`
                    // (and `[Symbol.asyncIterator]()`) are registered on the class
                    // under the synthetic names `@@iterator` / `@@asyncIterator`.
                    // Read the method off the class and return a bound method so
                    // iteration-protocol consumers (`[...x]`, `for…of`,
                    // `Math.max(...x)`, destructuring) can drive `.next()`. Guard
                    // on `class_method_slot_owner` first: `js_class_method_bind`
                    // otherwise mints a bound closure for a non-existent method.
                    if let Some(method_name) = well_known_symbol_method_name(sym_key) {
                        if let Some(owner) =
                            crate::object::class_method_slot_owner(class_id, method_name)
                        {
                            if let Some(value) = class_iterator_prototype_override(
                                receiver_f64,
                                sym_f64,
                                class_id,
                                owner,
                            ) {
                                return value;
                            }
                            return crate::object::js_class_method_bind(
                                obj_f64,
                                method_name.as_ptr(),
                                method_name.len(),
                            );
                        }
                    }
                    // #6173: a USER symbol-keyed instance method (`[S]() {}`)
                    // lives in CLASS_SYMBOL_METHODS — the table the direct
                    // call path resolves through — not in the accessor /
                    // well-known tables checked above, so a bare `obj[S]`
                    // read returned undefined while `obj[S]()` worked.
                    // Materialize the resolved target as a bound method. Own
                    // symbol props (checked earlier) still shadow it, and the
                    // accessor branch above keeps getter priority. This also
                    // fixes instance-side `S in obj`, whose presence check
                    // (`js_object_has_property`) delegates to this resolver.
                    // USER symbols only: a well-known computed method (e.g.
                    // `[Symbol.toPrimitive]() {}`) is lowered to a synthetic
                    // `@@name` vtable member and keeps resolving through the
                    // name-based #1838 tail below, preserving the existing
                    // behavior for every well-known symbol that HAS one.
                    // A well-known symbol with no synthetic slot
                    // (`[Symbol.hasInstance]() {}` on an instance,
                    // `[Symbol.split]() {}`, …) lives only in this table, so
                    // skipping it made `obj[Symbol.hasInstance]` undefined and
                    // the call return undefined (#11170).
                    if !crate::symbol::is_well_known_symbol(sym_key)
                        || !well_known_symbol_has_instance_method_slot(sym_key)
                    {
                        if let Some((func_ptr, param_count, has_rest, _entry)) =
                            crate::object::lookup_class_symbol_method_in_chain(
                                class_id, sym_key, false,
                            )
                        {
                            return crate::object::build_symbol_bound_method_closure(
                                obj_f64,
                                func_ptr,
                                param_count,
                                has_rest,
                                false,
                                &crate::symbol::symbol_function_name(sym_key),
                            );
                        }
                    }
                }
            }
        }
    }
    if let Some(v) = resolve_explicit_object_prototype_symbol(obj_f64, sym_f64, receiver_f64) {
        return v;
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
    if raw_ptr >= 0x10000 && crate::buffer::is_registered_buffer(raw_ptr) {
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
    if raw_addr >= 0x1000 && crate::buffer::is_registered_buffer(raw_addr) {
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
    if raw_addr >= 0x1000 && crate::typedarray::lookup_typed_array_kind(raw_addr).is_some() {
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
                // #1838: a class can define a computed well-known-symbol METHOD
                // (`[Symbol.iterator]() {}`) — class lowering names it
                // `@@iterator` in the vtable (class_members.rs), NOT as a symbol
                // property, so the proto-chain symbol walk above misses it. Map
                // the well-known symbol back to its `@@name`, and if the class
                // (or an ancestor) has that method, return it bound to the
                // instance. This is how effect's `EffectPrimitive` exposes
                // `Symbol.iterator` (→ `SingleShotGen`), so `yield* effectValue`
                // / `Symbol.iterator in effectValue` resolve.
                if let Some(at_name) = well_known_symbol_method_key(sym_f64) {
                    if crate::object::shapes::receiver_has_class_identity(obj_ptr)
                        && class_chain_has_method(cid, at_name)
                    {
                        return crate::object::js_class_method_bind(
                            obj_f64,
                            at_name.as_ptr(),
                            at_name.len(),
                        );
                    }
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
    if header.obj_type != crate::gc::GC_TYPE_OBJECT
        || header.gc_flags & crate::gc::GC_FLAG_FORWARDED != 0
    {
        return false;
    }
    let class_id = (*ptr).class_id;
    class_id != 0 && declared_prototype_symbol_holder(receiver, sym, class_id).is_some()
}

/// #1838: map a well-known symbol value to the synthetic `@@<name>` vtable key
/// that class lowering assigns to a computed `[Symbol.X]() {}` method (see
/// `lower_decl/class_members.rs`). Returns `None` for symbols that don't name a
/// class method (or non-symbol values). `dispose`/`asyncDispose` use distinct
/// `__perry_*__` names and are dispatched via the using-block desugarer, so
/// they're deliberately excluded here.
unsafe fn well_known_symbol_method_key(sym_f64: f64) -> Option<&'static str> {
    let sk = sym_key_from_f64(sym_f64);
    if sk == 0 {
        return None;
    }
    for (short, at_name) in [
        ("iterator", "@@iterator"),
        ("asyncIterator", "@@asyncIterator"),
        ("hasInstance", "@@hasInstance"),
        ("toPrimitive", "@@toPrimitive"),
        ("toStringTag", "@@toStringTag"),
    ] {
        let wk = well_known_symbol(short);
        if !wk.is_null() {
            let wk_f64 = f64::from_bits(crate::value::JSValue::pointer(wk as *const u8).bits());
            if sym_key_from_f64(wk_f64) == sk {
                return Some(at_name);
            }
        }
    }
    None
}

/// Does a computed INSTANCE method keyed by this well-known symbol also get a
/// synthetic vtable slot that the name-based resolvers read? Keep in sync with
/// the alias list in `js_register_class_computed_method` (and the static
/// lowering's renames): `iterator`, `asyncIterator`, `toPrimitive`, `dispose`,
/// `asyncDispose`. Every other well-known symbol's instance method is
/// reachable ONLY through `CLASS_SYMBOL_METHODS` (#11170).
fn well_known_symbol_has_instance_method_slot(sym_key: usize) -> bool {
    [
        "iterator",
        "asyncIterator",
        "toPrimitive",
        "dispose",
        "asyncDispose",
    ]
    .iter()
    .any(|wk| {
        let s = well_known_symbol(wk);
        // SAFETY: `s` is a live well-known symbol from the process-lifetime
        // cache; `sym_key_from_f64` only decodes its NaN-boxed pointer.
        !s.is_null()
            && unsafe {
                sym_key_from_f64(f64::from_bits(
                    crate::value::JSValue::pointer(s as *const u8).bits(),
                ))
            } == sym_key
    })
}

/// #1838: does `class_id` or any ancestor define a vtable method named `name`?
fn class_chain_has_method(class_id: u32, name: &str) -> bool {
    let mut cid = class_id;
    let mut depth = 0usize;
    while depth < 32 && cid != 0 {
        if crate::object::class_has_own_method(cid, name) {
            return true;
        }
        match crate::object::get_parent_class_id(cid) {
            Some(p) if p != 0 && p != cid => {
                cid = p;
                depth += 1;
            }
            _ => break,
        }
    }
    false
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
    // bits are invariant across any collection. The #5437 fix logic (no-op on undefined, `_req` fallthrough)
    // is value-agnostic, so a number tests it faithfully without depending on
    // GC suppression to keep a heap `meta` from moving.
    fn immovable_meta() -> f64 {
        // A normal finite double whose bits are stable and unambiguous.
        1234.5_f64
    }

    #[test]
    fn wrapper_reads_share_underlying_handle_meta() {
        unsafe {
            // Compact the arena up front with a full GC so the few small
            // allocations this test makes can't trip a mid-test block-alloc GC
            // (which bypasses `gc_suppress`). `gc_suppress` is kept as belt-and-
            // braces; the immovable number `meta` makes the assertion itself
            // GC-invariant regardless.
            crate::gc::js_gc_collect();
            crate::gc::gc_suppress();
            let sym = next_request_meta_symbol();
            // Pick a handle id well inside the small-handle band but unlikely to
            // collide with another test's side-table entry.
            let owner =
                crate::object::handle_expando::test_support::TestSmallHandleOwner::new(0x321);
            let handle = owner.value();

            // The per-request metadata value lives on the handle. Use an
            // immovable number so a GC can't invalidate the comparison.
            let meta = immovable_meta();
            super::properties::js_object_set_symbol_property(handle, sym, meta);
            // #10510: the install on a handle owner arms the `_req` fallback,
            // which is skipped entirely until then.
            assert!(crate::symbol::small_handle_symbol_owner_ever());

            // A heap wrapper that aliases the handle via `_req` but never had
            // its own `[sym]` seeded.
            let wrapper_obj = crate::object::js_object_alloc(0, 1);
            assert!(!wrapper_obj.is_null());
            let req_key = js_string_from_bytes(b"_req".as_ptr(), 4);
            crate::object::js_object_set_field_by_name(wrapper_obj, req_key, handle);
            let wrapper = crate::value::js_nanbox_pointer(wrapper_obj as i64);

            // Reading the symbol off the wrapper falls through to the handle's
            // shared meta — the exact Node-by-reference semantics.
            let got = js_object_get_symbol_property(wrapper, sym);
            // Unsuppress before asserting so a panic can't leave GC suppressed
            // for sibling tests on this thread.
            crate::gc::gc_unsuppress();
            assert_eq!(
                got.to_bits(),
                meta.to_bits(),
                "wrapper symbol read should share the handle's meta value"
            );
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
