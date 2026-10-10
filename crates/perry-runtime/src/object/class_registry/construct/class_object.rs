/// Construct an exact class evaluation, before the unrelated builtin tower.
unsafe fn construct_class_object(
    func_value: f64,
    args_ptr: *const f64,
    args_len: usize,
    new_target: f64,
) -> f64 {
    construct_class_object_entry(func_value, args_ptr, args_len, new_target, None, None)
}

unsafe fn construct_class_object_entry(
    func_value: f64,
    args_ptr: *const f64,
    args_len: usize,
    new_target: f64,
    entry: Option<(usize, u32, u32)>,
    capture_slot: Option<(u32, u32)>,
) -> f64 {
    // Root the class object: its pointer is both the constructor value and
    // the private-brand identity for the instance.
    let scope = crate::gc::RuntimeHandleScope::new();
    let class_handle = scope.root_nanbox_f64(func_value);
    let new_target = scope.root_nanbox_f64(new_target);
    let obj = crate::value::JSValue::from_bits(class_handle.get_nanbox_f64().to_bits())
        .as_pointer::<ObjectHeader>();
    let class_cid = js_object_get_class_id(obj);
    // The template's own record, named by the class object; an image
    // static, so it stays put across every allocation below.
    let cell = unsafe { super::super::field_get_set::class_object_template_cell(obj) };
    let inst = construct_class_object_instance(class_handle.get_nanbox_f64(), class_cid, cell);
    // #7280: root the instance across the replay — see the long note
    // in `construct_registered_class_ref`. The replay runs a user
    // constructor body, so a bare `*mut ObjectHeader` held across it
    // is an unrooted receiver and this arm returns the pre-move
    // address. Reproduced by `new C()` where `C = mk()` is a class
    // EXPRESSION value.
    let inst_handle = scope.root_raw_mut_ptr(inst);
    // GetPrototypeFromConstructor runs before the body. A returned object is
    // left alone; only the provisional receiver inherits newTarget.prototype.
    if new_target.get_nanbox_f64().to_bits() != class_handle.get_nanbox_f64().to_bits() {
        if let Some(proto) = new_target_custom_object_prototype(new_target.get_nanbox_f64()) {
            inst_handle.with_mut_ptr::<ObjectHeader, _>(|inst| {
                super::super::prototype_chain::object_set_static_prototype(inst as usize, proto)
            });
        }
    }

    // Every evaluation gets a distinct brand despite sharing its
    // class id. Stamp it before replay, where private access may occur.
    inst_handle.with_mut_ptr::<ObjectHeader, _>(|inst| {
        super::super::field_get_set::stamp_private_evaluation_brand(
            inst,
            class_handle.get_nanbox_f64(),
        );
    });
    inst_handle.with_mut_ptr::<ObjectHeader, _>(|inst| {
        super::pin_instance_constructing_class(inst, class_handle.get_nanbox_f64());
    });
    // Replay the class's registered constructor (instance-field
    // initializers + body) on the fresh instance, filling the
    // capture params from the snapshotted `__perry_ctor_caps`. The
    // mechanism lives in `class_constructors` to keep this file under
    // the 2,000-line CI gate.
    // Publish this exact class evaluation as newTarget while replaying
    // the standalone constructor. Dynamic builtin `super()` uses it
    // to give a replacement receiver the evaluation-specific
    // prototype and private brand (#9503).
    let prev_new_target = crate::object::js_new_target_get();
    let prev_new_target_handle = scope.root_nanbox_f64(prev_new_target);
    let active_new_target = new_target.get_nanbox_f64();
    crate::object::js_new_target_set(active_new_target);
    let prev_current_new_target =
        CURRENT_NEW_TARGET.with(|value| value.replace(active_new_target.to_bits()));
    let prev_current_new_target_handle = scope.root_nanbox_u64(prev_current_new_target);
    let ctor_result = inst_handle.with_mut_ptr::<ObjectHeader, _>(|inst| {
        super::super::class_constructors::construct_class_object_resolved(
            class_handle.get_nanbox_f64(),
            class_cid,
            inst,
            args_ptr,
            args_len,
            entry,
            capture_slot,
        )
    });
    CURRENT_NEW_TARGET.with(|value| value.set(prev_current_new_target_handle.get_nanbox_u64()));
    crate::object::js_new_target_set(prev_new_target_handle.get_nanbox_f64());
    // The standalone constructor publishes its final `this` when a
    // dynamic super-constructor can replace the provisional receiver.
    // A class-object replay used to discard that result and return the
    // allocation above, so writes after `super()` landed on an object
    // that `new` never exposed (#9503). Return an actual replacement
    // immediately; when the constructor retained the allocation, keep
    // the native-backing completion paths below unchanged.
    let current_inst =
        inst_handle.with_mut_ptr::<ObjectHeader, _>(|i| crate::value::js_nanbox_pointer(i as i64));
    if constructor_return_overrides_this(ctor_result)
        && ctor_result.to_bits() != current_inst.to_bits()
    {
        return ctor_result;
    }
    // A template recorded without heritage, of a class with no
    // declared parent, has no builtin in its chain to back.
    let heritage = cell.is_none_or(|cell| unsafe { cell.has_heritage() })
        || get_parent_class_id(class_cid).is_some_and(|parent| parent != 0);
    if !heritage {
        return current_inst;
    }
    // `class X extends Request/Response {}` constructed via the dynamic
    // (class-expression value) path: the replayed ctor's `super()`
    // can't statically route an aliased parent, so attach the native
    // fetch handle here when the registered parent is a fetch builtin
    // and the instance didn't already get one. Refs `@hono/node-server`.
    if let Some(kind) = fetch_parent_kind_in_chain(class_cid) {
        let has_handle = inst_handle.with_mut_ptr::<ObjectHeader, _>(|inst| {
            super::super::field_get_set::fetch_subclass_handle_id(inst as usize).is_some()
        });
        if !has_handle {
            inst_handle.with_mut_ptr::<ObjectHeader, _>(|inst| {
                super::super::attach_fetch_handle_for_construction(inst, kind, args_ptr, args_len)
            });
        }
    }
    // Class-expression values can also extend Promise and reach this
    // dynamic construct path. The synthesized default constructor does
    // not call construct-only builtins as plain functions; attach the
    // Promise backing here, matching the ClassRef path below. An
    // explicit `super(executor)` has already installed it, so avoid
    // invoking the executor twice.
    ensure_promise_subclass_backing(&inst_handle, class_cid, args_ptr, args_len);
    // Re-read: the fetch attachment and Promise executor both allocate.
    return crate::value::js_nanbox_pointer(inst_handle.get_raw_mut_ptr::<ObjectHeader>() as i64);
}

/// The instance `new` through fresh class value `class_value` (template
/// `class_cid`) allocates, linked to that evaluation's distinct prototype
/// object. Class-id dispatch alone follows the shared template and cannot
/// preserve per-evaluation inheritance.
///
/// The instance is born in its class's declared keys, as `new` of the shared
/// class is (`construct_registered_class_ref`), and moves to that shape's
/// facts at the prototype's identity. The template remembers the link
/// (`class_object_template::record_instance_link`), so every later instance
/// of the same evaluation is born in the linked shape directly.
fn construct_class_object_instance(
    class_value: f64,
    class_cid: u32,
    cell: Option<super::super::field_get_set::TemplateCell>,
) -> *mut ObjectHeader {
    use super::super::field_get_set::TemplateInstance;
    let template = cell.and_then(|cell| unsafe {
        super::super::field_get_set::template_instance(cell, class_value, class_cid)
    });
    let (inst, width) = match template {
        Some(TemplateInstance::Linked(inst)) => return inst,
        Some(TemplateInstance::Birth(inst, width)) => (inst, width),
        None => allocate_class_instance(class_cid),
    };
    let scope = crate::gc::RuntimeHandleScope::new();
    let class = scope.root_nanbox_f64(class_value);
    let instance = scope.root_raw_mut_ptr(inst);
    let class_obj = || {
        crate::value::JSValue::from_bits(class.get_nanbox_f64().to_bits())
            .as_pointer::<ObjectHeader>()
    };
    let prototype =
        unsafe { super::super::field_get_set::class_object_prototype_value(class_obj()) };
    let prototype = scope.root_nanbox_u64(prototype.bits());
    let birth = instance.with_mut_ptr::<ObjectHeader, _>(|instance| unsafe {
        crate::object::shapes::object_shape_stamp(instance)
    });
    instance.with_mut_ptr::<ObjectHeader, _>(|instance| {
        super::super::prototype_chain::object_link_class_evaluation_prototype(
            instance as usize,
            prototype.get_nanbox_u64(),
        )
    });
    instance.with_mut_ptr::<ObjectHeader, _>(|instance| unsafe {
        super::super::field_get_set::record_instance_link(
            class_obj(),
            instance,
            birth,
            width,
            prototype.get_nanbox_u64(),
        );
        instance
    })
}

/// An instance of class `class_cid` allocated in its declared keys, and its
/// width; the learned width without keys for a class that registered none.
fn allocate_class_instance(class_cid: u32) -> (*mut ObjectHeader, u32) {
    if let Some((keys_array, field_count)) = registered_class_keys_array(class_cid) {
        // As wide as the class's instances have been learned to grow, so the
        // keys a constructor adds beyond the declared ones stay inline.
        let field_count = field_count.max(crate::object::learned_inline_field_count(class_cid));
        let inst = crate::object::alloc::alloc_class_instance_with_keys(
            class_cid,
            0,
            field_count,
            keys_array,
        );
        (inst, field_count)
    } else {
        let field_count = crate::object::learned_inline_field_count(class_cid);
        (js_object_alloc(class_cid, field_count), field_count)
    }
}

/// Object's constructor has special newTarget semantics: when invoked as the
/// super-constructor of a derived class it ignores `value` and performs
/// OrdinaryCreateFromConstructor(newTarget). Calling the ordinary Object thunk
/// would instead coerce and return the first argument, binding an unrelated
/// object as the derived `this` and losing its class prototype and brand.
unsafe fn construct_object_with_new_target(new_target: f64) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let new_target = scope.root_nanbox_f64(new_target);
    let instance_cid = new_target_class_id(new_target.get_nanbox_f64())
        .unwrap_or_else(|| synthetic_class_id_for_function(new_target.get_nanbox_f64()));
    let instance =
        if let Some((keys_array, field_count)) = registered_class_keys_array(instance_cid) {
            crate::object::alloc::alloc_class_instance_with_keys(
                instance_cid,
                0,
                field_count,
                keys_array,
            )
        } else {
            js_object_alloc(
                instance_cid,
                crate::object::learned_inline_field_count(instance_cid),
            )
        };
    let instance = scope.root_raw_mut_ptr(instance);
    let prototype = new_target_custom_object_prototype(new_target.get_nanbox_f64())
        .or_else(global_object_prototype_bits)
        .map(|bits| scope.root_nanbox_u64(bits));
    let new_target_value = new_target.get_nanbox_f64();
    if is_class_object_value(new_target_value) {
        instance.with_mut_ptr::<ObjectHeader, _>(|instance| {
            super::super::field_get_set::stamp_private_evaluation_brand(instance, new_target_value)
        });
    }
    if let Some(prototype) = prototype {
        instance.with_mut_ptr::<ObjectHeader, _>(|instance| {
            super::super::prototype_chain::object_set_static_prototype(
                instance as usize,
                prototype.get_nanbox_u64(),
            )
        });
    }
    instance.with_mut_ptr::<ObjectHeader, _>(|i| crate::value::js_nanbox_pointer(i as i64))
}

/// Reflect.construct of a class evaluation uses that evaluation's captures
/// and initializes the receiver from the supplied newTarget before the body.
unsafe fn construct_class_object_with_new_target(
    func_value: f64,
    args_ptr: *const f64,
    args_len: usize,
    new_target: f64,
) -> f64 {
    construct_class_object(func_value, args_ptr, args_len, new_target)
}

/// Prepare the caller-rooted receiver before entering an exact parent body.
/// A static subclass has no evaluation brand yet; a fresh subclass keeps its
/// more-derived brand. This is the same setup as the generic replay path.
unsafe fn prepare_super_class_receiver(
    receiver: crate::gc::RuntimeHandle<'_>,
    parent: crate::gc::RuntimeHandle<'_>,
) {
    receiver.with_mut_ptr::<ObjectHeader, _>(|receiver| {
        super::pin_instance_constructing_class(receiver, parent.get_nanbox_f64());
    });
    receiver.with_mut_ptr::<ObjectHeader, _>(|receiver| {
        let value = crate::value::js_nanbox_pointer(receiver as i64);
        if super::super::field_get_set::private_evaluation_brand_value(value).is_none() {
            super::super::field_get_set::stamp_private_evaluation_brand(
                receiver,
                parent.get_nanbox_f64(),
            );
        }
    });
}
