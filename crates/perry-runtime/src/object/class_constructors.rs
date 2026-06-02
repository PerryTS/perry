//! Per-template constructor replay for class EXPRESSIONS used as values
//! (issue #1787, epic #1785 / design #1772).
//!
//! Split out of `object/class_registry.rs` to keep that file under the 2,000-
//! line CI gate. Holds the `CLASS_CONSTRUCTORS` registry, its registration
//! entry point, and the replay helper invoked by the heap-class-object arm of
//! `js_new_function_construct`.

use std::collections::HashMap;
use std::sync::RwLock;

use super::class_registry::call_vtable_method;
use super::ObjectHeader;

/// #1787: per-template constructor function pointers, keyed by the
/// compile-time class_id. The value is `(fn_ptr, total_param_count)`:
/// `fn_ptr` is the standalone `<prefix>__<class>_constructor` LLVM symbol
/// (signature `double(double this, double arg0, ...)` — the same shape as a
/// vtable method, so `call_vtable_method` invokes it), and `total_param_count`
/// is the constructor's full arity (user params plus the synthesized
/// `__perry_cap_<id>` capture params appended by `synthesize_class_captures`).
///
/// Consulted only by the heap-class-object (`OBJECT_TYPE_CLASS`) arm of
/// `js_new_function_construct`: a class EXPRESSION evaluated as a value
/// (`const A = mk(...); new A()`) can't have its constructor inlined at the
/// `new` site (the callee is a runtime value, and the captured environment
/// lived at the evaluation site, not the construction site). So the
/// per-evaluation captures are snapshotted onto the class object (as the
/// `__perry_ctor_caps` own array) and the constructor is replayed here.
/// Top-level class DECLARATIONS keep the INT32 class-ref `new` path and do not
/// consult this table, so registering every class's constructor is
/// behavior-neutral for them.
pub static CLASS_CONSTRUCTORS: RwLock<Option<HashMap<u32, (usize, u32)>>> = RwLock::new(None);

/// #1787: register a class's standalone constructor in `CLASS_CONSTRUCTORS`,
/// keyed by the (template) class_id, so `new <classObjectValue>()` can replay
/// the constructor / field initializers on a dynamically-allocated instance.
/// Emitted by codegen at module init alongside the vtable registration.
#[no_mangle]
pub unsafe extern "C" fn js_register_class_constructor(
    class_id: i64,
    func_ptr: i64,
    param_count: i64,
) {
    if class_id == 0 || func_ptr == 0 {
        return;
    }
    let mut guard = CLASS_CONSTRUCTORS.write().unwrap();
    if guard.is_none() {
        *guard = Some(HashMap::new());
    }
    guard
        .as_mut()
        .unwrap()
        .insert(class_id as u32, (func_ptr as usize, param_count as u32));
}

/// Look up a class's registered constructor `(fn_ptr, total_param_count)`.
fn lookup_class_constructor(class_id: u32) -> Option<(usize, u32)> {
    CLASS_CONSTRUCTORS
        .read()
        .ok()?
        .as_ref()?
        .get(&class_id)
        .copied()
}

/// #1787: replay a class expression's constructor on a freshly-allocated
/// instance. `classobj_value` is the NaN-boxed heap class object the `new`
/// callee resolved to; `class_cid` is its (template) class_id; `inst` is the
/// already-allocated instance; `args_ptr`/`args_len` are the `new`-call args.
///
/// The constructor's parameters are `[user params..., capture params...]`. The
/// `new`-call args fill the user slots; the per-evaluation captures
/// snapshotted onto the class object (`__perry_ctor_caps`, an own array in
/// capture-param order) fill the trailing slots. No-op when the class has no
/// registered constructor.
pub(crate) unsafe fn replay_class_object_constructor(
    classobj_value: f64,
    class_cid: u32,
    inst: *mut ObjectHeader,
    args_ptr: *const f64,
    args_len: usize,
) {
    let Some((ctor_ptr, total_params)) = lookup_class_constructor(class_cid) else {
        return;
    };

    // Read the snapshotted captures (an own array, in capture-param order).
    // Absent → no captures.
    let caps_val = crate::object::js_object_get_own_field_or_undef(
        classobj_value,
        b"__perry_ctor_caps".as_ptr(),
        17,
    );
    let caps_jv = crate::value::JSValue::from_bits(caps_val.to_bits());
    let (caps_arr, n_caps): (*const crate::array::ArrayHeader, u32) = if caps_jv.is_pointer() {
        let arr = caps_jv.as_pointer::<crate::array::ArrayHeader>();
        if arr.is_null() {
            (std::ptr::null(), 0)
        } else {
            (arr, crate::array::js_array_length(arr))
        }
    } else {
        (std::ptr::null(), 0)
    };

    let user_params = (total_params as usize).saturating_sub(n_caps as usize);
    let undef = f64::from_bits(crate::value::TAG_UNDEFINED);
    let mut final_args: Vec<f64> = Vec::with_capacity(total_params as usize);
    for i in 0..user_params {
        if !args_ptr.is_null() && i < args_len {
            final_args.push(*args_ptr.add(i));
        } else {
            final_args.push(undef);
        }
    }
    for j in 0..n_caps {
        final_args.push(crate::array::js_array_get_f64(caps_arr, j));
    }
    let _ = call_vtable_method(
        ctor_ptr,
        inst as i64,
        final_args.as_ptr(),
        final_args.len(),
        total_params,
    );
}
