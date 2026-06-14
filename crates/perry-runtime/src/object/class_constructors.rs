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

/// Codegen FFI: read one slot of a class's decl-site capture snapshot —
/// STATIC method prologue rebinds (statics have no instance to carry the
/// `__perry_cap_*` fields). Absent snapshot/slot reads `undefined`.
#[no_mangle]
pub extern "C" fn js_class_capture_value(class_id: u32, index: u32) -> f64 {
    CLASS_CAPTURE_VALUES.with(|m| {
        m.borrow()
            .get(&class_id)
            .and_then(|v| v.get(index as usize).copied())
            .map(f64::from_bits)
            .unwrap_or(f64::from_bits(crate::value::TAG_UNDEFINED))
    })
}

/// Keepalive anchor (generated-code-only callee).
#[used]
static KEEP_JS_CLASS_CAPTURE_VALUE: extern "C" fn(u32, u32) -> f64 = js_class_capture_value;

/// `super(...spread)` — invoke the closest registered ancestor constructor
/// of `child_cid` on the EXISTING `this`, with args from the materialized
/// `args_array` (dynamic count; the inline-super path needs a static arg
/// list). The ancestor's trailing `__perry_cap_*` params are filled from
/// its decl-site snapshot, mirroring `replay_registered_class_constructor`.
///
/// # Safety
/// `this_value`/`args_array` must be valid NaN-boxed heap pointers.
#[no_mangle]
pub unsafe extern "C" fn js_super_construct_apply(
    child_cid: u32,
    this_value: f64,
    args_array: f64,
) -> f64 {
    let undef = f64::from_bits(crate::value::TAG_UNDEFINED);
    let this_raw = (this_value.to_bits() & crate::value::POINTER_MASK) as i64;
    if std::env::var_os("PERRY_SUPER_DEBUG").is_some() {
        eprintln!(
            "super_apply child={} this_bits={:#x} args_bits={:#x}",
            child_cid,
            this_value.to_bits(),
            args_array.to_bits()
        );
    }
    if this_raw == 0 {
        return undef;
    }
    let arr =
        (args_array.to_bits() & crate::value::POINTER_MASK) as *const crate::array::ArrayHeader;
    let mut cur = crate::object::get_parent_class_id(child_cid).unwrap_or(0);
    let mut depth = 0usize;
    while cur != 0 && depth < 64 {
        if let Some((ctor_ptr, total_params)) = lookup_class_constructor(cur) {
            if std::env::var_os("PERRY_SUPER_DEBUG").is_some() {
                eprintln!(
                    "super_apply resolved ancestor cid={} total={}",
                    cur, total_params
                );
            }
            let caps = class_capture_values(cur).unwrap_or_default();
            let user_params = (total_params as usize).saturating_sub(caps.len());
            let n = if arr.is_null() {
                0
            } else {
                crate::array::js_array_length(arr)
            } as usize;
            let mut final_args: Vec<f64> = Vec::with_capacity(total_params as usize);
            for i in 0..user_params {
                if i < n {
                    final_args.push(crate::array::js_array_get_f64(arr, i as u32));
                } else {
                    final_args.push(undef);
                }
            }
            for bits in &caps {
                final_args.push(f64::from_bits(*bits));
            }
            let _ = call_vtable_method(
                ctor_ptr,
                this_raw,
                final_args.as_ptr(),
                final_args.len(),
                total_params,
                false,
                false,
            );
            return undef;
        }
        let next = crate::object::get_parent_class_id(cur).unwrap_or(0);
        if next == cur {
            break;
        }
        cur = next;
        depth += 1;
    }
    undef
}

/// Keepalive anchor (generated-code-only callee).
#[used]
static KEEP_JS_SUPER_CONSTRUCT_APPLY: unsafe extern "C" fn(u32, f64, f64) -> f64 =
    js_super_construct_apply;

/// Append the spread of `value` to `target` (array handle), handling BOTH
/// real arrays AND array-likes (Perry's `arguments` object is an
/// ObjectHeader with "0".."n-1" + "length" props — `super(...arguments)`
/// spreads it). Returns the (possibly reallocated) target handle.
///
/// # Safety
/// `target` must be a valid ArrayHeader pointer.
#[no_mangle]
pub unsafe extern "C" fn js_array_push_spread_any(
    target: *mut crate::array::ArrayHeader,
    value: f64,
) -> *mut crate::array::ArrayHeader {
    let jv = crate::value::JSValue::from_bits(value.to_bits());
    if !jv.is_pointer() && !jv.is_string() {
        return target;
    }
    let raw = (value.to_bits() & crate::value::POINTER_MASK) as *const u8;
    if raw.is_null() {
        return target;
    }
    // Real array → bulk append.
    let as_arr = crate::array::clean_arr_ptr(raw as *const crate::array::ArrayHeader);
    if !as_arr.is_null() {
        return crate::array::js_array_push_spread_f64(target, as_arr);
    }
    // Array-like object (arguments): read `length`, copy indexed props.
    let obj = raw as *const ObjectHeader;
    let len_key = crate::string::js_string_from_bytes(b"length".as_ptr(), 6);
    let len_v = crate::object::js_object_get_field_by_name(obj, len_key);
    let len_f = f64::from_bits(len_v.bits());
    if !len_f.is_finite() || len_f < 0.0 {
        return target;
    }
    let n = len_f as u32;
    let mut cur = target;
    for i in 0..n {
        let idx = i.to_string();
        let key = crate::string::js_string_from_bytes(idx.as_ptr(), idx.len() as u32);
        let v = crate::object::js_object_get_field_by_name(obj, key);
        cur = crate::array::js_array_push_f64(cur, f64::from_bits(v.bits()));
    }
    cur
}

/// Keepalive anchor (generated-code-only callee).
#[used]
static KEEP_JS_ARRAY_PUSH_SPREAD_ANY: unsafe extern "C" fn(
    *mut crate::array::ArrayHeader,
    f64,
) -> *mut crate::array::ArrayHeader = js_array_push_spread_any;

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

    // A class DECLARATION reached as a heap class object (webpack interop:
    // `t["default"] = PQueue` read back cross-module) has no per-evaluation
    // `__perry_ctor_caps` array — fall back to the decl-site snapshot
    // (CLASS_CAPTURE_VALUES), exactly like the ClassRef replay path. Without
    // this, the trailing `__perry_cap_*` ctor params read the USER args
    // (p-queue's `new PQueue({...})` left `i.default` undefined and
    // `new e.queueClass` threw "undefined is not a constructor").
    let snapshot_caps: Vec<u64> = if n_caps == 0 {
        class_capture_values(class_cid).unwrap_or_default()
    } else {
        Vec::new()
    };
    let effective_caps = (n_caps as usize).max(snapshot_caps.len());
    let user_params = (total_params as usize).saturating_sub(effective_caps);
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
    for bits in &snapshot_caps {
        final_args.push(f64::from_bits(*bits));
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
