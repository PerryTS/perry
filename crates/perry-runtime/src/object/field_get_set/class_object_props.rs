//! #6497: `.name` on a heap class-expression value (`ClassExprFresh` — #6470
//! also routes capturing function-body class DECLARATIONS through it) must
//! expose the template's registry name, matching the INT32 class-ref path's
//! #2059 arm. Split from `get_field_by_name_tail.rs` for the file-size cap.

use super::*;

pub(crate) const CLASS_EVALUATION_PROTOTYPE_KEY: &[u8] = b"#<perry:class-evaluation-prototype>";

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
    // The slot the template's recorded `EvaluationPrototype` transition put
    // it in, while the object still carries that transition's target shape.
    if let Some(existing) = super::class_object_template::recorded_evaluation_prototype(obj) {
        return existing;
    }
    if let Some(existing) = super::super::class_registry::class_object_own_field_bytes(
        obj,
        CLASS_EVALUATION_PROTOTYPE_KEY,
    )
    .filter(|value| value.to_bits() != crate::value::TAG_UNDEFINED)
    {
        return existing;
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let class = scope.root_raw_mut_ptr(obj as *mut ObjectHeader);
    let class_id = class.with_mut_ptr::<ObjectHeader, _>(|class| (*class).class_id);

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
    let parent_proto = parent_proto.map(|bits| scope.root_heap_word_u64(bits));

    // Every evaluation after the template's first in this agent is born in
    // the shape that one reached (`class_object_template`).
    if let Some(parent_proto) = &parent_proto {
        if let Some(proto) = class.with_mut_ptr::<ObjectHeader, _>(|class| {
            super::class_object_template::prototype_from_template(
                &scope,
                class,
                class_id,
                parent_proto.get_heap_word_u64(),
            )
        }) {
            CLASS_EVALUATION_PROTOTYPES_MATERIALIZED
                .store(true, std::sync::atomic::Ordering::Relaxed);
            let proto_value = proto
                .with_mut_ptr::<ObjectHeader, _>(|p| crate::value::js_nanbox_pointer(p as i64));
            class.with_mut_ptr::<ObjectHeader, _>(|class| {
                super::class_object_template::class_object_add_internal_for(
                    class,
                    super::class_object_template::InternalKey::EvaluationPrototype,
                    proto_value,
                )
            });
            return proto
                .with_mut_ptr::<ObjectHeader, _>(|p| crate::value::js_nanbox_pointer(p as i64));
        }
    }

    // Inline room for `constructor` and every declared member (see
    // `class_decl_prototype_value`).
    let members = super::super::class_registry::class_prototype_member_names(class_id).len() as u32;
    let proto = scope.root_raw_mut_ptr(js_object_alloc(class_id, members + 1));
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

    for (name, is_accessor) in super::super::class_registry::class_prototype_member_names(class_id)
    {
        if is_accessor {
            // S2: an accessor is a real accessor property of this prototype.
            proto.with_mut_ptr::<ObjectHeader, _>(|proto| {
                super::super::class_registry::install_decl_prototype_accessor(
                    proto, class_id, &name,
                )
            });
            continue;
        }
        let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
        let key = scope.root_string_ptr(key);
        let class_value = class
            .with_mut_ptr::<ObjectHeader, _>(|class| crate::value::js_nanbox_pointer(class as i64));
        // The evaluation's own function object for the method: its entry runs
        // the method at home in this evaluation (`<method>__eclo`). A template
        // compiled without entries keeps the by-name method value.
        let method =
            super::class_object_template::evaluation_method_value(class_id, &name, class_value)
                .unwrap_or_else(|| {
                    super::super::native_module::class_evaluation_method_value_for_name(
                        class_id,
                        &name,
                        class_value,
                    )
                });
        proto.with_mut_ptr::<ObjectHeader, _>(|proto| {
            key.with_const_ptr::<crate::StringHeader, _>(|key| {
                js_object_set_field_by_name(proto, key, method)
            });
            set_builtin_property_attrs(proto as usize, name, PropertyAttrs::new(true, false, true));
        });
    }

    if let Some(parent_proto) = &parent_proto {
        proto.with_mut_ptr::<ObjectHeader, _>(|proto| {
            super::super::prototype_chain::object_link_class_evaluation_prototype(
                proto as usize,
                parent_proto.get_heap_word_u64(),
            )
        });
        proto.with_mut_ptr::<ObjectHeader, _>(|proto| {
            class.with_const_ptr::<ObjectHeader, _>(|class| {
                super::class_object_template::record_prototype_template(
                    proto,
                    class,
                    class_id,
                    members + 1,
                    parent_proto.get_heap_word_u64(),
                )
            })
        });
    }

    let proto_value = proto
        .with_mut_ptr::<ObjectHeader, _>(|proto| crate::value::js_nanbox_pointer(proto as i64));
    class.with_mut_ptr::<ObjectHeader, _>(|class| {
        super::class_object_template::class_object_add_internal_for(
            class,
            super::class_object_template::InternalKey::EvaluationPrototype,
            proto_value,
        )
    });
    proto.with_mut_ptr::<ObjectHeader, _>(|proto| crate::value::js_nanbox_pointer(proto as i64))
}

/// What a fresh class object owns from the moment it is created, the way a
/// class's function object owns them (`class_value_mint`): `length` and `name`
/// (ClassDefinitionEvaluation's SetFunctionLength / SetFunctionName, `{ !w, !e,
/// c }`), then every ClassBody static method as a data property `{ w, !e, c }`
/// whose value is that declaration bound to this object. They are real
/// properties of THIS object, so `getOwnPropertyNames` lists them, `delete`
/// removes them, and a deleted one stays gone: the object's own keys are the
/// authority, not the template's registry (which every evaluation of the class
/// shares).
///
/// `prototype` is not stored here: it is `{ !w, !e, !c }`, so it is present on
/// every class object for its whole life ([`class_object_has_prototype_property`])
/// and has no state to keep. It is built at its first read, which must follow
/// the class definition (computed members register while it evaluates).
///
/// A static field of the same name (`static_field_mask`: bit 0 `length`, bit 1
/// `name`) is stored over `length` / `name` right after, so its slot is created
/// in this position with the field's ordinary attributes.
pub(crate) unsafe fn define_class_object_own_properties(
    obj: *mut ObjectHeader,
    static_field_mask: u32,
) {
    use super::super::class_value::{
        intrinsic_own_data_value, static_member_owns, INTRINSIC_ATTRS,
    };
    let class_id = (*obj).class_id;
    if class_id == 0 {
        return;
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let class = scope.root_raw_mut_ptr(obj);
    let define = |name: &str, value: f64, attrs: (bool, bool, bool)| {
        let value = scope.root_nanbox_f64(value);
        let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
        let key = scope.root_string_ptr(key);
        class.with_mut_ptr::<ObjectHeader, _>(|class| {
            key.with_const_ptr::<crate::StringHeader, _>(|key| {
                define_builtin_data_property(
                    class,
                    key,
                    value.get_nanbox_f64(),
                    name.to_string(),
                    PropertyAttrs::new(attrs.0, attrs.1, attrs.2),
                )
            })
        });
    };
    for (bit, key) in [(1, "length"), (2, "name")] {
        // A static method of this name replaces the value (and the attributes)
        // in the same position, below; a static accessor leaves nothing here.
        let method_replaces =
            super::super::class_registry::class_has_own_static_method(class_id, key);
        if static_member_owns(class_id, key) && !method_replaces {
            continue;
        }
        if static_field_mask & bit != 0 {
            // The class body's static field of this name stores over it next:
            // it keeps this position, and takes the field's ordinary attributes.
            define(
                key,
                f64::from_bits(crate::value::TAG_UNDEFINED),
                (true, true, true),
            );
        } else if let Some(value) = intrinsic_own_data_value(class_id, key) {
            define(key, value, INTRINSIC_ATTRS);
        }
    }
    for name in super::super::class_registry::class_own_string_member_names(class_id, true) {
        // The method's own function object runs the declaration's
        // closure-convention entry, as the shared class's function object's
        // does (`install_declared_static_method`): a call's `this` is the
        // body's `this`, and its `name` and `length` are facts of the entry.
        // One per evaluation, so evaluations never share a method value; its
        // home is this class object, the evaluation its body runs in however
        // it is called (`js_static_method_entry_enter_home`). An accessor has
        // no entry.
        let Some(code) =
            super::super::class_registry::class_own_static_method_code(class_id, &name)
        else {
            continue;
        };
        let f = super::class_object_template::static_method_value(code);
        if f.is_null() {
            continue;
        }
        class.with_mut_ptr::<ObjectHeader, _>(|class| {
            super::class_object_template::set_static_method_home(
                f,
                crate::value::js_nanbox_pointer(class as i64),
            )
        });
        define(
            &name,
            crate::value::js_nanbox_pointer(f as i64),
            (true, false, true),
        );
    }
}

/// Is `value` the method value `define_class_object_own_properties` stored for
/// static `name` of template `owner` on class object `holder`: a function
/// object running that declaration's entry, at home in `holder`? Anything else
/// in that slot is the program's.
unsafe fn is_declared_static_method_value(
    value: f64,
    owner: u32,
    name: &str,
    holder: *const ObjectHeader,
) -> bool {
    // The function object's home first: one capture read, before the
    // registry lookup of the declaration's entry.
    let v = JSValue::from_bits(value.to_bits());
    v.is_pointer()
        && crate::closure::is_closure_ptr(v.as_pointer::<u8>() as usize)
        && crate::closure::js_closure_get_capture_bits(
            v.as_pointer::<crate::closure::ClosureHeader>(),
            0,
        ) == crate::value::js_nanbox_pointer(holder as i64).to_bits()
        && super::super::class_registry::class_own_static_method_code(owner, name).is_some_and(
            |code| {
                super::class_object_template::static_method_value_runs(
                    value.to_bits(),
                    code,
                    holder,
                )
            },
        )
}

/// May a call `C.name(..)` on class object `object` run the registered static
/// method (with `this` = `object`)? The evaluation that declares `name` keeps
/// it as an own property of its class object: when that object still holds the
/// declaration the registry runs it; when it no longer does (deleted, or the
/// program stored something else) the property decides, not the registry. A
/// declaration of a shared class (no class object) is the registry's.
pub(crate) unsafe fn class_object_registry_serves_static(
    object: *const ObjectHeader,
    name: &str,
) -> bool {
    let Some((owner, _)) =
        super::super::class_registry::lookup_static_method_owner((*object).class_id, name)
    else {
        return false;
    };
    if !super::super::class_registry::template_has_class_objects(owner) {
        return true;
    }
    let mut holder = object;
    for _ in 0..32 {
        if (*holder).class_id == owner {
            return super::super::class_registry::class_object_own_field_bytes(
                holder,
                name.as_bytes(),
            )
            .is_some_and(|v| is_declared_static_method_value(v, owner, name, holder));
        }
        let Some(parent) = super::super::class_registry::class_object_pinned_parent(holder) else {
            break;
        };
        let parent = JSValue::from_bits(parent.to_bits());
        if !parent.is_pointer()
            || !super::super::class_registry::is_class_object_ptr(parent.as_pointer::<u8>())
        {
            break;
        }
        holder = parent.as_pointer::<ObjectHeader>();
    }
    // No evaluation of the declaring template is in this object's chain.
    true
}

/// Does `obj` (a class object) own `key` without storing it? Only `prototype`:
/// `{ !w, !e, !c }`, created with the class, so it is there until the object
/// is gone and no `delete` or `defineProperty` can change that.
pub(crate) fn class_object_has_prototype_property(key: &[u8]) -> bool {
    key == b"prototype"
}

/// `Function.prototype.toString` of a class object: the class's source text,
/// as for the class's function object (`class_ref_to_string`). Every
/// evaluation of a class shares its source, so the template id names it.
/// `None` when `value` is not a class object.
pub(crate) fn class_object_source_text(value: f64) -> Option<String> {
    if !super::super::class_registry::is_class_object_value(value) {
        return None;
    }
    let obj = JSValue::from_bits(value.to_bits()).as_pointer::<ObjectHeader>();
    // SAFETY: `is_class_object_value` proved a live class object.
    let class_id = unsafe { (*obj).class_id };
    Some(super::super::class_registry::class_ref_to_string(class_id).into_owned())
}

/// The text `String(C)` / `` `${C}` `` produce for the class object `value`
/// when no `toString` of the program's is in the way, else `None`. An own
/// `toString` property of the object, whatever it holds (a non-callable one
/// makes the conversion throw), and a static `toString` the object or a class
/// it inherits from still declares answer instead: the caller's ordinary
/// conversion finds them. A declared static `toString` the evaluation no
/// longer holds is not in the way.
pub(crate) fn class_object_default_to_string(value: f64) -> Option<String> {
    let text = class_object_source_text(value)?;
    let obj = JSValue::from_bits(value.to_bits()).as_pointer::<ObjectHeader>();
    // SAFETY: a live class object (above).
    if super::super::class_registry::class_object_owns_key_bytes(obj, b"toString")
        || unsafe { class_object_registry_serves_static(obj, "toString") }
    {
        return None;
    }
    Some(text)
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

/// Resolve `.name` for an `OBJECT_TYPE_CLASS` heap object: the object's OWN
/// `name` (`define_class_object_own_properties` creates it with the object, a
/// `static name` member or `defineProperty` replaces it), else nothing. A
/// `delete C.name` removes the property for this object only, and it stays gone
/// (`None`): the template's registered name is not a second source for it.
pub(super) unsafe fn class_object_name_value(
    obj: *const ObjectHeader,
    key: *const crate::StringHeader,
) -> Option<JSValue> {
    own_data_field_by_name(obj, key)
}

/// #6530 (size-gate split from `get_field_by_name_tail.rs` — pure
/// relocation): resolve the `constructor` special key for an instance
/// receiver. Own `constructor` data field wins; then WeakMap/WeakSet,
/// the per-evaluation class-object registry (capture-carrying classes),
/// vtable `constructor` methods, boxed primitives, the
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
    // #11759 (c′): an instance of a later evaluation of a declaration whose
    // first evaluation is the class function object reads `constructor` from
    // its own evaluation's prototype (the generic walk).
    if crate::object::class_value::class_value_is_first_evaluation(class_id)
        && super::super::prototype_chain::object_static_prototype(obj as usize).is_some()
    {
        return None;
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
    // #11868: an anonymous shape is not a constructor identity. Let the
    // ordinary lookup read Object.prototype's own property, including edits,
    // deletion and accessors. The writable global Object binding is unrelated.
    if class_id != 0 && is_anon_shape_class_id(class_id) {
        return None;
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
        // it without the general chain walk, whose receiver juggling,
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
        return Some(JSValue::from_bits(
            crate::object::class_value::class_value(class_id).to_bits(),
        ));
    }
    // Ordinary objects without a class id also use the generic walk, which
    // respects a null prototype instead of synthesizing a constructor.
    None
}

#[cfg(test)]
mod evaluation_owner_tests {
    use super::*;

    #[test]
    fn evaluation_prototype_miss_never_reads_template_properties() {
        unsafe {
            const CID: u32 = 62_443;
            crate::object::js_register_class_id(CID);
            crate::object::js_register_class_name(CID, b"Ownership".as_ptr(), 9);
            let scope = crate::gc::RuntimeHandleScope::new();
            let declared = scope.root_nanbox_f64(
                super::super::super::class_registry::class_decl_prototype_value(CID),
            );
            let key =
                scope.root_string_ptr(crate::string::js_string_from_bytes(b"extra".as_ptr(), 5));
            key.with_const_ptr(|key: *const crate::StringHeader| {
                js_object_set_field_by_name(
                    crate::value::js_nanbox_get_pointer(declared.get_nanbox_f64())
                        as *mut ObjectHeader,
                    key,
                    1.0,
                );
            });
            let class = scope.root_raw_mut_ptr(
                super::super::class_object_template::js_class_evaluation_object(
                    CID,
                    0,
                    0,
                    std::ptr::null(),
                ) as *mut ObjectHeader,
            );
            let prototype = class.with_const_ptr(|class: *const ObjectHeader| {
                class_evaluation_prototype_value(class)
            });
            let prototype = scope.root_nanbox_f64(prototype);
            key.with_const_ptr(|key: *const crate::StringHeader| {
                let obj = crate::value::js_nanbox_get_pointer(prototype.get_nanbox_f64())
                    as *const ObjectHeader;
                assert_ne!(
                    prototype.get_nanbox_f64().to_bits(),
                    declared.get_nanbox_f64().to_bits()
                );
                assert!(js_object_get_field_by_name(obj, key).is_undefined());
                assert_eq!(
                    js_object_get_field_by_name(
                        crate::value::js_nanbox_get_pointer(declared.get_nanbox_f64())
                            as *const ObjectHeader,
                        key,
                    )
                    .as_number(),
                    1.0,
                );
            });
        }
    }

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
