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

thread_local! {
    /// Decl-site snapshots of a function-nested class DECLARATION's captured
    /// outer locals, keyed by class_id. Filled by the codegen-emitted
    /// `js_class_register_capture_values` call at the class's source-order
    /// declaration position (parallel to `js_register_class_parent_dynamic`),
    /// consumed by `replay_registered_class_constructor` so dynamic
    /// construction of the class VALUE (`exports.C = C; new mod.C()` — the
    /// webpack / vendored-zod bundle pattern) fills the synthesized
    /// `__perry_cap_<id>` ctor params. Re-running the enclosing function
    /// overwrites the snapshot (last-definition-wins) — exact for the
    /// run-once module-factory pattern these bundles use; class EXPRESSIONS
    /// keep their per-evaluation `__perry_ctor_caps` snapshot instead.
    static CLASS_CAPTURE_VALUES: std::cell::RefCell<HashMap<u32, Vec<u64>>> =
        std::cell::RefCell::new(HashMap::new());
}

/// Codegen FFI: snapshot `len` capture values for `class_id`. See
/// [`CLASS_CAPTURE_VALUES`].
///
/// # Safety
/// `values_ptr` must point at `len` readable f64 slots.
#[no_mangle]
pub unsafe extern "C" fn js_class_register_capture_values(
    class_id: u32,
    values_ptr: *const f64,
    len: usize,
) {
    if class_id == 0 || values_ptr.is_null() {
        return;
    }
    let mut values = Vec::with_capacity(len);
    for i in 0..len {
        values.push((*values_ptr.add(i)).to_bits());
    }
    CLASS_CAPTURE_VALUES.with(|m| {
        m.borrow_mut().insert(class_id, values);
    });
}

/// Keepalive anchor for the auto-optimize whole-program build —
/// `js_class_register_capture_values` is a generated-code-only callee.
#[used]
static KEEP_JS_CLASS_REGISTER_CAPTURE_VALUES: unsafe extern "C" fn(u32, *const f64, usize) =
    js_class_register_capture_values;

/// GC root scan for the capture-value snapshots (registered alongside the
/// other runtime mutable-root scanners in `gc::mod`).
pub fn scan_class_capture_value_roots_mut(visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    CLASS_CAPTURE_VALUES.with(|m| {
        let mut m = m.borrow_mut();
        for values in m.values_mut() {
            for bits in values.iter_mut() {
                visitor.visit_nanbox_u64_slot(bits);
            }
        }
    });
}

/// The decl-site capture snapshot for `class_id`, if one was registered.
fn class_capture_values(class_id: u32) -> Option<Vec<u64>> {
    CLASS_CAPTURE_VALUES.with(|m| m.borrow().get(&class_id).cloned())
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
        false,
        // Capture-forwarding constructor args are materialized positionally
        // above (including any caps), so no trailing rest re-packing here.
        false,
    );
}

/// Replay a registered class declaration constructor for an INT32-tagged
/// ClassRef callee. Unlike class-expression values, class declarations do not
/// carry per-evaluation capture slots on a heap class object, so only the
/// user-provided `new` arguments are forwarded.
pub(crate) unsafe fn replay_registered_class_constructor(
    class_cid: u32,
    inst: *mut ObjectHeader,
    args_ptr: *const f64,
    args_len: usize,
) {
    let Some((ctor_ptr, total_params)) = lookup_class_constructor(class_cid) else {
        return;
    };

    // A function-nested class declaration may carry a decl-site capture
    // snapshot (see CLASS_CAPTURE_VALUES). The ctor's trailing
    // `__perry_cap_<id>` params are filled from it; user args fill the rest.
    let caps = class_capture_values(class_cid).unwrap_or_default();
    let user_params = (total_params as usize).saturating_sub(caps.len());

    let undef = f64::from_bits(crate::value::TAG_UNDEFINED);
    let mut final_args: Vec<f64> = Vec::with_capacity(total_params as usize);
    for i in 0..user_params {
        if !args_ptr.is_null() && i < args_len {
            final_args.push(*args_ptr.add(i));
        } else {
            final_args.push(undef);
        }
    }
    for bits in &caps {
        final_args.push(f64::from_bits(*bits));
    }
    let _ = call_vtable_method(
        ctor_ptr,
        inst as i64,
        final_args.as_ptr(),
        final_args.len(),
        total_params,
        false,
        false,
    );
}
