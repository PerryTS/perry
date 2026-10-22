//! Default-derived Construct uses the original argument list. Its own-field
//! entry and constructor kind are immutable ClassDeclaration image facts.
use super::class_registry::declarations::{class_declaration, CLASS_DECL_DEFAULT_DERIVED};

pub(crate) unsafe fn try_construct(
    class_id: u32,
    owner: Option<f64>,
    this: f64,
    args_ptr: *const f64,
    args_len: usize,
) -> Option<f64> {
    let declaration = class_declaration(class_id)?;
    if declaration.flags & CLASS_DECL_DEFAULT_DERIVED == 0 {
        return None;
    }
    let initializer = declaration.default_derived_initializer;
    debug_assert!(!initializer.is_null());
    let scope = crate::gc::RuntimeHandleScope::new();
    let receiver = scope.root_nanbox_f64(this);
    let owner = owner.map(|value| scope.root_nanbox_f64(value));
    let args = if args_ptr.is_null() {
        &[][..]
    } else {
        std::slice::from_raw_parts(args_ptr, args_len)
    };
    let args = scope.root_nanbox_f64_slice(args);
    let owner = owner
        .map(|h| h.get_nanbox_f64())
        .unwrap_or_else(|| super::class_value::class_value(class_id));
    let _evaluation = super::class_registry::push_active_class_evaluation(owner);
    let parent = scope.root_nanbox_f64(super::js_object_get_prototype_of(owner));
    let _new_target = super::SuperNewTargetScope::bind(&scope, receiver.get_nanbox_f64());
    let args = crate::gc::RuntimeHandleScope::refreshed_nanbox_f64_slice(&args);
    let result = super::class_registry::js_class_value_super_construct_site(
        parent.get_nanbox_f64(),
        receiver.get_nanbox_f64(),
        args.as_ptr(),
        args.len(),
        std::ptr::null(),
    );
    let result =
        super::class_registry::js_ctor_return_override(receiver.get_nanbox_f64(), result, 0);
    receiver.set_nanbox_f64(result);
    // The entry takes the already bound derived this. It neither forwards a
    // fixed parameter band nor replays any superclass field initializer.
    let initialize =
        crate::closure::body_call::js_bare_body_fn_unwind!(initializer as *const u8; a0);
    initialize(receiver.get_nanbox_f64());
    Some(receiver.get_nanbox_f64())
}
