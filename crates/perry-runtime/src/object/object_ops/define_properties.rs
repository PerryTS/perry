//! `Object.defineProperties` and `Object.setPrototypeOf`.
use super::*;

/// Snapshot all keys, collect current enumerable descriptors, then apply.
///
/// Both key snapshots are fresh arrays (`getOwnPropertyNames` /
/// `getOwnPropertySymbols`, or the Proxy's validated `ownKeys` list) rooted
/// before any per-key callback. Each collected definition is a contiguous
/// record of rooted handles in this operation's own scope
/// (`DescView::collected_words`), so the operation owns no Rust-heap buffer:
/// an abrupt completion leaves nothing to release, and the thrower's
/// savepoint truncates the records with the rest of the handle stack. No
/// protected frame is needed.
#[no_mangle]
pub extern "C" fn js_object_define_properties(target: f64, properties: f64) -> f64 {
    unsafe {
        if !definition_target_is_object(target) {
            throw_object_type_error(b"Object.defineProperties called on non-object");
        }
        let scope = crate::gc::RuntimeHandleScope::new();
        // Legacy raw object operands are admitted at this public boundary only;
        // normalize them before any enumeration or descriptor callback.
        let target = scope.root_nanbox_f64(normalize_descriptor_operand(target));
        let properties = scope.root_nanbox_f64(normalize_descriptor_operand(properties));
        let value = f64::from_bits(properties.get_nanbox_u64());
        if matches!(
            value.to_bits(),
            crate::value::TAG_NULL | crate::value::TAG_UNDEFINED
        ) {
            throw_object_type_error(b"Cannot convert undefined or null to object");
        }
        let properties = scope.root_nanbox_u64(if definition_target_is_object(value) {
            value.to_bits()
        } else {
            super::super::js_object_coerce(value).to_bits()
        });
        let current_properties = || f64::from_bits(properties.get_nanbox_u64());
        let proxy = crate::proxy::js_proxy_is_proxy(current_properties()) != 0;
        let names = if proxy {
            crate::proxy::js_proxy_own_keys(current_properties())
        } else {
            js_object_get_own_property_names(current_properties())
        };
        let names = scope.root_raw_mut_ptr(
            crate::value::js_nanbox_get_pointer(names) as *mut crate::array::ArrayHeader
        );
        // Root the name array while enumerating symbols; both snapshots are
        // complete before any per-key own-descriptor/Get callback.
        let symbols = if proxy {
            None
        } else {
            let raw = crate::symbol::js_object_get_own_property_symbols(current_properties());
            (raw != 0).then(|| scope.root_raw_mut_ptr(raw as *mut crate::array::ArrayHeader))
        };
        let length_of = |array: &crate::gc::RuntimeHandle<'_>| {
            array.with_const_ptr::<crate::array::ArrayHeader, _>(|array| {
                crate::array::js_array_length(array)
            })
        };
        let name_count = length_of(&names);
        let key_count = name_count + symbols.as_ref().map(length_of).unwrap_or(0);
        // Per-key work runs in its own scope; only the finished record is
        // pushed into `scope`, so records sit back to back from `base`.
        let base = crate::gc::runtime_handle_stack_savepoint();
        let mut entries = 0usize;
        for index in 0..key_count {
            let words = {
                let key_scope = crate::gc::RuntimeHandleScope::new();
                let key = key_scope.root_nanbox_f64(match (&symbols, index < name_count) {
                    (_, true) => names.with_const_ptr::<crate::array::ArrayHeader, _>(|array| {
                        crate::array::js_array_get_f64(array, index)
                    }),
                    (Some(symbols), false) => symbols
                        .with_const_ptr::<crate::array::ArrayHeader, _>(|array| {
                            crate::array::js_array_get_f64(array, index - name_count)
                        }),
                    (None, false) => unreachable!("key_count counts only snapshot entries"),
                });
                // `props.[[GetOwnProperty]](key)` read for its [[Enumerable]]
                // only: the source family answers from its own facts (a
                // Proxy runs its getOwnPropertyDescriptor trap exactly once),
                // so no reflection record is built per source key.
                if js_object_property_is_enumerable(current_properties(), key.get_nanbox_f64())
                    .to_bits()
                    != crate::value::TAG_TRUE
                {
                    continue;
                }
                // The Get result is a JSValue at rest: decode it as-is, never
                // through the raw-operand admission used for public operands.
                let bag = key_scope.root_nanbox_f64(super::super::js_object_get_property_key(
                    current_properties(),
                    key.get_nanbox_f64(),
                ));
                let descriptor = decode_property_descriptor(&key_scope, &bag);
                let words = descriptor.collected_words(key.get_nanbox_u64());
                words
            };
            // No allocation between reading the words and rooting them.
            for word in words {
                scope.root_nanbox_u64(word);
            }
            entries += 1;
        }
        for entry in 0..entries {
            let (key, descriptor) = DescView::collected_at(&scope, base, entry);
            if !define_own_property_decoded(&scope, &target, &key, &descriptor) {
                throw_definition_rejected(&scope, &target, &key);
            }
        }
        f64::from_bits(target.get_nanbox_u64())
    }
}

/// `Object.setPrototypeOf(obj, proto)` — chalk's callable-with-getter-bag
/// foundation. Perry's runtime bakes class IDs at allocation time (it
/// walks `parent_class_id` for INT32-tagged class refs), so we cannot
/// mutate an existing object's prototype chain in a fully observable
/// way. What we *can* do is satisfy the spec's "return target" contract
/// so callers like
///
/// ```text
/// const chalk = (...s) => s.join(' ');
/// Object.setPrototypeOf(chalk, Foo.prototype);
/// ```
///
/// don't crash with `TypeError: value is not a function` (which is what
/// the generic `(Object).setPrototypeOf(...)` PropertyGet → Call fallback
/// used to produce — the property lookup returned undefined and the call
/// dispatched a non-callable). chalk's module init invokes this exact
/// pattern; ms / express decorate functions with `Object.assign` instead,
/// which is already a fast path.
///
/// Pragmatically: today this returns the target and otherwise no-ops.
/// chalk's getters on `createChalk.prototype` won't actually fire under
/// Perry, but the rest of the program keeps running and chalk's
/// call-without-properties form (the most common usage) keeps working.
/// A future change can register the (obj → proto) mapping in a
/// thread-local side-table so a downstream `Object.getPrototypeOf(obj)`
/// + inherited property dispatch can consult it.
#[no_mangle]
pub extern "C" fn js_object_set_prototype_of(obj_value: f64, proto: f64) -> f64 {
    crate::array::subclass_elements::deopt_value(obj_value);
    const TAG_NULL: u64 = 0x7FFC_0000_0000_0002;
    const POINTER_TAG: u64 = 0x7FFD_0000_0000_0000;
    let obj_bits = obj_value.to_bits();
    let proto_bits = proto.to_bits();

    // A Proxy receiver is a small registered id, not a heap object — the
    // recording path below would deref the fake pointer and segfault. Route
    // through the Reflect entry (which resolves the proxy to its target and
    // runs the trap chain, recursing through proxy targets). `Object.setPrototypeOf`
    // must surface a `false` internal-method result as a `TypeError`
    // (`Reflect.setPrototypeOf` returns the boolean without throwing). Without
    // this, `Object.setPrototypeOf(proxyOfNonExtensibleProxy, x)` silently
    // succeeded instead of throwing (test262
    // Proxy/setPrototypeOf/trap-is-{missing,undefined}-target-is-proxy).
    if crate::proxy::js_proxy_is_proxy(obj_value) != 0 {
        let ok = crate::proxy::js_reflect_set_prototype_of(obj_value, proto);
        if crate::value::js_is_truthy(ok) == 0 {
            throw_object_type_error(b"#<Object> is not extensible");
        }
        return obj_value;
    }

    // #2820: `Object.setPrototypeOf(null | undefined, proto)` throws
    // `TypeError: Object.setPrototypeOf called on null or undefined`.
    {
        let jv = crate::value::JSValue::from_bits(obj_bits);
        if jv.is_null() || jv.is_undefined() {
            throw_object_type_error(b"Object.setPrototypeOf called on null or undefined");
        }
    }

    // #2820: `proto` must be an object or `null`. A primitive / undefined proto
    // throws `TypeError: Object prototype may only be an Object or null`. A
    // Symbol is pointer-tagged but is NOT an object, so reject it explicitly.
    let proto_is_null = proto_bits == TAG_NULL;
    let proto_is_symbol = unsafe { crate::symbol::js_is_symbol(proto) != 0 };
    let proto_ok = proto_is_null
        || crate::proxy::js_proxy_is_proxy(proto) != 0
        || (!proto_is_symbol
            && (unsafe { value_is_object_like(proto) }
                || super::super::class_ref_id(proto).is_some()));
    if !proto_ok {
        // V8 renders the offending value: `... an Object or null: 5`.
        let rendered = unsafe { describe_value_for_type_error(proto) };
        throw_object_type_error_with_suffix(
            "Object prototype may only be an Object or null: ",
            &rendered,
        );
    }

    // OrdinarySetPrototypeOf: a non-extensible target rejects a *changing*
    // prototype. `Object.setPrototypeOf` surfaces that rejection as a
    // TypeError; `Reflect.setPrototypeOf` returns `false` without throwing
    // (handled in js_reflect_set_prototype_of, which never reaches here for the
    // reject case). A no-op set to the SAME prototype still succeeds. Primitive
    // targets are extensible-irrelevant — `obj_value_no_extend` is false for
    // non-objects, so they fall through to the no-op return below. (test262
    // Reflect/preventExtensions/prevent-extensions:
    // `Object.setPrototypeOf(o, Array.prototype)` after preventExtensions.)
    if crate::object::obj_value_no_extend(obj_value) {
        let current = js_object_get_prototype_of(obj_value);
        if current.to_bits() != proto_bits {
            throw_object_type_error(b"#<Object> is not extensible");
        }
        return obj_value;
    }

    // OrdinarySetPrototypeOf step 7: detect prototype cycles.
    // Walk the prototype chain of the proposed new prototype; if any ancestor
    // equals the target object, setting the prototype would form a cycle.
    // Use Floyd's tortoise-and-hare so a pre-existing multi-node cycle in the
    // chain (A→B→A) terminates instead of looping forever. The `tortoise`
    // advances one step; the `hare` advances two. If they meet, the chain is
    // cyclic and contains a loop (so it will never reach null), meaning we also
    // can't form a fresh cycle by setting obj's proto to `proto`.
    if !proto_is_null {
        const TAG_NULL_U64: u64 = 0x7FFC_0000_0000_0002;
        const TAG_UNDEFINED_U64: u64 = 0x7FFC_0000_0000_0001;
        let advance = |bits: u64| -> u64 {
            let val = f64::from_bits(bits);
            // OrdinarySetPrototypeOf step 7.b.ii.1: if `p`'s [[GetPrototypeOf]]
            // is not the ordinary internal method (a Proxy's is exotic — it may
            // run arbitrary trap code), the walk stops here without invoking it.
            // Without this guard the cycle-detection walk called the target's
            // `getPrototypeOf` trap as a side effect of unrelated cycle-safety
            // bookkeeping (test262 has/call-in-prototype-index.js,
            // set/call-parameters-prototype-index.js observe a `getPrototypeOf`
            // trap the test handler never installs).
            if crate::proxy::js_proxy_is_proxy(val) != 0 {
                return TAG_NULL_U64;
            }
            let next = js_object_get_prototype_of(val);
            let nb = next.to_bits();
            // Treat undefined as chain-end like null: `js_object_get_prototype_of`
            // returns undefined (not spec's object-or-null) for some exotic
            // receivers, and feeding that back into the next advance would call
            // `js_object_get_prototype_of(undefined)`, which throws "Cannot
            // convert undefined or null to object". comment-json's `__extends`
            // feature-test `{__proto__: []}` hit this at Next.js server boot. A
            // genuine cycle can never contain undefined, so ending the walk is
            // sound.
            if nb == TAG_NULL_U64 || nb == TAG_UNDEFINED_U64 {
                TAG_NULL_U64
            } else {
                nb
            }
        };
        let mut tortoise = proto_bits;
        let mut hare = proto_bits;
        loop {
            // Check current tortoise position first (catches `proto == obj`
            // on the very first iteration without an extra advance).
            if tortoise == obj_bits {
                throw_object_type_error(b"Cyclic __proto__ value");
            }
            if tortoise == TAG_NULL_U64 {
                break;
            }
            // Advance tortoise one step, hare two steps.
            tortoise = advance(tortoise);
            // The hare reaches the chain end (null) before the tortoise on any
            // acyclic chain longer than one link (e.g. a function proto:
            // fn → Function.prototype → Object.prototype → null). Freeze it at
            // null instead of advancing again — advance(null) would call
            // js_object_get_prototype_of(null), which throws "Cannot convert
            // undefined or null to object". comment-json's `__extends` hit this
            // on every transpiled subclass at Next.js server boot. The tortoise
            // still walks the remaining chain alone, so the obj-membership
            // (cycle) check stays complete.
            hare = if hare == TAG_NULL_U64 {
                TAG_NULL_U64
            } else {
                let h1 = advance(hare);
                if h1 == TAG_NULL_U64 {
                    TAG_NULL_U64
                } else {
                    advance(h1)
                }
            };
            // If they meet, the existing chain already has a cycle — the walk
            // will never reach null, so we also can never form a new one by
            // setting obj's proto. Just break; the set is safe.
            if hare == tortoise {
                break;
            }
        }
    }

    // A class constructor's own [[Prototype]]: recorded on its function object
    // (the state record every function object uses). Effect's Schema.Opaque
    // depends on this exact shape:
    //
    //   class Opaque {}
    //   Object.setPrototypeOf(Opaque, schema)
    //   class Partial extends Opaque {}
    //   Partial.ast
    //
    // It is the CONSTRUCTOR-side link.
    //
    // It must not go in CLASS_PROTOTYPE_OBJECTS: that table means "what
    // INSTANCES of this class inherit from", so parking a constructor link
    // there makes `new Opaque().ast` resolve the static (Node: undefined) and
    // makes prototype-method mirroring write into `schema` itself. A null
    // prototype clears an earlier link. Other valid prototype kinds retain
    // their existing behavior.
    if let Some(class_id) = super::super::class_ref_id(obj_value) {
        if proto_is_null {
            super::super::class_registry::class_static_prototype_root_clear(class_id);
            return obj_value;
        }
        if (proto_bits & 0xFFFF_0000_0000_0000) == POINTER_TAG {
            let proto_ptr = crate::value::js_nanbox_get_pointer(proto) as *mut ObjectHeader;
            // `proto` is user-supplied, so it can carry a fetch/zlib/proxy
            // handle rather than a heap object. A bare `is_valid_obj_ptr`
            // accepts those bands on Linux, and this pointer is stored into a
            // GC root table that the collector later dereferences — a segfault
            // there, silently hidden on macOS (#1843/#4004/#4665/#4800/#6271).
            // Require a real, readable GC header instead.
            // A function (including another class) is a valid [[Prototype]]:
            // the link is a traced edge of the function object, not a table.
            if !proto_ptr.is_null()
                && crate::value::addr_class::is_above_handle_band(proto_ptr as usize)
                && unsafe {
                    crate::value::addr_class::try_read_gc_header(proto_ptr as usize).is_some()
                }
                && is_valid_obj_ptr(proto_ptr as *const u8)
            {
                super::super::class_registry::class_static_prototype_root_store(
                    class_id, proto_ptr,
                );
                return obj_value;
            }
        }
    }

    // #2820: setting the prototype of a primitive target is a spec no-op that
    // returns the (boxed) primitive value. `value_is_object_like` is false for
    // numbers/strings/booleans, and class refs are handled by the recording
    // path below — so a non-object, non-closure target just returns unchanged.
    let obj_ptr_for_record = {
        let top = obj_bits >> 48;
        if top == 0x7FFD {
            (obj_bits & 0x0000_FFFF_FFFF_FFFF) as usize
        } else if top == 0 && obj_bits > 0x10000 {
            obj_bits as usize
        } else {
            0
        }
    };

    // Wall 10 — `Object.setPrototypeOf(handle, proto)` on a native registry
    // handle (a POINTER-tagged small-handle id, e.g. a node:http
    // `ServerResponse` / `IncomingMessage`). Express attaches its augmented
    // `res.send` / `res.json` / `res.status` (and `req.fresh` / `req.accepts` /
    // …) onto the per-request native objects via
    // `Object.setPrototypeOf(res, app.response)`. The heap-object recording
    // path below rejects the handle (`is_valid_obj_ptr` is false for a small
    // id), so without this the prototype was silently dropped and every
    // express response method no-op'd (the Wall-10 express/NestJS blocker).
    // Record the link in the SAME `OBJECT_PROTOTYPES` side-table keyed by the
    // handle id; the small-handle method/property dispatch fallbacks then walk
    // it via `resolve_inherited_field`, binding `this` to the handle so the
    // express method's internal `this.end(...)` / `this.statusCode = …` route
    // back to the native handle. Gated on a non-zero handle id in the small
    // band; a plain heap object (top16 0, addr above the band) still takes the
    // canonical path below.
    {
        let top = obj_bits >> 48;
        let handle_id = if top == 0x7FFD {
            (obj_bits & 0x0000_FFFF_FFFF_FFFF) as usize
        } else {
            0
        };
        if crate::value::addr_class::is_small_handle(handle_id) {
            super::super::prototype_chain::object_set_user_prototype(handle_id, proto_bits);
            return obj_value;
        }
    }

    // #36 / #321: when the target is a closure (a plain function value) and the
    // proto is an object or null, record the (closure → proto) link in the closure
    // static-prototype side-table. effect's `Context.Tag(id)` returns a
    // function `TagClass` whose `_op`/`[TagTypeId]`/`[EffectTypeId]` live on a
    // `TagProto` object wired in via `Object.setPrototypeOf(TagClass,
    // TagProto)`. Recording the link lets later string/symbol property reads on
    // the closure (and on a subclass that `extends TagClass`) walk to the
    // proto's own properties, so the Tag is recognized as a valid Effect.
    if (obj_bits & 0xFFFF_0000_0000_0000) == POINTER_TAG
        && ((proto_bits & 0xFFFF_0000_0000_0000) == POINTER_TAG
            || proto_bits == crate::value::TAG_NULL)
    {
        let obj_ptr = crate::value::js_nanbox_get_pointer(obj_value) as usize;
        if obj_ptr != 0 && crate::closure::is_closure_ptr(obj_ptr) {
            crate::closure::closure_set_static_prototype(obj_ptr, proto_bits);
            return obj_value;
        }
    }

    // #2820: ordinary heap object — record the observable [[Prototype]] in the
    // object-prototype side-table so `Object.getPrototypeOf(obj)` and inherited
    // property reads (`obj.x` where `x` lives on `proto`) reflect it. Records
    // `TAG_NULL` for `setPrototypeOf(obj, null)`.
    if obj_ptr_for_record != 0
        && !crate::closure::is_closure_ptr(obj_ptr_for_record)
        && is_valid_obj_ptr(obj_ptr_for_record as *const u8)
    {
        super::super::prototype_chain::object_set_user_prototype(obj_ptr_for_record, proto_bits);
        // A grown array's local may still hold the FORWARDED (old) pointer;
        // the spec [[HasProperty]]/[[Get]] helpers look the prototype up by
        // the CLEANED address. Record under both keys so either resolves
        // (test262 copyWithin/coerced-values-start-change-* second case).
        unsafe {
            let hdr = (obj_ptr_for_record as *const u8).sub(crate::gc::GC_HEADER_SIZE)
                as *const crate::gc::GcHeader;
            if (*hdr).obj_type == crate::gc::GC_TYPE_ARRAY
                || (*hdr).obj_type == crate::gc::GC_TYPE_LAZY_ARRAY
            {
                let cleaned = crate::array::clean_arr_ptr(
                    obj_ptr_for_record as *const crate::array::ArrayHeader,
                ) as usize;
                if cleaned != 0 && cleaned != obj_ptr_for_record {
                    super::super::prototype_chain::object_set_user_prototype(cleaned, proto_bits);
                }
            }
        }
    }

    // Spec: `Object.setPrototypeOf(O, proto)` returns O.
    obj_value
}
