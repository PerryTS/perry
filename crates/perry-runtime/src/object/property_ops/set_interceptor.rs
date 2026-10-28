//! OrdinarySet's interception walk, shared with the strict by-name store.
//! `None` means an ordinary own data write can complete the operation; the
//! caller owns that write, so this walk never re-enters the by-name store.
use super::*;

pub(crate) fn set_interceptor(target: f64, key: f64, value: f64, receiver: f64) -> Option<bool> {
    let mut current = target;
    // Bounded by the chain, not a hop count: past 64 hops a store on a deep
    // `Object.create` chain was refused (#12698; silent in sloppy code,
    // "Cannot assign to read only property" in strict code).
    let mut cycle = crate::object::prototype_chain::ChainCycle::new();
    loop {
        if cycle.repeats(current.to_bits() as usize) {
            return Some(false);
        }
        // A Proxy hop in the prototype chain: `OrdinarySetWithOwnDescriptor`
        // step 2.a-b dispatches the full `[[Set]]` on the parent with the
        // ORIGINAL `receiver`, not the raw own-descriptor walk below (which
        // would misread the small proxy id as a heap pointer).
        if lookup(current).is_some() {
            return Some(
                crate::value::js_is_truthy(proxy_set_with_receiver(current, key, value, receiver))
                    != 0,
            );
        }
        // Integer-Indexed exotic [[Set]] (§10.4.5.5): a typed array in the
        // chain intercepts a canonical numeric index key — the prototype
        // chain is NEVER consulted for it. `SameValue(O, Receiver)` writes
        // the element; a different receiver with a valid index falls to the
        // ordinary data-descriptor flow (create on receiver); an invalid
        // canonical index is a silent no-op `true`.
        let cur_addr = extract_pointer(current.to_bits()) as usize;
        if crate::typedarray_props::is_typed_array_owner(cur_addr) {
            if let Some(name) = property_key_to_rust_string(key) {
                match crate::typedarray_props::typed_array_canonical_index_validity(cur_addr, &name)
                {
                    Some(valid) => {
                        let recv_addr = extract_pointer(receiver.to_bits()) as usize;
                        if recv_addr == cur_addr {
                            return Some(unsafe {
                                crate::typedarray_props::typed_array_set_property_by_name(
                                    cur_addr, &name, value,
                                )
                            });
                        }
                        if !valid {
                            return Some(true);
                        }
                        // The receiver may itself be a typed array: the
                        // CreateDataProperty lands in ITS [[DefineOwnProperty]],
                        // which rejects an index that is invalid FOR THE
                        // RECEIVER (`Reflect.set(ta, "0", v, emptyTa)` → false).
                        if crate::typedarray_props::is_typed_array_owner(recv_addr) {
                            return match crate::typedarray_props::
                                typed_array_canonical_index_validity(recv_addr, &name)
                            {
                                Some(true) => Some(unsafe {
                                    crate::typedarray_props::typed_array_set_property_by_name(
                                        recv_addr, &name, value,
                                    )
                                }),
                                Some(false) => Some(false),
                                None => None,
                            };
                        }
                        return None;
                    }
                    // Ordinary key on a TA in the chain: stop the walk (Perry's
                    // TA prototype methods are served natively, not as data
                    // descriptors visible to `own_set_descriptor`) and define
                    // on the receiver.
                    None => {
                        return None;
                    }
                }
            }
        }
        if let Some(desc) = own_set_descriptor(current, key) {
            return match desc {
                OwnSetDescriptor::Data { writable } => (!writable).then_some(false),
                OwnSetDescriptor::Accessor { setter_bits } => {
                    Some(call_setter_with_receiver(setter_bits, receiver, value))
                }
            };
        }
        // #11201: a class prototype (or class instance) on a class-id-0
        // receiver's chain keeps its accessors in the class vtable, which
        // `own_set_descriptor` does not model. See `class_link_accessor_set`.
        if let Some(done) = class_link_accessor_set(current, key, value, receiver) {
            return Some(done);
        }
        // #6828/#10482: `%Object.prototype%.__proto__` is a legacy accessor
        // whose setter performs `SetPrototypeOf(Receiver, value)`.
        // `object/global_this/proto_methods.rs` now materializes it as a
        // REAL accessor descriptor on `Object.prototype` (#10482), so
        // `own_set_descriptor` just above finds it and dispatches through
        // `call_setter_with_receiver` before the walk ever reaches here —
        // this arm is kept as a fallback for a walk that reaches
        // `Object.prototype` without ever consulting the descriptor table
        // (defensive; not known to be reachable). Both arms must behave
        // identically, so both call the one shared implementation.
        //
        // Keep this AFTER `own_set_descriptor`: a user-installed own
        // `__proto__` data/accessor property on an object earlier in the chain
        // must win. A null-prototype receiver never reaches the canonical
        // Object.prototype and therefore still creates an ordinary own data
        // property.
        let current_addr = extract_pointer(current.to_bits()) as usize;
        if current_addr != 0
            && key_equals(key, b"__proto__")
            && current_addr == crate::array::object_prototype_addr()
        {
            legacy_dunder_proto_set(receiver, value);
            return Some(true);
        }
        // A class function object is not a leaf of the walk: its [[Prototype]]
        // (the parent class) may hold the accessor (#11521).
        if crate::closure::is_closure_ptr(extract_pointer(current.to_bits()) as usize)
            && crate::object::class_value::class_closure_id(
                extract_pointer(current.to_bits()) as usize
            )
            .is_none()
        {
            // ECMAScript poison pill: `fn.caller = v` / `fn.arguments = v` on
            // a strict-mode function throws via %ThrowTypeError%. A plain
            // non-strict function instead rejects the inherited setter-less
            // property with `false`; PutValue's `Throw` flag decides whether
            // that rejection stays silent or becomes a TypeError. A genuine
            // own data prop (defineProperty round-trip) still wins via the
            // descriptor arm above.
            let cur_ptr = extract_pointer(current.to_bits()) as usize;
            if let Some(name) = key_to_rust_string(key) {
                if matches!(name.as_str(), "caller" | "arguments")
                    && !crate::closure::closure_has_own_dynamic_prop(cur_ptr, &name)
                {
                    let closure = cur_ptr as *const crate::closure::ClosureHeader;
                    let is_non_strict_ordinary_function = crate::closure::closure_info(closure)
                        .is_some_and(|info| {
                            // A permanent compiled body carries its kind in
                            // the info whether or not its text was retained.
                            const PERMANENT_COMPILED: u32 = crate::codegen_abi::FN_PERMANENT_IMAGE
                                | crate::codegen_abi::FN_COMPILED_BODY;
                            if info.flags & crate::codegen_abi::FN_HAS_SOURCE != 0
                                || info.flags & PERMANENT_COMPILED == PERMANENT_COMPILED
                            {
                                info.flags & crate::codegen_abi::FN_NON_STRICT_ORDINARY != 0
                            } else {
                                crate::builtins::function_is_non_strict_ordinary_for_ptr(
                                    info.code as usize,
                                )
                            }
                        });
                    if is_non_strict_ordinary_function {
                        return Some(false);
                    }
                    throw_type_error("Restricted function property assignment");
                }
            }
            // Functions continue through the same descriptor walk: a
            // handled readonly write is a failure, not a successful store.
        }
        let Some(proto) = prototype_of_for_set(current) else {
            return None;
        };
        current = proto;
    }
}
