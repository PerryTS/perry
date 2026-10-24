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
    let birth_args = if args_ptr.is_null() {
        &[][..]
    } else {
        unsafe { std::slice::from_raw_parts(args_ptr, args_len) }
    };
    let inst =
        construct_class_object_instance(class_handle.get_nanbox_f64(), class_cid, cell, birth_args);
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

    // The linked birth shape already names this exact evaluation before
    // replay; later instances need no per-object evaluation stamp or lookup.
    // Case 2c: a static family's instance takes its whole layout now, before
    // the constructor prologue stores it (`object/layout_desc.rs`) and before
    // the heritage pin below adds its own key (to the family's overflow).
    let unadopted = inst_handle.get_raw_mut_ptr::<ObjectHeader>();
    let _ = inst_handle.across_mut::<ObjectHeader, _>(|| unsafe {
        crate::object::layout_desc::adopt_family_layout(unadopted)
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
    let prev_new_target_handle = scope.root_new_target_save(prev_new_target);
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
    if ctor_result.to_bits() != current_inst.to_bits()
        && constructor_return_overrides_this(ctor_result)
    {
        return ctor_result;
    }
    // Explicit and default super share the native Construct dispatcher.
    // A relinked constructor must not acquire its original builtin's state.
    current_inst
}

/// The instance `new` through fresh class value `class_value` (template
/// `class_cid`) allocates, linked to that evaluation's distinct prototype
/// object. Class-id dispatch alone follows the shared template and cannot
/// preserve per-evaluation inheritance.
///
/// The instance reserves the class's declared width without owning its keys,
/// as `new` of the shared class does, and moves to that shape's
/// facts at the prototype's identity. The template remembers the link
/// (`class_object_template::record_instance_link`), so every later instance
/// of the same evaluation is born in the linked shape directly.
fn construct_class_object_instance(
    class_value: f64,
    class_cid: u32,
    cell: Option<super::super::field_get_set::TemplateCell>,
    args: &[f64],
) -> *mut ObjectHeader {
    use super::super::field_get_set::TemplateInstance;
    let template = cell.and_then(|cell| unsafe {
        super::super::field_get_set::template_instance(cell, class_value, class_cid)
    });
    let (inst, width) = match template {
        Some(TemplateInstance::Linked(inst)) => return inst,
        Some(TemplateInstance::Birth(inst, width)) => (inst, width),
        None => {
            if let Some(inst) = born_on_evaluation_layout(class_value, class_cid, cell, args) {
                return inst;
            }
            allocate_class_instance(class_cid, args)
        }
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
    // The evaluation's brand and prototype are one transition (one shape
    // minted per evaluation); the stamp below then has nothing to do.
    let holder_brand =
        unsafe { crate::object::shapes::prepare_evaluation_holder(class.get_nanbox_f64().to_bits()) };
    let birth = instance.with_mut_ptr::<ObjectHeader, _>(|instance| unsafe {
        crate::object::shapes::object_shape_stamp(instance)
    });
    instance.with_mut_ptr::<ObjectHeader, _>(|instance| {
        super::super::prototype_chain::object_link_class_evaluation_prototype_branded(
            instance as usize,
            prototype.get_nanbox_u64(),
            holder_brand,
        )
    });
    instance.with_mut_ptr::<ObjectHeader, _>(|instance| unsafe {
        let holder = class.get_nanbox_f64().to_bits();
        if crate::object::shapes::object_evaluation_brand(instance) != holder {
            crate::object::shapes::stamp_evaluation_holder(instance, holder);
        }
    });
    // A class family instance now carries its evaluation's copy of the
    // family layout, branded: the evaluation names it for its next births.
    let copy = instance.with_mut_ptr::<ObjectHeader, _>(|instance| unsafe {
        evaluation_layout_to_record(instance)
    });
    if let Some(copy) = copy {
        // The add roots the class object itself and may move it.
        unsafe {
            super::super::field_get_set::class_object_set_instance_layout(
                class_obj() as *mut ObjectHeader,
                copy,
            )
        };
    }
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

/// `new` through class evaluation `class_value` (template `class_cid`) of a
/// class family: an instance born on the layout the evaluation names
/// (`InternalKey::EvaluationInstanceLayout`), its copy of the family layout
/// that `family_birth_desc` picks for `args`, already naming the
/// evaluation's prototype and brand. One allocation: no prototype funnel, no
/// shape intern, no metadata record. `None` when the evaluation names none
/// yet, or one of another creation-split layout, or one this agent no
/// longer has (`shapes::evaluation_copy_birth_edges`): the caller links the
/// ordinary way and the evaluation records the result.
fn born_on_evaluation_layout(
    class_value: f64,
    class_cid: u32,
    cell: Option<super::super::field_get_set::TemplateCell>,
    args: &[f64],
) -> Option<*mut ObjectHeader> {
    let cell = cell?;
    let class = crate::value::JSValue::from_bits(class_value.to_bits()).as_pointer::<ObjectHeader>();
    let copy = unsafe { super::super::field_get_set::evaluation_instance_layout(cell, class) }?;
    let desc = crate::object::layout_desc::family_birth_desc(class_cid, args)?;
    let family = crate::object::layout_desc::registered_layout_id(unsafe {
        crate::object::layout_desc::load_layout_id(desc)
    })?;
    let (prototype, holder) = crate::object::shapes::evaluation_copy_birth_edges(copy, family)?;
    let inst = unsafe { crate::object::layout_desc::alloc_family_layout_copy(desc, copy) };
    // As the ordinary link and brand stamp do: the edges live in the record.
    crate::gc::runtime_shade_external_edge(prototype);
    crate::gc::runtime_shade_external_edge(holder);
    Some(inst)
}

/// The evaluation copy of a class family layout `inst` was just linked and
/// branded onto, for its class object to name: meta-less, so the copy is
/// everything the birth was given.
unsafe fn evaluation_layout_to_record(inst: *const ObjectHeader) -> Option<u32> {
    if !(*inst).meta.is_null()
        || crate::object::layout_desc::family_of_class((*inst).class_id).is_none()
    {
        return None;
    }
    let word = (*inst).parent_class_id;
    if word & crate::object::layout_desc::LAYOUT_SHADOW_BIT != 0 {
        return None;
    }
    let copy = crate::object::shapes::object_shape_stamp(inst);
    (crate::object::shapes::evaluation_family_desc_index_plus_one(copy) != 0
        && crate::object::shapes::object_evaluation_brand(inst) != 0)
        .then_some(copy)
}

/// Reserve declared/learned capacity; constructor stores and field definitions
/// establish key membership in execution order, on every runtime construct path.
fn allocate_class_instance(class_cid: u32, args: &[f64]) -> (*mut ObjectHeader, u32) {
    // Case 2c: a static family's instance is exactly its layout's width (the
    // creation split's layout for its constructor arguments); the keys it
    // gains later live in its overflow, not in learned inline room.
    if let Some(descriptor) = crate::object::layout_desc::family_birth_desc(class_cid, args) {
        let width = unsafe { (*descriptor).slot_count };
        return (
            unsafe { crate::object::layout_desc::perry_alloc_layout(descriptor) },
            width,
        );
    }
    let declared = registered_class_keys_array(class_cid).map_or(0, |(_, width)| width);
    let width = declared.max(crate::object::learned_inline_field_count(class_cid));
    (js_object_alloc(class_cid, width), width)
}

/// Object's constructor has special newTarget semantics: when invoked as the
/// super-constructor of a derived class it ignores `value` and performs
/// OrdinaryCreateFromConstructor(newTarget). Calling the ordinary Object thunk
/// would instead coerce and return the first argument, binding an unrelated
/// object as the derived `this` and losing its class prototype and brand.
unsafe fn construct_object_with_new_target(new_target: f64) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let new_target = scope.root_nanbox_f64(new_target);
    let instance_cid = new_target_class_id(new_target.get_nanbox_f64()).unwrap_or(0);
    let instance = allocate_class_instance(instance_cid, &[]).0;
    let instance = scope.root_raw_mut_ptr(instance);
    let prototype = new_target_custom_object_prototype(new_target.get_nanbox_f64())
        .or_else(global_object_prototype_bits)
        .map(|bits| scope.root_nanbox_u64(bits));
    if let Some(prototype) = prototype {
        instance.with_mut_ptr::<ObjectHeader, _>(|instance| {
            super::super::prototype_chain::object_set_static_prototype(
                instance as usize,
                prototype.get_nanbox_u64(),
            )
        });
    }
    if is_class_object_value(new_target.get_nanbox_f64()) {
        instance.with_mut_ptr::<ObjectHeader, _>(|instance| {
            super::super::field_get_set::stamp_private_birth_evaluation_brand(
                instance,
                new_target.get_nanbox_f64(),
            );
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
            super::super::field_get_set::stamp_private_birth_evaluation_brand(
                receiver,
                parent.get_nanbox_f64(),
            );
        }
    });
}
