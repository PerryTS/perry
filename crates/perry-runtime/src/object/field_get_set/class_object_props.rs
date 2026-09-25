//! #6497: `.name` on a heap class-expression value (`ClassExprFresh` — #6470
//! also routes capturing function-body class DECLARATIONS through it) must
//! expose the template's registry name, matching the INT32 class-ref path's
//! #2059 arm. Split from `get_field_by_name_tail.rs` for the file-size cap.

use super::*;

const CLASS_EVALUATION_PROTOTYPE_KEY: &[u8] = b"#<perry:class-evaluation-prototype>";

/// Set once the first per-evaluation prototype is materialized, so
/// [`class_evaluation_prototype_class_id`] costs one relaxed load for the
/// (overwhelmingly common) program that never builds one. Its caller is the
/// miss path of `class_id_for_decl_prototype_object`, which every
/// `Object.defineProperty` reaches (#9180).
static CLASS_EVALUATION_PROTOTYPES_MATERIALIZED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// #11043: the template class id of `ptr` when it is the prototype object
/// materialized by [`class_evaluation_prototype_value`] for one evaluation of
/// a heap class object, else `None`.
///
/// Recognized structurally rather than through a side table: the prototype's
/// traced metadata holds the evaluation owner, and that class object's hidden
/// evaluation-prototype slot points back at `ptr`. The public `constructor`
/// property may be replaced or deleted. A side table would have to
/// be either a GC root (leaking one prototype per evaluation of a class that
/// lives in a factory) or a weak, evacuation-rekeyed map; the back-edge is
/// already maintained by the heap itself. Never allocates — callers hold raw
/// pointers across this call.
pub(crate) fn class_evaluation_prototype_class_id(ptr: usize) -> Option<u32> {
    if !CLASS_EVALUATION_PROTOTYPES_MATERIALIZED.load(std::sync::atomic::Ordering::Relaxed) {
        return None;
    }
    unsafe {
        let header = crate::value::addr_class::try_read_gc_header(ptr)?;
        if header.obj_type != crate::gc::GC_TYPE_OBJECT
            || header.gc_flags & crate::gc::GC_FLAG_FORWARDED != 0
        {
            return None;
        }
        // A class object shares its template id with its prototype; it is
        // the constructor, never the prototype.
        if super::super::class_registry::is_class_object_ptr(ptr as *const u8) {
            return None;
        }
        let class_id = (*(ptr as *const ObjectHeader)).class_id;
        if class_id == 0 {
            return None;
        }
        let proto_value = crate::value::js_nanbox_pointer(ptr as i64);
        let ctor = super::super::private_evaluation_brand_value(proto_value)?;
        if !super::super::class_registry::is_class_object_value(ctor) {
            return None;
        }
        let class_obj = JSValue::from_bits(ctor.to_bits()).as_pointer::<ObjectHeader>();
        if (*class_obj).class_id != class_id {
            return None;
        }
        let back_edge = super::super::js_object_get_own_field_or_undef(
            ctor,
            CLASS_EVALUATION_PROTOTYPE_KEY.as_ptr(),
            CLASS_EVALUATION_PROTOTYPE_KEY.len(),
        );
        (back_edge.to_bits() == proto_value.to_bits()).then_some(class_id)
    }
}

/// Materialize the distinct prototype object created by one evaluation of a
/// heap class expression/declaration. Template class ids still own dispatch,
/// but observable method identity and private-name closures belong to the
/// evaluation, not to that shared template.
unsafe fn class_evaluation_prototype_value(obj: *const ObjectHeader) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let class = scope.root_raw_mut_ptr(obj as *mut ObjectHeader);
    let hidden_key = crate::string::js_string_from_bytes(
        CLASS_EVALUATION_PROTOTYPE_KEY.as_ptr(),
        CLASS_EVALUATION_PROTOTYPE_KEY.len() as u32,
    );
    let hidden_key = scope.root_string_ptr(hidden_key);
    let existing = class.with_mut_ptr::<ObjectHeader, _>(|class| {
        hidden_key
            .with_const_ptr::<crate::StringHeader, _>(|key| own_data_field_by_name(class, key))
    });
    if let Some(existing) = existing.filter(|value| !value.is_undefined()) {
        return f64::from_bits(existing.bits());
    }

    let class_id = class.with_mut_ptr::<ObjectHeader, _>(|class| (*class).class_id);
    let proto = scope.root_raw_mut_ptr(js_object_alloc(class_id, 0));
    CLASS_EVALUATION_PROTOTYPES_MATERIALIZED.store(true, std::sync::atomic::Ordering::Relaxed);

    // Symbol aliases resolve method values lazily. Their lexical evaluation
    // owner must survive changes to the public `constructor` property. This
    // traced metadata edge is not an initialized instance-private element.
    let owner = class
        .with_const_ptr::<ObjectHeader, _>(|class| crate::value::js_nanbox_pointer(class as i64));
    proto.with_mut_ptr::<ObjectHeader, _>(|proto| {
        super::stamp_private_evaluation_brand(proto, owner);
    });

    let constructor_key = crate::string::js_string_from_bytes(b"constructor".as_ptr(), 11);
    let constructor_key = scope.root_string_ptr(constructor_key);
    let class_value = class
        .with_mut_ptr::<ObjectHeader, _>(|class| crate::value::js_nanbox_pointer(class as i64));
    proto.with_mut_ptr::<ObjectHeader, _>(|proto| {
        constructor_key.with_const_ptr::<crate::StringHeader, _>(|key| {
            js_object_set_field_by_name(proto, key, class_value)
        });
        set_builtin_property_attrs(
            proto as usize,
            "constructor".to_string(),
            PropertyAttrs::new(true, false, true),
        );
    });

    for name in super::super::class_registry::class_decl_prototype_method_names(class_id) {
        let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
        let key = scope.root_string_ptr(key);
        let class_value = class
            .with_mut_ptr::<ObjectHeader, _>(|class| crate::value::js_nanbox_pointer(class as i64));
        let method = super::super::native_module::class_evaluation_method_value_for_name(
            class_id,
            &name,
            class_value,
        );
        proto.with_mut_ptr::<ObjectHeader, _>(|proto| {
            key.with_const_ptr::<crate::StringHeader, _>(|key| {
                js_object_set_field_by_name(proto, key, method)
            });
            set_builtin_property_attrs(proto as usize, name, PropertyAttrs::new(true, false, true));
        });
    }

    // Each evaluation owns a distinct prototype object, and that object's
    // [[Prototype]] follows this evaluation's pinned heritage edge rather than
    // the template-id (last-wins) parent table.
    let pinned_parent = class.with_const_ptr::<ObjectHeader, _>(|class| {
        super::super::class_registry::class_object_pinned_parent(class)
    });
    let parent_proto = match pinned_parent {
        Some(parent) if parent.to_bits() == crate::value::TAG_NULL => Some(crate::value::TAG_NULL),
        Some(parent) => {
            let parent = scope.root_nanbox_f64(parent);
            let parent_value = parent.get_nanbox_f64();
            if super::super::class_registry::is_class_object_value(parent_value) {
                let parent_obj =
                    JSValue::from_bits(parent_value.to_bits()).as_pointer::<ObjectHeader>();
                (!parent_obj.is_null())
                    .then(|| class_evaluation_prototype_value(parent_obj).to_bits())
            } else if let Some(parent_id) = super::super::class_ref_id(parent_value) {
                Some(super::super::class_registry::class_decl_prototype_value(parent_id).to_bits())
            } else if let Some(proto) = ((parent_value.to_bits() >> 48) == 0x7FFE)
                .then(|| {
                    // #11298: a static native parent (`extends EventEmitter`)
                    // pins as a ClassRef to a RESERVED builtin id, which has
                    // no class registration. Resolve it as the declared-class
                    // path does (#10599).
                    super::super::class_registry::reserved_native_parent_prototype_bits(
                        parent_value.to_bits() as u32,
                    )
                })
                .flatten()
            {
                Some(proto)
            } else {
                let parent_js = JSValue::from_bits(parent_value.to_bits());
                if parent_js.is_pointer()
                    && crate::closure::is_closure_ptr(parent_js.as_pointer::<u8>() as usize)
                {
                    let value = crate::closure::closure_get_dynamic_prop(
                        parent_js.as_pointer::<u8>() as usize,
                        "prototype",
                    );
                    let value_js = JSValue::from_bits(value.to_bits());
                    if value.to_bits() == crate::value::TAG_NULL {
                        Some(crate::value::TAG_NULL)
                    } else {
                        value_js.is_pointer().then_some(value.to_bits())
                    }
                } else {
                    None
                }
            }
        }
        None => super::super::class_registry::global_object_prototype_bits(),
    };
    if let Some(parent_proto) = parent_proto {
        let parent_proto = scope.root_heap_word_u64(parent_proto);
        proto.with_mut_ptr::<ObjectHeader, _>(|proto| {
            super::super::prototype_chain::object_link_class_evaluation_prototype(
                proto as usize,
                parent_proto.get_heap_word_u64(),
            )
        });
    }

    let proto_value = proto
        .with_mut_ptr::<ObjectHeader, _>(|proto| crate::value::js_nanbox_pointer(proto as i64));
    class.with_mut_ptr::<ObjectHeader, _>(|class| {
        hidden_key.with_const_ptr::<crate::StringHeader, _>(|key| {
            js_object_set_field_by_name(class, key, proto_value)
        })
    });
    proto.with_mut_ptr::<ObjectHeader, _>(|proto| crate::value::js_nanbox_pointer(proto as i64))
}

/// #4949: heap class-expression values (`ClassExprFresh`) are real
/// OBJECT_TYPE_CLASS objects, not INT32 class refs. Their `.prototype`
/// read must still expose the live declared-class prototype object so
/// tsc/tslib decorator code can inspect and mutate method descriptors.
pub(crate) unsafe fn class_object_prototype_value(obj: *const ObjectHeader) -> JSValue {
    JSValue::from_bits(class_evaluation_prototype_value(obj).to_bits())
}

/// #10890: the prototype object [`class_evaluation_prototype_value`] already
/// materialized for this evaluation of a heap class object, or `None` when
/// nothing has read `C.prototype` yet.
///
/// Never allocates, so an instance-read walk that holds raw pointers can call
/// it. An unmaterialized prototype cannot hold a user write (writing one
/// needs the `C.prototype` read that materializes it). Its declared methods
/// are served by the template walk that the caller continues with.
pub(crate) fn class_object_materialized_prototype(
    obj: *const ObjectHeader,
) -> Option<*mut ObjectHeader> {
    let value = super::super::class_registry::class_object_own_field_bytes(
        obj,
        CLASS_EVALUATION_PROTOTYPE_KEY,
    )?;
    let value = JSValue::from_bits(value.to_bits());
    if !value.is_pointer() {
        return None;
    }
    let proto = value.as_pointer::<ObjectHeader>() as *mut ObjectHeader;
    (!proto.is_null()).then_some(proto)
}

/// Resolve `.name` for an `OBJECT_TYPE_CLASS` heap object. An explicit
/// `static name` member (an own field on the class object) wins; a deleted
/// key still reads `undefined` (returns `None`).
pub(super) unsafe fn class_object_name_value(
    obj: *const ObjectHeader,
    key: *const crate::StringHeader,
) -> Option<JSValue> {
    if let Some(v) = own_data_field_by_name(obj, key) {
        return Some(v);
    }
    let class_id = (*obj).class_id;
    if super::super::class_registry::class_is_key_deleted(class_id, "name") {
        return None;
    }
    let cname = super::super::class_registry::class_name_for_id(class_id)?;
    let s = crate::string::js_string_from_bytes(cname.as_ptr(), cname.len() as u32);
    Some(JSValue::from_bits(
        crate::js_nanbox_string(s as i64).to_bits(),
    ))
}

/// #6530 (size-gate split from `get_field_by_name_tail.rs` — pure
/// relocation): resolve the `constructor` special key for an instance
/// receiver. Own `constructor` data field wins; then WeakMap/WeakSet,
/// the per-evaluation class-object registry (capture-carrying classes),
/// vtable `constructor` methods, boxed primitives, anon shapes, the
/// function-class table, and the INT32 ClassRef synthesis. `None` means
/// "not resolved here" — the caller falls through to the generic walk.
pub(super) unsafe fn instance_constructor_value(
    obj: *const ObjectHeader,
    key: *const crate::StringHeader,
) -> Option<JSValue> {
    if let Some(v) = own_data_field_by_name(obj, key) {
        return Some(v);
    }
    // #7757: a monomorphized specialization must present the GENERIC's
    // reflective surface. `new Gen<number>()` is stamped with `Gen$num`'s id
    // (`monomorph::mangle::generate_specialized_name`), so every arm below
    // answered with the specialization -- making `a.constructor !== Gen` and,
    // worse after #7632 gave both the same display name, two constructors that
    // PRINT identically and compare unequal. TypeScript erases type arguments:
    // at runtime there is exactly one `Gen`.
    //
    // This is the third and last identity surface to take the same origin edge
    // `instanceof` uses (#7575), the display name uses (#7632) and the two
    // prototype registries use (#7762). METHOD DISPATCH is deliberately not
    // aliased -- it runs off the per-class-id vtable, so each specialization
    // keeps its own monomorphized bodies.
    let class_id = crate::object::class_generic_origin((*obj).class_id)
        .filter(|generic| is_class_id_registered(*generic))
        .unwrap_or((*obj).class_id);
    // #6530: a capture-carrying class has no ClassRef value — the
    // class VALUE is the per-evaluation class OBJECT registered at
    // `js_object_mark_class` time. Return that same object so
    // identity holds (`x.constructor === Sub`, bundled zod's
    // `describe()` re-construction via `this.constructor`
    // yielding the SUBCLASS instead of collapsing to the base).
    // The arms below would otherwise hand back a bound
    // constructor-method closure (whose underlying func is the
    // nearest ancestor ctor — reporting the BASE class's name for
    // every subclass) or the bare INT32 ClassRef.
    if let Some(v) = super::super::class_registry::class_object_value_for_cid(class_id) {
        return Some(JSValue::from_bits(v.to_bits()));
    }
    // #5834: WeakMap/WeakSet instances carry a reserved class_id
    // (not a registered declared-class one), so none of the
    // arms below resolve them and `(new WeakMap()).constructor`
    // fell through to `undefined`.
    if class_id == crate::weakref::CLASS_ID_WEAKMAP || class_id == crate::weakref::CLASS_ID_WEAKSET
    {
        let name: &[u8] = if class_id == crate::weakref::CLASS_ID_WEAKMAP {
            b"WeakMap"
        } else {
            b"WeakSet"
        };
        let v = js_get_global_this_builtin_value(name.as_ptr(), name.len());
        return Some(JSValue::from_bits(v.to_bits()));
    }
    if class_id != 0 && class_has_own_method(class_id, "constructor") {
        let value = class_prototype_method_value_for_name(class_id, "constructor");
        return Some(JSValue::from_bits(value.to_bits()));
    }
    if matches!(
        class_id,
        CLASS_ID_BOXED_NUMBER
            | CLASS_ID_BOXED_STRING
            | CLASS_ID_BOXED_BOOLEAN
            | CLASS_ID_BOXED_BIGINT
            | CLASS_ID_BOXED_SYMBOL
    ) {
        let name = match class_id {
            CLASS_ID_BOXED_NUMBER => b"Number".as_slice(),
            CLASS_ID_BOXED_STRING => b"String".as_slice(),
            CLASS_ID_BOXED_BOOLEAN => b"Boolean".as_slice(),
            CLASS_ID_BOXED_BIGINT => b"BigInt".as_slice(),
            CLASS_ID_BOXED_SYMBOL => b"Symbol".as_slice(),
            _ => unreachable!(),
        };
        let v = js_get_global_this_builtin_value(name.as_ptr(), name.len());
        return Some(JSValue::from_bits(v.to_bits()));
    }
    // Object-literal instances (`{ x: 1 }`) carry a synthetic
    // `__AnonShape_*` class id. Spec says their `.constructor`
    // is the global `Object`, not the synthetic class — so
    // resolve through the globalThis singleton so the value
    // matches the bare `Object` identifier (`x.constructor
    // === Object`, date-fns `constructFrom`, drizzle's
    // `isPlainObject` duck check).
    if class_id != 0 && is_anon_shape_class_id(class_id) {
        let v = js_get_global_this_builtin_value(b"Object".as_ptr(), 6);
        return Some(JSValue::from_bits(v.to_bits()));
    }
    if let Some(func_value) = super::super::class_registry::function_value_for_class_id(class_id) {
        return Some(JSValue::from_bits(func_value.to_bits()));
    }
    // #10478: an `Object.create(proto)` result is stamped with a synthetic
    // class id that only indexes its prototype object
    // (`CLASS_PROTOTYPE_OBJECTS`); unlike the function ids above it names no
    // class VALUE. Its `constructor` is the inherited `proto.constructor`, so
    // read it off that chain. The INT32 synthesis below minted a ClassRef for
    // the synthetic id itself (`0x7FFE_0000_8000_0000`): unequal to `Object` /
    // `A`, printed as `[object Function]`, and `C instanceof C` segfaulted
    // (lodash `isEqual(cloneDeep(x), x)`).
    let synthetic_proto = super::super::class_registry::synthetic_class_prototype_object(class_id);
    if !synthetic_proto.is_null() {
        // The prototype's OWN `constructor` data field answers the common
        // shapes directly — `Object.prototype`, a declared `C.prototype`, a
        // materialized `F.prototype`, a `{ constructor: F }` literal — so take
        // it without the general chain walk, whose implicit-`this` juggling,
        // accessor-receiver override and registry probes cost ~3000
        // instructions per read. Skipped when an accessor owns the key, which
        // must run through the walk to fire with the right receiver.
        if get_accessor_descriptor(synthetic_proto as usize, "constructor").is_none() {
            if let Some(value) = own_data_field_by_name(synthetic_proto, key) {
                if !value.is_undefined() && !value.is_null() {
                    return Some(value);
                }
            }
        }
        let receiver = f64::from_bits(crate::value::js_nanbox_pointer(obj as i64).to_bits());
        return Some(
            super::super::class_registry::resolve_proto_chain_field_with_receiver(
                class_id, key, receiver,
            )
            .unwrap_or_else(JSValue::undefined),
        );
    }
    if class_id != 0 && is_class_id_registered(class_id) {
        let bits = 0x7FFE_0000_0000_0000u64 | (class_id as u64);
        return Some(JSValue::from_bits(bits));
    }
    // class_id == 0 fallback: plain ObjectHeader allocated
    // without an HIR shape (Object.create(null) hybrids, raw
    // empty `{}` produced by JSON.parse, etc.). Report
    // `Object` so duck-type tests don't trip undefined.
    if class_id == 0 {
        // #6537 review: an EXPLICIT null-prototype object
        // (`Object.create(null)` / `js_object_alloc_null_proto`, marked with
        // `OBJ_FLAG_NULL_PROTO` on the GC header) has NO `constructor` —
        // fall through (→ undefined) instead of reporting `Object`. The
        // `Object` report stays for ordinary shapeless objects (raw `{}`
        // from JSON.parse etc.), which spec-correctly inherit
        // `Object.prototype.constructor`.
        if let Some(gc_header) = crate::value::addr_class::try_read_gc_header(obj as usize) {
            if gc_header._reserved & crate::gc::OBJ_FLAG_NULL_PROTO != 0 {
                return None;
            }
        }
        let v = js_get_global_this_builtin_value(b"Object".as_ptr(), 6);
        return Some(JSValue::from_bits(v.to_bits()));
    }
    None
}

#[cfg(test)]
mod evaluation_owner_tests {
    use super::*;

    #[test]
    fn prototype_owner_survives_constructor_replacement_and_deletion() {
        unsafe {
            const CID: u32 = 62_442;
            let scope = crate::gc::RuntimeHandleScope::new();
            let class = scope.root_raw_mut_ptr(js_object_alloc(CID, 0));
            class.with_mut_ptr(|class: *mut ObjectHeader| {
                crate::object::js_object_mark_class(class as i64);
            });
            let prototype = class.with_const_ptr(|class: *const ObjectHeader| {
                class_evaluation_prototype_value(class)
            });
            let prototype = scope.root_nanbox_f64(prototype);
            let ptr = || crate::value::js_nanbox_get_pointer(prototype.get_nanbox_f64()) as usize;
            assert_eq!(class_evaluation_prototype_class_id(ptr()), Some(CID));
            let key = scope.root_string_ptr(crate::string::js_string_from_bytes(
                b"constructor".as_ptr(),
                11,
            ));
            key.with_const_ptr(|key: *const crate::StringHeader| {
                js_object_set_field_by_name(
                    ptr() as *mut ObjectHeader,
                    key,
                    f64::from_bits(crate::value::TAG_NULL),
                );
            });
            assert_eq!(class_evaluation_prototype_class_id(ptr()), Some(CID));
            key.with_const_ptr(|key: *const crate::StringHeader| {
                crate::object::js_object_delete_field(ptr() as *mut ObjectHeader, key);
            });
            assert_eq!(class_evaluation_prototype_class_id(ptr()), Some(CID));
            let owner =
                crate::object::private_evaluation_brand_value(prototype.get_nanbox_f64()).unwrap();
            class.with_const_ptr(|class: *const ObjectHeader| {
                assert_eq!(crate::value::js_nanbox_get_pointer(owner), class as i64);
            });
        }
    }
}
