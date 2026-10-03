//! The live `[[Prototype]]` chain a `super` reference reads.
//!
//! `super.name` is a property lookup on the home object's CURRENT
//! `[[Prototype]]` (the class prototype for an instance member, the class
//! constructor for a static one), with `this` as the receiver. These helpers
//! find that base and call or read through it when a relink, a patch or a
//! non-modeled base means the declared class tables no longer describe it.
//!
//! Split out of `class_constructors.rs` to keep that file under the 2,000-line
//! CI gate.

use super::class_constructors::pinned_class_object_for_ancestor;

/// The class object of the evaluation a `super` reference in class
/// `home_cid`'s methods belongs to, when one is pinned: repeated evaluations
/// share a template id, so the template tables cannot name it.
#[inline]
pub(crate) fn super_home_owner(home_cid: u32, this_value: f64) -> Option<f64> {
    super::field_get_set::current_private_lexical_brand_value(home_cid).or_else(|| {
        super::field_get_set::private_evaluation_brand_value(this_value)
            .and_then(|owner| pinned_class_object_for_ancestor(owner, home_cid))
    })
}

/// The object `super` reads from in a method whose home object belongs to class
/// `home_cid`: that home object's current `[[Prototype]]`. The home object is
/// the class prototype for an instance method and the class constructor for a
/// static one.
///
/// `None` when the declared-chain lookups must answer instead: the home is not
/// this template's class value (a per-evaluation class object), or the home
/// still links to its declared parent and that parent's declared chain
/// reaches a builtin, native or function-valued base, whose members this
/// runtime does not model as properties of a prototype object.
///
/// # Safety
/// Reads class registry state; `home_owner` must be a live value or `None`.
pub(crate) unsafe fn class_super_base(
    home_cid: u32,
    parent_cid: u32,
    home_owner: Option<f64>,
    is_static: bool,
) -> Option<f64> {
    if home_cid == 0 || !super::is_class_id_registered(home_cid) {
        return None;
    }
    let class_value = super::class_value::class_value(home_cid);
    if home_owner.is_some_and(|owner| owner.to_bits() != class_value.to_bits()) {
        return None;
    }
    let home = if is_static {
        class_value
    } else {
        super::class_registry::class_decl_prototype_value(home_cid)
    };
    if !crate::value::JSValue::from_bits(home.to_bits()).is_pointer() {
        return None;
    }
    // Materializing the declared parent's prototype can allocate.
    let scope = crate::gc::RuntimeHandleScope::new();
    let base = scope.root_nanbox_f64(super::js_object_get_prototype_of(home));
    if parent_cid == 0 {
        return None;
    }
    let declared = if is_static {
        super::class_value::class_value(parent_cid)
    } else {
        super::class_registry::class_decl_prototype_value(parent_cid)
    };
    let base = base.get_nanbox_f64();
    (base.to_bits() != declared.to_bits() || declared_chain_is_user_classes(parent_cid))
        .then_some(base)
}

/// Was the `[[Prototype]]` of a class constructor on the declared chain from
/// `home_cid` up to (not including) `owner_cid` relinked, so that the declared
/// static lookup no longer describes it? One latch load answers `false` in a
/// process that never set a user prototype.
pub(super) fn static_chain_relinked(home_cid: u32, owner_cid: u32) -> bool {
    if !super::prototype_chain::any_class_chain_relinked() {
        return false;
    }
    let mut cur = home_cid;
    for _ in 0..64 {
        if cur == owner_cid {
            return false;
        }
        let Some(parent) = crate::object::get_parent_class_id(cur).filter(|p| *p != 0 && *p != cur)
        else {
            return false;
        };
        // A constructor nobody has seen as a value cannot have been relinked.
        if let Some(ctor) = super::class_value::class_value_if_minted(cur) {
            let proto =
                super::js_object_get_prototype_of(crate::value::js_nanbox_pointer(ctor as i64));
            if proto.to_bits() != super::class_value::class_value(parent).to_bits() {
                return true;
            }
        }
        cur = parent;
    }
    false
}

/// The live super base for `super.key` in a method whose home belongs to class
/// `home_cid`, when the declared lookups may answer differently: only after a
/// relink, because they read the prototype objects and class function
/// objects, so patched, deleted and accessor members already apply. Asked
/// once a user prototype override exists; may allocate.
#[cold]
#[inline(never)]
pub(crate) unsafe fn super_get_live_base(
    home_cid: u32,
    parent_cid: u32,
    receiver: f64,
) -> Option<f64> {
    let is_static = super::class_ref_id(receiver).is_some()
        || super::class_registry::is_class_object_value(receiver);
    let relinked = if is_static {
        static_chain_relinked(home_cid, 0)
    } else {
        declared_chain_has_relinked_prototype(home_cid)
    };
    if !relinked {
        return None;
    }
    let owner = super_home_owner(home_cid, receiver);
    class_super_base(home_cid, parent_cid, owner, is_static)
}

/// Does the declared chain from `cid` consist of compiled user classes only,
/// ending in a class that extends nothing? Then every member on it is a
/// property of a prototype object (or class constructor) the runtime reads.
pub(super) fn declared_chain_is_user_classes(cid: u32) -> bool {
    let mut cur = cid;
    for _ in 0..64 {
        if cur == 0 || cur >= 0xFFFF_0000 || !super::is_class_id_registered(cur) {
            return false;
        }
        match crate::object::get_parent_class_id(cur) {
            Some(p) if p != 0 && p != cur => cur = p,
            _ => {
                // No parent edge: a recorded heritage value means a function,
                // native or builtin base.
                return crate::value::JSValue::from_bits(
                    super::class_registry::parent_static::template_dynamic_parent_value(cur)
                        .to_bits(),
                )
                .is_undefined();
            }
        }
    }
    false
}

/// `super.name(...args)` with `base` as the super base: `base.[[Get]](name,
/// this)`, called with `this_value` as receiver. A null base or a
/// non-callable value throws the call's TypeError.
///
/// # Safety
/// `args_ptr` must point to `args_len` valid `f64`s (or be null when
/// `args_len == 0`).
pub(super) unsafe fn super_call_on_live_base(
    name: &str,
    this_value: f64,
    args_ptr: *const f64,
    args_len: usize,
    base: f64,
) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let base = scope.root_nanbox_f64(base);
    super_call_on_relinked_chain(name, this_value, args_ptr, args_len, |key, receiver| {
        let base = base.get_nanbox_f64();
        let jv = crate::value::JSValue::from_bits(base.to_bits());
        if jv.is_null() || jv.is_undefined() {
            return None;
        }
        let key = f64::from_bits(crate::value::JSValue::string_ptr(key as *mut _).bits());
        Some(crate::value::JSValue::from_bits(
            crate::proxy::js_reflect_get(base, key, receiver).to_bits(),
        ))
    })
}

/// Is the prototype of `cid` or of one of its declared ancestors relinked by a
/// user operation? Asked only after the declared-member lookups missed.
pub(super) fn declared_chain_has_relinked_prototype(cid: u32) -> bool {
    let mut cur = cid;
    for _ in 0..32 {
        if cur == 0 {
            return false;
        }
        if super::class_registry::class_decl_prototype_relinked(cur) {
            return true;
        }
        match crate::object::get_parent_class_id(cur) {
            Some(p) if p != cur => cur = p,
            _ => return false,
        }
    }
    false
}

/// `super.name(...args)` resolved by `read` on a relinked chain: call the value
/// with `this_value` as receiver, or throw the TypeError a call of a
/// non-callable `super.name` throws.
///
/// # Safety
/// `args_ptr` must point to `args_len` valid `f64`s (or be null when
/// `args_len == 0`).
pub(super) unsafe fn super_call_on_relinked_chain(
    name: &str,
    this_value: f64,
    args_ptr: *const f64,
    args_len: usize,
    read: impl FnOnce(*const crate::StringHeader, f64) -> Option<crate::value::JSValue>,
) -> f64 {
    // The key allocation and the read (a getter on the new chain) can collect;
    // the receiver and the arguments ride across them in handles.
    let scope = crate::gc::RuntimeHandleScope::new();
    let this_handle = scope.root_nanbox_f64(this_value);
    let args: Vec<f64> = if args_len > 0 && !args_ptr.is_null() {
        std::slice::from_raw_parts(args_ptr, args_len).to_vec()
    } else {
        Vec::new()
    };
    let arg_handles = scope.root_nanbox_f64_slice(&args);
    let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
    let value = if key.is_null() {
        None
    } else {
        read(
            key as *const crate::StringHeader,
            this_handle.get_nanbox_f64(),
        )
    };
    let callable = value.filter(|v| {
        let boxed = f64::from_bits(v.bits());
        v.is_pointer()
            && ((crate::proxy::js_proxy_is_proxy(boxed) == 1
                && crate::proxy::proxy_wraps_callable(boxed))
                || crate::closure::is_closure_ptr(
                    crate::value::js_nanbox_get_pointer(boxed) as usize
                ))
    });
    let Some(method) = callable else {
        crate::error::js_throw_type_error_not_a_function(
            std::ptr::null(),
            0,
            name.as_ptr(),
            name.len(),
        )
    };
    let method_handle = scope.root_nanbox_f64(f64::from_bits(method.bits()));
    let args = crate::gc::RuntimeHandleScope::refreshed_nanbox_f64_slice(&arg_handles);
    if crate::proxy::js_proxy_is_proxy(method_handle.get_nanbox_f64()) == 1 {
        return crate::proxy::call_proxy_value_with_this(
            method_handle.get_nanbox_f64(),
            this_handle.get_nanbox_f64(),
            &args,
        );
    }
    crate::closure::native_call_value_this(
        method_handle.get_nanbox_f64(),
        crate::closure::JsThis::from_f64(this_handle.get_nanbox_f64()),
        args.as_ptr(),
        args.len(),
    )
}
