//! Symbol-keyed property reads: the `js_object_get_symbol_property` resolver
//! and its prototype-chain / well-known-symbol / handle helpers.

use super::*;
use crate::string::{js_string_from_bytes, StringHeader};
use std::collections::{HashMap, HashSet};
use std::sync::Mutex;

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

/// #1758: the OWN symbol-property lookup — the raw `SYMBOL_PROPERTIES`
/// side-table read keyed by the object's address (no class-ref / no prototype
/// chain). Used by `js_object_get_symbol_property` and by
/// `resolve_proto_chain_symbol`, which walks prototype objects itself and must
/// therefore NOT recurse into the full chain-walking getter.
pub(crate) unsafe fn own_symbol_property(obj_f64: f64, sym_f64: f64) -> Option<f64> {
    if let Some(acc) = accessors::symbol_accessor_property(obj_f64, sym_f64) {
        if acc.get != 0 {
            let closure =
                (acc.get & crate::value::POINTER_MASK) as *const crate::closure::ClosureHeader;
            if !closure.is_null() {
                return Some(crate::closure::js_closure_call0(closure));
            }
        }
        return Some(f64::from_bits(TAG_UNDEFINED));
    }
    let obj_key = obj_key_from_f64(obj_f64);
    let sym_key = sym_key_from_f64(sym_f64);
    if obj_key == 0 || sym_key == 0 {
        return None;
    }
    let guard = crate::gc::lock_gc_root_registry(&SYMBOL_PROPERTIES);
    if let Some(map) = guard.as_ref() {
        if let Some(entries) = map.get(&obj_key) {
            for &(sk, vb) in entries.iter() {
                if sk == sym_key {
                    return Some(f64::from_bits(vb));
                }
            }
        }
    }
    None
}

unsafe fn object_header_ptr_from_value_bits(bits: u64) -> Option<usize> {
    let top16 = bits >> 48;
    let raw = if top16 == 0x7FFD {
        (bits & POINTER_MASK) as usize
    } else if top16 == 0 {
        bits as usize
    } else {
        return None;
    };
    if raw < crate::gc::GC_HEADER_SIZE + 0x1000 {
        return None;
    }
    let header_addr = raw - crate::gc::GC_HEADER_SIZE;
    let gc_header = header_addr as *const crate::gc::GcHeader;
    let tracked_malloc = crate::gc::gc_malloc_header_is_tracked(gc_header);
    let arena_payload = !matches!(
        crate::arena::classify_heap_space(raw),
        crate::arena::HeapSpace::Unknown
    );
    let arena_header = !matches!(
        crate::arena::classify_heap_space(header_addr),
        crate::arena::HeapSpace::Unknown
    );
    if !tracked_malloc && !(arena_payload && arena_header) {
        return None;
    }
    if (*gc_header).obj_type == crate::gc::GC_TYPE_OBJECT {
        Some(raw)
    } else {
        None
    }
}

unsafe fn resolve_explicit_object_prototype_symbol(obj_f64: f64, sym_f64: f64) -> Option<f64> {
    const TAG_NULL: u64 = 0x7FFC_0000_0000_0002;
    let mut owner = object_header_ptr_from_value_bits(obj_f64.to_bits())?;
    for _ in 0..8 {
        let proto_bits = crate::object::prototype_chain::object_static_prototype(owner)?;
        if proto_bits == TAG_NULL {
            return None;
        }
        let proto_f64 = f64::from_bits(proto_bits);
        if let Some(v) = own_symbol_property(proto_f64, sym_f64) {
            return Some(v);
        }
        let proto_ptr = object_header_ptr_from_value_bits(proto_bits)?;
        if proto_ptr == owner {
            return None;
        }
        let proto_obj = proto_ptr as *const crate::object::ObjectHeader;
        let cid = crate::object::js_object_get_class_id(proto_obj);
        if cid != 0 {
            if let Some(v) = crate::object::resolve_proto_chain_symbol(cid, sym_f64) {
                return Some(v);
            }
        }
        owner = proto_ptr;
    }
    None
}

unsafe fn web_stream_symbol_property(obj_f64: f64, sym_f64: f64) -> Option<f64> {
    if !obj_f64.is_finite() || obj_f64 <= 0.0 || obj_f64.fract() != 0.0 {
        return None;
    }
    let kind_probe = crate::object::stream_handle_kind_probe()?;
    let kind = kind_probe(obj_f64 as usize);
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

#[no_mangle]
pub unsafe extern "C" fn js_object_get_symbol_property(obj_f64: f64, sym_f64: f64) -> f64 {
    // A Proxy is a small registered id (its band overlaps the small-handle
    // band); dereferencing it as a heap object to read a symbol-keyed property
    // is an EXC_BAD_ACCESS. Route a SYMBOL-keyed read through the proxy `get`
    // trap (which forwards to the target). drizzle's aliased-column proxies are
    // read with symbol keys (`col[entityKind]`, `col[Table.Symbol.*]`) while
    // building a relational query.
    if crate::proxy::js_proxy_is_proxy(obj_f64) != 0 {
        return crate::proxy::js_proxy_get(obj_f64, sym_f64);
    }
    // Check CLASS_STATIC_SYMBOLS first when receiver is a class ref
    // (top16 == 0x7FFE, INT32_TAG).
    let bits = obj_f64.to_bits();
    if (bits >> 48) == 0x7FFE {
        let class_id = (bits & 0xFFFF_FFFF) as u32;
        let sym_key = sym_key_from_f64(sym_f64);
        if sym_key != 0 {
            if let Some(v) =
                crate::object::class_symbol_getter_value(class_id, sym_key, obj_f64, true)
            {
                return v;
            }
        }
        if let Some(vb) = class_static_symbol_lookup(class_id, sym_f64) {
            return f64::from_bits(vb);
        }
        // #1758: a class ref whose own static symbols miss may inherit the
        // symbol from a class-expression parent (`class Sub extends make(...) {}`
        // → `Sub[TypeId]`). Walk the CLASS_PROTOTYPE_OBJECTS chain.
        if let Some(v) = crate::object::resolve_proto_chain_symbol(class_id, sym_f64) {
            return v;
        }
        // #36 / #321: the subclass extends a FUNCTION value
        // (`class Svc extends Context.Tag(id)<...>() {}`). Read the symbol off
        // the parent closure — own symbol props plus, via the closure symbol
        // getter, its static prototype (`Svc[TagTypeId]`/`Svc[EffectTypeId]`
        // live on TagProto). Recurse into the closure-aware getter so its proto
        // walk fires.
        if let Some(closure_ptr) = crate::object::class_parent_closure(class_id) {
            let closure_f64 =
                f64::from_bits(crate::value::js_nanbox_pointer(closure_ptr as i64).to_bits());
            let v = js_object_get_symbol_property(closure_f64, sym_f64);
            if v.to_bits() != TAG_UNDEFINED {
                return v;
            }
        }
        return f64::from_bits(TAG_UNDEFINED);
    }
    // #1545: Web Stream handles are normal finite numbers, not heap objects.
    // Resolve their well-known symbol surface before pointer-oriented fallback
    // paths reinterpret the raw f64 bits as an address. ReadableStream is
    // async-iterable only; none of the Web Stream handles expose
    // `Symbol.iterator`.
    if let Some(v) = web_stream_symbol_property(obj_f64, sym_f64) {
        return v;
    }
    // #1213: Timeout/Immediate handles expose `Symbol.dispose` so
    // `using t = setTimeout(...)` and `t[Symbol.dispose]()` clear the timer.
    // The handle is a small id NaN-boxed as POINTER; the symbol-keyed read
    // otherwise misses the side table and returns undefined.
    if (bits >> 48) == 0x7FFD {
        let id = (bits & 0x0000_FFFF_FFFF_FFFF) as i64;
        if crate::value::addr_class::is_small_handle(id as usize)
            && crate::timer::is_known_timer_id(id)
        {
            let dispose = well_known_symbol("dispose");
            if !dispose.is_null() {
                let dispose_f64 =
                    f64::from_bits(crate::value::JSValue::pointer(dispose as *const u8).bits());
                if sym_key_from_f64(sym_f64) == sym_key_from_f64(dispose_f64) {
                    let mname = b"@@__perry_wk_dispose";
                    return crate::object::js_class_method_bind(
                        obj_f64,
                        mname.as_ptr(),
                        mname.len(),
                    );
                }
            }
        }
    }
    // Generic small-handle `Symbol.dispose` support. Subsystems that expose
    // a dispose method through HANDLE_PROPERTY_DISPATCH can bind it here
    // without adding a runtime-specific special case.
    if (bits >> 48) == 0x7FFD {
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
    if (bits >> 48) == 0x7FFD {
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
    // Web Fetch and other stdlib handle-backed values are small ids
    // NaN-boxed as POINTER. A computed `handle[Symbol.iterator]` reaches the
    // symbol resolver directly, bypassing the normal string-key handle
    // property dispatcher. Map the well-known symbol back to the dispatcher so
    // `Headers` can expose its `entries` method as the iterator function.
    if (bits >> 48) == 0x7FFD {
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
    if (bits >> 48) == 0x7FFD {
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
        return accessors::invoke_symbol_accessor_getter(acc.get, obj_f64);
    }
    if let Some(v) = own_symbol_property(obj_f64, sym_f64) {
        return v;
    }
    let sym_key = sym_key_from_f64(sym_f64);
    if sym_key != 0 {
        let jsval = crate::value::JSValue::from_bits(bits);
        if jsval.is_pointer() {
            let ptr = jsval.as_pointer::<crate::object::ObjectHeader>();
            if !ptr.is_null() && crate::object::is_valid_obj_ptr(ptr as *const u8) {
                let class_id = crate::object::js_object_get_class_id(ptr);
                if class_id != 0 {
                    if let Some(v) =
                        crate::object::class_symbol_getter_value(class_id, sym_key, obj_f64, false)
                    {
                        return v;
                    }
                    // #5128: a symbol-keyed instance METHOD — `*[Symbol.iterator]()`
                    // (and `[Symbol.asyncIterator]()`) are registered on the class
                    // under the synthetic names `@@iterator` / `@@asyncIterator`.
                    // Read the method off the class and return a bound method so
                    // iteration-protocol consumers (`[...x]`, `for…of`,
                    // `Math.max(...x)`, destructuring) can drive `.next()`. Guard
                    // on `method_owner_class_id` first: `js_class_method_bind`
                    // otherwise mints a bound closure for a non-existent method.
                    if let Some(method_name) = well_known_symbol_method_name(sym_key) {
                        if crate::object::method_owner_class_id(class_id, method_name).is_some() {
                            return crate::object::js_class_method_bind(
                                obj_f64,
                                method_name.as_ptr(),
                                method_name.len(),
                            );
                        }
                    }
                }
            }
        }
    }
    if let Some(v) = resolve_explicit_object_prototype_symbol(obj_f64, sym_f64) {
        return v;
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
    // #36 / #321: the receiver is a closure whose OWN symbol props miss — walk
    // its static prototype chain (`Object.setPrototypeOf(closure, protoObj)`).
    // effect's `TagClass[TagTypeId]` / `isTag(TagClass)` read symbols off
    // `TagProto`. Bounded depth guards against an accidental cycle.
    if (bits >> 48) == 0x7FFD {
        let ptr = crate::value::js_nanbox_get_pointer(obj_f64) as usize;
        if ptr != 0 && crate::closure::is_closure_ptr(ptr) {
            let mut cur = ptr;
            let mut depth = 0usize;
            while depth < 8 {
                let Some(proto_bits) = crate::closure::closure_static_prototype(cur) else {
                    break;
                };
                let proto_f64 = f64::from_bits(proto_bits);
                let proto_ptr = crate::value::js_nanbox_get_pointer(proto_f64) as usize;
                if proto_ptr == 0 || proto_ptr == cur {
                    break;
                }
                if let Some(v) = own_symbol_property(proto_f64, sym_f64) {
                    return v;
                }
                // A class-object proto may carry the symbol through ITS own
                // class_id prototype chain (effect's TagProto spreads
                // EffectPrototype). Walk that before following the closure link.
                let proto_obj = crate::value::JSValue::from_bits(proto_bits)
                    .as_pointer::<crate::object::ObjectHeader>();
                if !proto_obj.is_null() {
                    let cid = crate::object::js_object_get_class_id(proto_obj);
                    if cid != 0 {
                        if let Some(v) = crate::object::resolve_proto_chain_symbol(cid, sym_f64) {
                            return v;
                        }
                    }
                }
                if crate::closure::is_closure_ptr(proto_ptr) {
                    cur = proto_ptr;
                    depth += 1;
                    continue;
                }
                break;
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
                if let Some(v) = own_symbol_property(func_proto, sym_f64) {
                    return v;
                }
            }
        }
    }
    // Buffers inherit TypedArray iteration semantics in Node: the default
    // iterator is `values()`, yielding numeric bytes.
    let raw_addr = if (bits >> 48) >= 0x7FF8 {
        (bits & POINTER_MASK) as usize
    } else {
        bits as usize
    };
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
    // #321: arrays expose `Symbol.iterator`. perry has no standalone array
    // iterator object (for-of is special-cased), but `arr[Symbol.iterator]`
    // must resolve to a callable so `Symbol.iterator in arr` is true
    // (effect's `Predicate.isIterable`) and `typeof arr[Symbol.iterator]` is
    // "function". Bind the array's `values` method as that callable. Pre-fix
    // the symbol key fell through to the numeric/string paths and read back a
    // number, so `isIterable([...])` was false and `Effect.all`'s
    // predicate-`dual` `forEach` went data-last (returned a function).
    if crate::array::js_array_is_array(obj_f64).to_bits() == crate::value::TAG_TRUE {
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
                if let Some(v) = crate::object::resolve_proto_chain_symbol(cid, sym_f64) {
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
                    if class_chain_has_method(cid, at_name) {
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
    f64::from_bits(TAG_UNDEFINED)
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
