/// Is a class-body `static get`/`set` named `name` declared anywhere on this
/// class-id parent chain? Registry lookup only — it never runs a getter, so it
/// is safe to ask on every static-call miss.
pub(crate) fn static_accessor_in_chain(class_id: u32, name: &str) -> bool {
    let mut cid = class_id;
    let mut depth = 0usize;
    while cid != 0 && depth < 32 {
        if crate::object::class_value::class_static_has_own_accessor(cid, name) {
            return true;
        }
        match get_parent_class_id(cid) {
            Some(p) if p != 0 && p != cid && crate::object::is_class_id_registered(p) => {
                cid = p;
                depth += 1;
            }
            _ => break,
        }
    }
    false
}

/// #10893 — `C.g(args)` where `g` is a static ACCESSOR reached through the
/// class-id parent chain.
///
/// `C.g(args)` is `Get(C, "g")` followed by a call, so a static getter whose
/// value is callable has to be READ and then invoked.
/// `js_class_static_method_call` resolves `name` against the static-method,
/// static-field, native-proto, constructor-prototype, Promise, Array-subclass
/// and parent-closure tables; none of them holds a class-body `static get`,
/// which lives in `CLASS_STATIC_ACCESSORS`.
///
/// Codegen has its own accessor interception
/// (`try_lower_class_static_accessor_call`), but it keys on the STATIC
/// `extends_name` chain, so an accessor declared on a parent reached through a
/// runtime-resolved heritage (`class G extends make() {}`) is invisible to it.
/// The class-id parent chain is the only place that edge exists, which is why
/// the fix belongs at this layer. A codegen-side "this class has a dynamic
/// parent, so read-then-call instead" guess cannot work: it cannot know whether
/// the runtime parent actually carries the accessor, and diverting on the guess
/// loses every inherited builtin static the arms below serve.
/// `class MyArr extends Array {}` is exactly that trap — `extends Array` lowers
/// to a DYNAMIC parent (`Array` is not a user class, so `lookup_class` misses
/// and `extends_expr` is captured), so such a guess also swallows
/// `MyArr.from(...)` / `.of(...)` / `.isArray(...)` (#7541), whose property-GET
/// form is still `undefined`.
///
/// The read itself goes through `js_object_get_field_by_name_f64` — the SAME
/// entry point codegen emits for `const f = C.g` — rather than calling
/// `class_static_accessor_getter_value` directly. That matters: the getter is
/// found on an ANCESTOR, and its captures (`__perry_ctor_caps`) live on the
/// per-evaluation parent class OBJECT, not on the subclass the call started
/// from. Passing the subclass straight to the accessor helper makes it the
/// capture/private OWNER and the getter body then reads `undefined` for every
/// captured binding (#10893's own `cache` Map). The GET path resolves the owner
/// through the static-prototype chain and stashes the original receiver as the
/// accessor-receiver override, so `this` is still the subclass; reusing it
/// keeps both halves right by construction.
///
/// Returns `None` when no accessor of that name is on the chain, or when its
/// value is not callable — both leave the caller's remaining arms intact.
///
/// The getter body is user JS and can collect, so the caller's argument buffer
/// (a raw pointer into a caller-side alloca the collector does not rewrite) is
/// rooted across it and re-read afterwards, as are the receiver and the key.
pub(crate) unsafe fn try_static_accessor_value_call(
    class_id: u32,
    name: &str,
    receiver: f64,
    args_ptr: *const f64,
    args_len: usize,
) -> Option<f64> {
    if !static_accessor_in_chain(class_id, name) {
        return None;
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let arg_handles = if args_ptr.is_null() || args_len == 0 {
        Vec::new()
    } else {
        scope.root_nanbox_f64_slice(std::slice::from_raw_parts(args_ptr, args_len))
    };
    let receiver_handle = scope.root_nanbox_f64(receiver);
    let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
    let key_handle = scope.root_string_ptr(key);
    // Codegen passes the receiver's raw NaN-boxed bits here (a ClassRef is an
    // INT32 box, not a heap pointer); match that encoding exactly.
    // #7341: the key is read INSIDE `with_const_ptr` rather than hoisted into a
    // bare `get_raw_const_ptr`, because the read sits in argument position to a
    // call — the shape `across_*` cannot convert. `js_object_get_field_by_name_f64`
    // is a self-rooting runtime entry point, which is what `with_const_ptr` is for.
    let callee = key_handle.with_const_ptr::<crate::StringHeader, _>(|key_ptr| {
        crate::object::js_object_get_field_by_name_f64(
            receiver_handle.get_nanbox_u64() as *const ObjectHeader,
            key_ptr,
        )
    });
    if !crate::collection_iter::is_callable(callee) {
        return None;
    }
    let callee_handle = scope.root_nanbox_f64(callee);
    let args = crate::gc::RuntimeHandleScope::refreshed_nanbox_f64_slice(&arg_handles);
    Some(crate::closure::native_call_value_this(
        callee_handle.get_nanbox_f64(),
        crate::closure::JsThis::from_f64(receiver_handle.get_nanbox_f64()),
        args.as_ptr(),
        args.len(),
    ))
}

pub(crate) unsafe fn class_static_accessor_getter_value(
    class_id: u32,
    name: &str,
    receiver: f64,
) -> Option<f64> {
    if name.starts_with('#') {
        return private_static_accessor_getter_value(class_id, name, receiver);
    }
    let mut cid = class_id;
    let mut depth = 0usize;
    while cid != 0 && depth < 32 {
        // A per-evaluation class object's own `defineProperty` accessor.
        if let Some(result) = class_dynamic_static_accessor_getter_value(cid, name, receiver) {
            return Some(result);
        }
        // The class function object's own accessor property (ClassBody or
        // `defineProperty`). #10911: reached through the STATIC prototype
        // chain, `receiver` is the class it was found on (the capture/private
        // owner); `this` is the class the read started from, stashed as the
        // accessor-receiver override (effect's `static get ast()`, #10891).
        if let Some((acc, _, _)) = crate::object::class_value::class_static_own_accessor(cid, name)
        {
            return Some(crate::object::class_value::class_static_accessor_call_get(
                acc, receiver,
            ));
        }
        match get_parent_class_id(cid) {
            Some(p) if p != 0 && p != cid && crate::object::is_class_id_registered(p) => {
                cid = p;
                depth += 1;
            }
            _ => break,
        }
    }
    None
}

/// A private static accessor (`static get #x()`): not a property, so it is
/// read from the class's registration and never inherited through a public
/// lookup.
unsafe fn private_static_accessor_getter_value(
    class_id: u32,
    name: &str,
    receiver: f64,
) -> Option<f64> {
    let mut cid = class_id;
    let mut depth = 0usize;
    while cid != 0 && depth < 32 {
        if let Some((getter, _)) = class_registered_static_accessor_ptrs(cid, name) {
            if getter == 0 {
                return Some(f64::from_bits(crate::value::TAG_UNDEFINED));
            }
            let owner = receiver;
            let receiver =
                crate::object::field_get_set::accessor_receiver_override_take().unwrap_or(receiver);
            crate::object::static_this_arm_if_unarmed(receiver);
            crate::object::static_private_owner_push(owner);
            let f = crate::closure::body_call::js_bare_body_fn!(getter as *const u8;);
            let result = f();
            crate::object::static_private_owner_pop();
            crate::object::static_this_disarm();
            return Some(result);
        }
        match get_parent_class_id(cid) {
            Some(p) if p != 0 && p != cid && crate::object::is_class_id_registered(p) => {
                cid = p;
                depth += 1;
            }
            _ => break,
        }
    }
    None
}

/// `C.name = value` where the class (or an ancestor) has an accessor `name`:
/// its setter runs and `true` is returned. A getter-only accessor refuses the
/// write — strict-mode [[Set]] throws a TypeError (#11521); only a private
/// `#x` reports `true` without a setter, its caller decides. `false` when no
/// accessor of that name is on the chain.
pub(crate) unsafe fn class_static_accessor_setter_apply(
    class_id: u32,
    name: &str,
    receiver: f64,
    value: f64,
) -> bool {
    if name.starts_with('#') {
        return private_static_accessor_setter_apply(class_id, name, receiver, value);
    }
    let mut cid = class_id;
    let mut depth = 0usize;
    while cid != 0 && depth < 32 {
        if let Some(applied) =
            class_dynamic_static_accessor_setter_apply(cid, name, receiver, value)
        {
            if !applied {
                throw_static_getter_only(class_id, name);
            }
            return true;
        }
        if let Some((acc, _, _)) = crate::object::class_value::class_static_own_accessor(cid, name)
        {
            if !crate::object::class_value::class_static_accessor_call_set(acc, receiver, value) {
                throw_static_getter_only(class_id, name);
            }
            return true;
        }
        match get_parent_class_id(cid) {
            Some(p) if p != 0 && p != cid && crate::object::is_class_id_registered(p) => {
                cid = p;
                depth += 1;
            }
            _ => break,
        }
    }
    false
}

fn throw_static_getter_only(class_id: u32, name: &str) -> ! {
    let class_name = class_name_for_id(class_id).unwrap_or_default();
    crate::collection_iter::throw_type_error(&format!(
        "Cannot set property {name} of [class {class_name}] which has only a getter"
    ))
}

unsafe fn private_static_accessor_setter_apply(
    class_id: u32,
    name: &str,
    receiver: f64,
    value: f64,
) -> bool {
    let mut cid = class_id;
    let mut depth = 0usize;
    while cid != 0 && depth < 32 {
        if let Some((_, setter)) = class_registered_static_accessor_ptrs(cid, name) {
            if setter != 0 {
                crate::object::static_this_arm_if_unarmed(receiver);
                crate::object::static_private_owner_push(receiver);
                let f = crate::closure::body_call::js_bare_body_fn!(setter as *const u8; value);
                let _ = f(value);
                crate::object::static_private_owner_pop();
                crate::object::static_this_disarm();
            }
            return true;
        }
        match get_parent_class_id(cid) {
            Some(p) if p != 0 && p != cid && crate::object::is_class_id_registered(p) => {
                cid = p;
                depth += 1;
            }
            _ => break,
        }
    }
    false
}
