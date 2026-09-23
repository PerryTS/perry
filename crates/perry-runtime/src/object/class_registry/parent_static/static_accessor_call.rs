/// Is a class-body `static get`/`set` named `name` declared anywhere on this
/// class-id parent chain? Registry lookup only — it never runs a getter, so it
/// is safe to ask on every static-call miss.
pub(crate) fn static_accessor_in_chain(class_id: u32, name: &str) -> bool {
    let mut cid = class_id;
    let mut depth = 0usize;
    while cid != 0 && depth < 32 {
        if class_own_static_accessor_ptrs(cid, name).is_some() {
            return true;
        }
        match get_parent_class_id(cid) {
            Some(p) if p != 0 && p != cid => {
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
    let callee = crate::object::js_object_get_field_by_name_f64(
        receiver_handle.get_nanbox_u64() as *const ObjectHeader,
        key_handle.get_raw_const_ptr::<crate::StringHeader>(),
    );
    if !crate::collection_iter::is_callable(callee) {
        return None;
    }
    let callee_handle = scope.root_nanbox_f64(callee);
    let args = crate::gc::RuntimeHandleScope::refreshed_nanbox_f64_slice(&arg_handles);
    let receiver = receiver_handle.get_nanbox_f64();
    let prev_this = scope.root_nanbox_f64(crate::object::js_implicit_this_set(receiver));
    let result = crate::closure::js_native_call_value(
        callee_handle.get_nanbox_f64(),
        args.as_ptr(),
        args.len(),
    );
    crate::object::js_implicit_this_set(prev_this.get_nanbox_f64());
    Some(result)
}
