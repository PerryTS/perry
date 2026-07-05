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
pub static CLASS_CONSTRUCTORS: RwLock<Option<HashMap<u32, (usize, u32, u32)>>> = RwLock::new(None);

/// #1787: register a class's standalone constructor in `CLASS_CONSTRUCTORS`,
/// keyed by the (template) class_id, so `new <classObjectValue>()` can replay
/// the constructor / field initializers on a dynamically-allocated instance.
/// Emitted by codegen at module init alongside the vtable registration.
/// `sig_caps` (#5957): how many of the ctor's TRAILING params are synthesized
/// `__perry_cap_*` capture params IN THE SIGNATURE. The construct dispatchers
/// used to derive the user/cap split from the decl-site SNAPSHOT length —
/// wrong for dynamic-parent classes whose ctors compile CAPLESS signatures
/// (parent fetched via the per-cid registry) while a snapshot IS registered
/// (extends-expr refs join the capture union): the subtraction ate user args
/// (`new WrappedLogged("alpha")` bound the mixin ctor's `seed` to undefined).
#[no_mangle]
pub unsafe extern "C" fn js_register_class_constructor(
    class_id: i64,
    func_ptr: i64,
    param_count: i64,
    sig_caps: i64,
) {
    if class_id == 0 || func_ptr == 0 {
        return;
    }
    let mut guard = CLASS_CONSTRUCTORS.write().unwrap();
    if guard.is_none() {
        *guard = Some(HashMap::new());
    }
    guard.as_mut().unwrap().insert(
        class_id as u32,
        (func_ptr as usize, param_count as u32, sig_caps as u32),
    );
}

/// Look up a class's registered constructor
/// `(fn_ptr, total_param_count, sig_cap_count)`.
fn lookup_class_constructor(class_id: u32) -> Option<(usize, u32, u32)> {
    CLASS_CONSTRUCTORS
        .read()
        .ok()?
        .as_ref()?
        .get(&class_id)
        .copied()
}

/// Per-class-id flags for a registered standalone constructor: whether its
/// trailing param is the HIR-synthesized `arguments` slot and/or a user rest
/// param (`constructor(a, ...rest)`). The `arguments` slot must receive ALL
/// call args (packed from index 0) whereas a user rest slot receives only the
/// args from the rest position onward — the same distinction `call_vtable_
/// method` draws via `has_synthetic_arguments` / `has_rest`. Registered by
/// codegen (`js_register_class_constructor_flags`) alongside the ctor itself so
/// the `super(...spread)` apply path (`js_super_construct_apply`) can forward
/// the flat spread args and let `call_vtable_method` pack the trailing slot
/// correctly. Absent entry ⇒ neither flag (a plain fixed-arity ctor).
static CLASS_CONSTRUCTOR_FLAGS: RwLock<Option<HashMap<u32, (bool, bool)>>> = RwLock::new(None);

/// Codegen FFI: record `(has_synthetic_arguments, has_rest)` for a class ctor.
/// See [`CLASS_CONSTRUCTOR_FLAGS`].
#[no_mangle]
pub extern "C" fn js_register_class_constructor_flags(
    class_id: i64,
    has_synthetic_arguments: i64,
    has_rest: i64,
) {
    if class_id == 0 {
        return;
    }
    let mut guard = CLASS_CONSTRUCTOR_FLAGS.write().unwrap();
    if guard.is_none() {
        *guard = Some(HashMap::new());
    }
    guard.as_mut().unwrap().insert(
        class_id as u32,
        (has_synthetic_arguments != 0, has_rest != 0),
    );
}

/// Keepalive anchor (generated-code-only callee).
#[used]
static KEEP_JS_REGISTER_CLASS_CONSTRUCTOR_FLAGS: extern "C" fn(i64, i64, i64) =
    js_register_class_constructor_flags;

/// Look up a class ctor's `(has_synthetic_arguments, has_rest)` flags.
fn lookup_class_constructor_flags(class_id: u32) -> (bool, bool) {
    CLASS_CONSTRUCTOR_FLAGS
        .read()
        .ok()
        .and_then(|g| g.as_ref().and_then(|m| m.get(&class_id).copied()))
        .unwrap_or((false, false))
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

/// #5437: read one slot of a class's decl-site capture snapshot, falling back
/// to `fallback` (the value the `new`-site appended as the capture arg) when
/// NO snapshot was registered for `class_id`.
///
/// The W6 fix (member-/bare-`new` of a function-nested capturing class) prefers
/// the authoritative decl-site snapshot over the `new`-site appended cap arg
/// because the bundle's multi-level capture chain can materialize a mis-boxed
/// value into that appended arg. But the snapshot only exists for classes that
/// reach the `RegisterClassCaptures` decl-site. An inline anonymous class
/// (`new class { m(){ return capturedLocal } }`) capturing a local whose
/// initializer derives from a `require(...)` result has NO registered snapshot
/// — so the bare snapshot read returned `undefined`, dropping the (correct)
/// appended cap arg. Falling back to `fallback` when the snapshot is absent
/// keeps W6 (snapshot wins when present) while restoring the appended value for
/// the snapshot-less case.
#[no_mangle]
pub extern "C" fn js_class_capture_value_or(class_id: u32, index: u32, fallback: f64) -> f64 {
    CLASS_CAPTURE_VALUES.with(|m| {
        match m.borrow().get(&class_id) {
            // A snapshot exists for this class. The recorded slot is
            // authoritative WHEN IT HOLDS A REAL VALUE (W6: the bundle's
            // multi-level capture chain can materialize a mis-boxed value into
            // the `new`-site appended `fallback`, so the decl-site snapshot of
            // a stable require-result must win over it).
            //
            // #5437 (hoisted-class stale snapshot): the decl-site snapshot is
            // taken at the class's DECLARATION position — and because class
            // declarations hoist to the top of the enclosing function body,
            // that runs BEFORE a captured local assigned LATER in the same body
            // (`class f { m(){ return cache } } const cache = a || await foo()`
            // — the `RegisterClassCaptures` statement is emitted before the
            // captured local's `Let` binding). At that point the captured slot
            // is still `undefined` (TDZ), so the snapshot recorded `undefined`
            // while the bare-`new f(LocalGet…)` site appended the CORRECT
            // post-assignment local. Returning the `undefined` snapshot dropped
            // that live value — every method reading the captured local then
            // saw `undefined` (e.g. `cache.get(…)` → `Cannot read properties of
            // undefined`).
            //
            // Resolve by SLOT value: an `undefined` snapshot slot carries no
            // information, so fall back to the `new`-site appended value; a slot
            // holding a real value stays authoritative (keeps W6). An entirely
            // absent slot (out-of-range index) also falls back. Same shape as
            // the require-derived getSpan fix (no snapshot → fallback), extended
            // to the snapshot-present-but-`undefined`-slot case.
            //
            // #5437 (captured-`undefined` tag-loss, Next.js dynamic/API routes):
            // a capture whose value is *genuinely* `undefined` (the bundle's
            // `let t_ = process.env.X ? fn : void 0` debug logger, `undefined`
            // by default) records `TAG_UNDEFINED` in the snapshot — which is the
            // CORRECT value, not a TDZ artifact. At giant-module scale the
            // `new`-site appended `fallback` for that same capture materializes
            // as a tag-stripped raw word `0x0000_0000_0000_0001` (the low bits of
            // `TAG_UNDEFINED` with the `0x7FFC` NaN-box tag stripped by a
            // multi-level capture mis-box). Blindly preferring that `fallback`
            // over the `undefined` snapshot handed `t_` the non-callable `0x1`,
            // so `null == t_` was false → `t_(…)` was called → "value is not a
            // function" → route init aborted → HTTP 500.
            //
            // A legitimate captured value is ALWAYS either NaN-boxed (top 16 bits
            // ≥ 0x7FF9 for ptr/string/int32/bigint/SSO/special) or a normal
            // IEEE-754 double (non-zero biased exponent). A `fallback` whose top
            // 16 bits are all zero is therefore a tag-stripped/mis-boxed raw word,
            // NEVER a real captured JSValue — so it must not override the
            // snapshot. When the snapshot slot is `undefined` and the fallback is
            // such a corrupt word, the snapshot's `undefined` is authoritative.
            // (A valid fallback over an `undefined` snapshot still wins, keeping
            // the hoisted-class/TDZ fix above.)
            Some(v) => match v.get(index as usize).copied() {
                Some(bits) if bits != crate::value::TAG_UNDEFINED => f64::from_bits(bits),
                slot => {
                    if fallback_is_tag_stripped(fallback) {
                        // Corrupt fallback — trust the snapshot (its `undefined`
                        // for an undefined-valued capture, or `undefined` for an
                        // absent slot).
                        f64::from_bits(slot.unwrap_or(crate::value::TAG_UNDEFINED))
                    } else {
                        fallback
                    }
                }
            },
            // No snapshot registered: use the `new`-site appended cap value —
            // unless it is a tag-stripped/mis-boxed raw word, in which case the
            // only safe interpretation is `undefined` (calling a `0x1` throws).
            None => {
                if fallback_is_tag_stripped(fallback) {
                    f64::from_bits(crate::value::TAG_UNDEFINED)
                } else {
                    fallback
                }
            }
        }
    })
}

/// #5437 (cross-module member-`new`, param-first): inverse of
/// `js_class_capture_value_or` — the LIVE `param` (the value the `new`-site
/// appended as the capture arg) wins WHENEVER IT IS PRESENT; the decl-site
/// snapshot is only consulted when `param` is `undefined`.
///
/// This is the correct policy for the synthesized constructor's capture
/// rebind. A SAME-module `new C(...)` supplies the current (possibly mutated)
/// outer as `param` — and that must NOT be overridden by a stale decl-site
/// snapshot taken when the class was declared:
///
/// ```ignore
/// let x = "a"; class C { constructor(){ this.x = x } } x = "b"; new C();
/// // node: this.x === "b" (the live param), NOT the "a" decl-site snapshot.
/// ```
///
/// A CROSS-MODULE `new ns.C(...)` routes to the runtime construct path
/// (`construct_registered_class_ref`) which supplies NO capture args, so the
/// synthesized cap param arrives `undefined`. In that case — and only that
/// case — we recover the captured value from the class's own decl-site
/// snapshot (the ctor body is compiled in the class's home module, so
/// `class_id` resolves to the real registered snapshot there). If no snapshot
/// (or no slot) exists either, the result is `undefined` — same as the param.
#[no_mangle]
pub extern "C" fn js_param_or_class_capture_value(param: f64, class_id: u32, index: u32) -> f64 {
    if param.to_bits() != crate::value::TAG_UNDEFINED {
        return param;
    }
    // param is `undefined` (cross-module construct dropped the cap arg):
    // recover from the decl-site snapshot when one is registered for this
    // class; otherwise stay `undefined`.
    CLASS_CAPTURE_VALUES.with(|m| {
        m.borrow()
            .get(&class_id)
            .and_then(|v| v.get(index as usize).copied())
            .map(f64::from_bits)
            .unwrap_or(param)
    })
}

/// A legitimate JSValue is either NaN-boxed (top 16 bits ≥ 0x7FF9 — the boxed
/// tags: SSO/BIGINT/special/POINTER/INT32/STRING) or a normal IEEE-754 double.
/// A NON-ZERO value whose top 16 bits are all zero is a positive subnormal
/// (< 2^-996) — a magnitude no program meaningfully captures — and is in
/// practice the `0x7FFC_…_0001 → 0x0000_…_0001` signature of a captured
/// `undefined` (or any heap pointer) that lost its NaN-box tag through a
/// low-bits extraction at giant-module scale. Such a word is never a real
/// captured value, so the capture-snapshot fallback must reject it. See #5437.
///
/// `+0.0`/`-0.0` are excluded: the number `0` is a legitimate captured value
/// (its bits are `0x0000_…_0000` / `0x8000_…_0000`, the latter has a non-zero
/// top 16), and a TDZ fallback that is genuinely `0` must still win.
#[inline]
pub(crate) fn fallback_is_tag_stripped(fallback: f64) -> bool {
    let bits = fallback.to_bits();
    (bits >> 48) == 0 && bits != 0
}

/// Keepalive anchors (generated-code-only callees).
#[used]
static KEEP_JS_CLASS_CAPTURE_VALUE: extern "C" fn(u32, u32) -> f64 = js_class_capture_value;
#[used]
static KEEP_JS_CLASS_CAPTURE_VALUE_OR: extern "C" fn(u32, u32, f64) -> f64 =
    js_class_capture_value_or;
#[used]
static KEEP_JS_PARAM_OR_CLASS_CAPTURE_VALUE: extern "C" fn(f64, u32, u32) -> f64 =
    js_param_or_class_capture_value;

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
        if let Some((ctor_ptr, total_params, sig_caps)) = lookup_class_constructor(cur) {
            if std::env::var_os("PERRY_SUPER_DEBUG").is_some() {
                eprintln!(
                    "super_apply resolved ancestor cid={} total={} sig_caps={}",
                    cur, total_params, sig_caps
                );
            }
            let caps = class_capture_values(cur).unwrap_or_default();
            // #5957: split user/cap slots from the SIGNATURE cap count, NOT the
            // decl-site snapshot length. A dynamic-parent ctor compiles a
            // CAPLESS signature (sig_caps == 0) yet may have a registered
            // snapshot (extends-expr refs joined the capture union) — the old
            // `total - caps.len()` ate user args (`new WrappedLogged("alpha")`
            // bound the mixin ctor's `seed` to undefined).
            let user_params = (total_params as usize).saturating_sub(sig_caps as usize);
            let n = if arr.is_null() {
                0
            } else {
                crate::array::js_array_length(arr)
            } as usize;
            let (has_synth, has_rest_flag) = lookup_class_constructor_flags(cur);
            let (final_args, call_synth, call_rest) = if sig_caps == 0 {
                // No signature cap params: #6018's synth/rest handling applies
                // cleanly — forward all spread args flat and let
                // `call_vtable_method` pack the trailing synthesized-`arguments`
                // (from index 0) or user-rest (from the rest position) slot.
                let mut fa: Vec<f64> = Vec::with_capacity(user_params.max(n));
                if has_synth || has_rest_flag {
                    for i in 0..n {
                        fa.push(crate::array::js_array_get_f64(arr, i as u32));
                    }
                    (fa, has_synth, has_rest_flag)
                } else {
                    for i in 0..user_params {
                        fa.push(if i < n {
                            crate::array::js_array_get_f64(arr, i as u32)
                        } else {
                            undef
                        });
                    }
                    (fa, false, false)
                }
            } else {
                // #5957: the signature HAS trailing `__perry_cap_*` params.
                // Pack the user args ourselves — rest-aware (a `constructor(
                // ...parts)` that ALSO captures combines a rest param with
                // trailing caps, which `call_vtable_method`'s own rest packing
                // would swallow) — then append exactly `sig_caps` snapshot
                // values. Call with synth/rest OFF since we packed the trailing
                // slot manually.
                let mut fa: Vec<f64> = Vec::with_capacity(total_params as usize);
                let rest_idx = crate::closure::lookup_closure_rest(ctor_ptr as *const u8)
                    .map(|ri| ri as usize)
                    .filter(|ri| *ri < user_params);
                if let Some(ri) = rest_idx {
                    for i in 0..ri {
                        fa.push(if i < n {
                            crate::array::js_array_get_f64(arr, i as u32)
                        } else {
                            undef
                        });
                    }
                    let mut rest_arr = crate::array::js_array_alloc(0);
                    let mut i = ri;
                    while i < n {
                        rest_arr = crate::array::js_array_push_f64(
                            rest_arr,
                            crate::array::js_array_get_f64(arr, i as u32),
                        );
                        i += 1;
                    }
                    fa.push(crate::value::js_nanbox_pointer(rest_arr as i64));
                } else {
                    for i in 0..user_params {
                        fa.push(if i < n {
                            crate::array::js_array_get_f64(arr, i as u32)
                        } else {
                            undef
                        });
                    }
                }
                for slot in 0..sig_caps as usize {
                    fa.push(caps.get(slot).map(|b| f64::from_bits(*b)).unwrap_or(undef));
                }
                (fa, false, false)
            };
            let _ = call_vtable_method(
                ctor_ptr,
                this_raw,
                final_args.as_ptr(),
                final_args.len(),
                total_params,
                call_synth,
                call_rest,
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
    // No registered Perry ancestor constructor. A `class X extends
    // Temporal.<Type>` heritage records its parent VALUE (the Temporal ctor
    // closure) at decl time but no class-id edge, so the walk above finds
    // nothing. Recover that value and, if it is a Temporal constructor, run it
    // and stash the returned cell as the subclass instance's brand — the
    // `super(...spread)` counterpart of the `js_fetch_or_value_super` branch
    // that handles non-spread `super(a, b)`. (#5587)
    #[cfg(feature = "temporal")]
    {
        let parent_val = crate::object::class_registry::js_get_dynamic_parent_value(child_cid);
        if crate::object::global_this::temporal_ctor_kind(parent_val).is_some() {
            let this_box = crate::value::js_nanbox_pointer(this_raw);
            let n = if arr.is_null() {
                0
            } else {
                crate::array::js_array_length(arr)
            } as usize;
            let mut flat: Vec<f64> = Vec::with_capacity(n);
            for i in 0..n {
                flat.push(crate::array::js_array_get_f64(arr, i as u32));
            }
            crate::object::global_this::temporal_subclass_super(
                parent_val,
                this_box,
                flat.as_ptr(),
                flat.len(),
            );
        }
    }
    // `class X extends Intl.<Ctor>` via `super(...spread)`: the decl-time parent
    // value is the Intl constructor closure; run it (new.target set) and re-home
    // the branded instance onto `this`, the spread counterpart of the
    // `js_fetch_or_value_super` Intl branch.
    {
        let parent_val = crate::object::class_registry::js_get_dynamic_parent_value(child_cid);
        if crate::intl::is_intl_constructor_value(parent_val) {
            let this_box = crate::value::js_nanbox_pointer(this_raw);
            let n = if arr.is_null() {
                0
            } else {
                crate::array::js_array_length(arr)
            } as usize;
            let mut flat: Vec<f64> = Vec::with_capacity(n);
            for i in 0..n {
                flat.push(crate::array::js_array_get_f64(arr, i as u32));
            }
            crate::intl::intl_subclass_super(parent_val, this_box, flat.as_ptr(), flat.len());
        }
    }
    undef
}

/// Keepalive anchor (generated-code-only callee).
#[used]
static KEEP_JS_SUPER_CONSTRUCT_APPLY: unsafe extern "C" fn(u32, f64, f64) -> f64 =
    js_super_construct_apply;

/// Dynamic `super.method(...)` dispatch for a class whose parent was registered
/// at runtime (`class X extends _mod.default` — wall 38/42). Static codegen
/// can't resolve the parent method (the textual parent name is "default", which
/// matches no compile-time class), so it falls back to this helper: resolve
/// `method_name` starting from the REGISTERED parent of `child_class_id` (NOT
/// the child itself — otherwise the child's own override is re-selected and
/// `super.m()` recurses forever) and invoke it on `this` with a flat f64 arg
/// buffer. Returns `undefined` when the method is not found on the parent chain.
///
/// # Safety
/// `name_ptr` must be valid for `name_len` bytes; `args_ptr` for `args_len`
/// `f64`s (or null when `args_len == 0`).
#[no_mangle]
pub unsafe extern "C" fn js_super_method_call_dynamic(
    child_class_id: u32,
    name_ptr: *const u8,
    name_len: usize,
    this_value: f64,
    args_ptr: *const f64,
    args_len: usize,
) -> f64 {
    let undef = f64::from_bits(crate::value::TAG_UNDEFINED);
    if child_class_id == 0 || name_ptr.is_null() {
        return undef;
    }
    let name = match std::str::from_utf8(std::slice::from_raw_parts(name_ptr, name_len)) {
        Ok(s) => s,
        Err(_) => return undef,
    };
    let parent_cid = match crate::object::get_parent_class_id(child_class_id) {
        Some(p) if p != 0 => p,
        _ => return undef,
    };
    // Static-context super call (`super.m()` inside a `static` method): the
    // receiver is the class constructor (a ClassRef), so resolve the PARENT's
    // STATIC method (not an instance/prototype method) and invoke it with
    // `this` bound to the current class. Refs class/super/in-static-methods.
    if super::class_ref_id(this_value).is_some() {
        if let Some((func_ptr, param_count, has_rest)) =
            super::class_registry::lookup_static_method_in_chain(parent_cid, name)
        {
            let prev_this = crate::object::js_implicit_this_set(this_value);
            crate::object::static_this_arm_if_unarmed(this_value);
            let result = if has_rest {
                // Mirror `js_class_static_method_call`'s rest bundling: fixed
                // positional args, then the remaining args as an array.
                let fixed = (param_count as usize).saturating_sub(1);
                let arr = crate::array::js_array_alloc(args_len.saturating_sub(fixed) as u32);
                let mut i = fixed;
                while i < args_len {
                    crate::array::js_array_push_f64(arr, *args_ptr.add(i));
                    i += 1;
                }
                let rest_box = crate::value::js_nanbox_pointer(arr as i64);
                let mut buf: Vec<f64> = Vec::with_capacity(param_count as usize);
                for j in 0..fixed {
                    buf.push(if j < args_len {
                        *args_ptr.add(j)
                    } else {
                        f64::from_bits(crate::value::TAG_UNDEFINED)
                    });
                }
                buf.push(rest_box);
                super::class_registry::call_static_method(
                    func_ptr,
                    buf.as_ptr(),
                    buf.len(),
                    param_count,
                )
            } else {
                super::class_registry::call_static_method(func_ptr, args_ptr, args_len, param_count)
            };
            crate::object::static_this_disarm();
            crate::object::js_implicit_this_set(prev_this);
            return result;
        }
    }
    // `lookup_class_method_in_chain` resolves under the registry read lock and
    // DROPS it before returning — the invoked method body may take the registry
    // write lock (a lazy `require()` registering a module class), so we must not
    // hold it across the call (the wall-37 deadlock).
    let resolved = super::class_registry::lookup_class_method_in_chain(parent_cid, name);
    if let Some((func_ptr, param_count, has_synth, has_rest)) = resolved {
        let this_raw = (this_value.to_bits() & crate::value::POINTER_MASK) as i64;
        return call_vtable_method(
            func_ptr,
            this_raw,
            args_ptr,
            args_len,
            param_count,
            has_synth,
            has_rest,
        );
    }
    // The parent may be a function-style class whose method lives in the
    // runtime prototype-method registry (`Base.prototype.m = ...` via
    // `js_register_function_prototype_method`, or a synthetic prototype object
    // wired by `js_set_function_prototype`) rather than the class vtable —
    // these never land in `lookup_class_method_in_chain`. `lookup_prototype_method`
    // walks the parent chain and drops its read lock before returning, so the
    // invoked body may re-take the registry lock without deadlocking (wall-37).
    if let Some(method_value) = super::class_registry::lookup_prototype_method(parent_cid, name) {
        let prev_this = super::IMPLICIT_THIS.with(|c| c.replace(this_value.to_bits()));
        let result = crate::closure::js_native_call_value(method_value, args_ptr, args_len);
        super::IMPLICIT_THIS.with(|c| c.set(prev_this));
        return result;
    }
    undef
}

/// Keepalive anchor (generated-code-only callee).
#[used]
static KEEP_JS_SUPER_METHOD_CALL_DYNAMIC: unsafe extern "C" fn(
    u32,
    *const u8,
    usize,
    f64,
    *const f64,
    usize,
) -> f64 = js_super_method_call_dynamic;

/// `super.method(...spread)` dispatch where the argument count is dynamic.
/// Codegen flattens every argument (regular args plus every spread-expanded
/// element) into a single JS array `args_array`, then routes here. We
/// materialise that array into a contiguous flat `f64` buffer and forward to
/// `js_super_method_call_dynamic`, so a `super.emit(event, ...args)` forwarding
/// a rest param to a native base (EventEmitter) delivers the spread elements as
/// individual arguments instead of one array. Without this the plain
/// `SuperMethodCall` lowering passed the spread operand as ONE positional arg.
///
/// # Safety
/// `name_ptr` must be valid for `name_len` bytes. `args_array` is a NaN-boxed
/// array pointer (or any non-array value, treated as zero args).
#[no_mangle]
pub unsafe extern "C" fn js_super_method_call_dynamic_apply(
    child_class_id: u32,
    name_ptr: *const u8,
    name_len: usize,
    this_value: f64,
    args_array: f64,
) -> f64 {
    let arr =
        (args_array.to_bits() & crate::value::POINTER_MASK) as *const crate::array::ArrayHeader;
    let n = if arr.is_null() {
        0usize
    } else {
        crate::array::js_array_length(arr) as usize
    };
    let mut flat: Vec<f64> = Vec::with_capacity(n);
    for i in 0..n {
        flat.push(crate::array::js_array_get_f64(arr, i as u32));
    }
    let (args_ptr, args_len) = if flat.is_empty() {
        (std::ptr::null(), 0usize)
    } else {
        (flat.as_ptr(), flat.len())
    };
    js_super_method_call_dynamic(
        child_class_id,
        name_ptr,
        name_len,
        this_value,
        args_ptr,
        args_len,
    )
}

/// Keepalive anchor (generated-code-only callee).
#[used]
static KEEP_JS_SUPER_METHOD_CALL_DYNAMIC_APPLY: unsafe extern "C" fn(
    u32,
    *const u8,
    usize,
    f64,
    f64,
) -> f64 = js_super_method_call_dynamic_apply;

/// Run the constructor of class `parent_cid` (or its nearest ctor-bearing
/// ancestor) on the EXISTING `this`, taking arguments from a flat f64 buffer —
/// the codegen `super()` ABI. Returns `true` when a constructor was found and
/// invoked.
///
/// Used by `js_fetch_or_value_super` for the `class X extends _mod.default`
/// case where the dynamic parent value resolves to a ClassRef (a real
/// registered Perry class — Next.js `NextNodeServer extends base-server`'s
/// default `Server`). A ClassRef is NaN-tagged, so it is NOT callable via
/// `js_native_call_value` (that path early-returns `undefined`); the base
/// constructor would never run and parent `this.<field> = …` writes would be
/// lost. This invokes the class constructor directly, mirroring
/// `js_super_construct_apply` but starting from `parent_cid` inclusive and
/// reading a flat arg buffer instead of an array handle.
///
/// # Safety
/// `this_raw` must be a valid `ObjectHeader` pointer (as `i64`); `args_ptr`
/// must point to `args_len` valid `f64`s (or be null when `args_len == 0`).
pub(crate) unsafe fn run_class_constructor_on_this_flat(
    parent_cid: u32,
    this_raw: i64,
    args_ptr: *const f64,
    args_len: usize,
) -> bool {
    if this_raw == 0 || parent_cid == 0 {
        return false;
    }
    let undef = f64::from_bits(crate::value::TAG_UNDEFINED);
    let mut cur = parent_cid;
    let mut depth = 0usize;
    while cur != 0 && depth < 64 {
        if let Some((ctor_ptr, total_params, sig_caps)) = lookup_class_constructor(cur) {
            // #5957: signature-truth user/cap split + rest-aware packing (see
            // `js_super_construct_apply`). This flat dispatcher is the one the
            // dynamic-parent super leg reaches (`js_fetch_or_value_super` →
            // ClassRef fallback), so the capless-sig-with-snapshot mixin case
            // resolves here: sig_caps == 0 ⇒ user_params == total_params ⇒ the
            // forwarded user args survive.
            let caps = class_capture_values(cur).unwrap_or_default();
            let user_params = (total_params as usize).saturating_sub(sig_caps as usize);
            if std::env::var_os("PERRY_SUPER_DEBUG").is_some() {
                eprintln!(
                    "flat_super cid={} total={} sig_caps={} snapshot={} args_len={} rest={:?}",
                    cur,
                    total_params,
                    sig_caps,
                    caps.len(),
                    args_len,
                    crate::closure::lookup_closure_rest(ctor_ptr as *const u8)
                );
            }
            let get = |i: usize| -> f64 {
                if !args_ptr.is_null() && i < args_len {
                    unsafe { *args_ptr.add(i) }
                } else {
                    undef
                }
            };
            let mut final_args: Vec<f64> = Vec::with_capacity(total_params as usize);
            let rest_idx = crate::closure::lookup_closure_rest(ctor_ptr as *const u8)
                .map(|ri| ri as usize)
                .filter(|ri| *ri < user_params);
            if let Some(ri) = rest_idx {
                for i in 0..ri {
                    final_args.push(get(i));
                }
                let mut rest_arr = crate::array::js_array_alloc(0);
                let mut i = ri;
                while i < args_len {
                    rest_arr = crate::array::js_array_push_f64(rest_arr, get(i));
                    i += 1;
                }
                final_args.push(crate::value::js_nanbox_pointer(rest_arr as i64));
            } else {
                for i in 0..user_params {
                    final_args.push(get(i));
                }
            }
            for slot in 0..sig_caps as usize {
                final_args.push(caps.get(slot).map(|b| f64::from_bits(*b)).unwrap_or(undef));
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
            return true;
        }
        let next = crate::object::get_parent_class_id(cur).unwrap_or(0);
        if next == cur {
            break;
        }
        cur = next;
        depth += 1;
    }
    false
}

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
    let Some((ctor_ptr, total_params, sig_caps)) = lookup_class_constructor(class_cid) else {
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
    // #5957: user/cap split from the SIGNATURE cap count (not the per-eval /
    // snapshot length — a dynamic-parent ctor is capless while a snapshot
    // exists, and the old `max(per-eval, snapshot)` subtraction ate user args).
    let user_params = (total_params as usize).saturating_sub(sig_caps as usize);
    let undef = f64::from_bits(crate::value::TAG_UNDEFINED);
    let mut final_args: Vec<f64> = Vec::with_capacity(total_params as usize);
    // #wall3: a `constructor(...args)` (rest param) called via the dynamic
    // member-new path (`new ns.Sub(opts)` → js_new_function_construct →
    // is_class_object_value → here) must BUNDLE the trailing call args into a JS
    // array for the rest slot. call_vtable_method's own `has_rest` can't do it
    // because the rest param is NOT last here — the positional `__perry_cap_*`
    // capture params follow it — so we pack the rest array ourselves at the rest
    // index, then append caps. Without this the rest binds to the first arg as a
    // scalar (`args`=opts, not [opts]) and `super(...args)` spreads a bare object
    // → 0x400000000 mis-box → crash (Next.js `new c.AppPageRouteModule({...})`).
    let rest_idx = crate::closure::lookup_closure_rest(ctor_ptr as *const u8)
        .map(|ri| ri as usize)
        .filter(|ri| *ri < user_params);
    if let Some(ri) = rest_idx {
        for i in 0..ri {
            if !args_ptr.is_null() && i < args_len {
                final_args.push(*args_ptr.add(i));
            } else {
                final_args.push(undef);
            }
        }
        let mut rest_arr = crate::array::js_array_alloc(0);
        if !args_ptr.is_null() {
            let mut i = ri;
            while i < args_len {
                rest_arr = crate::array::js_array_push_f64(rest_arr, *args_ptr.add(i));
                i += 1;
            }
        }
        final_args.push(crate::value::js_nanbox_pointer(rest_arr as i64));
    } else {
        for i in 0..user_params {
            if !args_ptr.is_null() && i < args_len {
                final_args.push(*args_ptr.add(i));
            } else {
                final_args.push(undef);
            }
        }
    }
    // Exactly `sig_caps` trailing cap slots: per-evaluation snapshot first
    // (class EXPRESSIONS carry `__perry_ctor_caps`), decl-site snapshot second
    // (class DECLARATIONS reached as heap values), undefined last.
    for slot in 0..sig_caps as usize {
        let v = if (slot as u32) < n_caps {
            crate::array::js_array_get_f64(caps_arr, slot as u32)
        } else if let Some(bits) = snapshot_caps.get(slot) {
            f64::from_bits(*bits)
        } else {
            undef
        };
        final_args.push(v);
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
    let Some((ctor_ptr, total_params, sig_caps)) = lookup_class_constructor(class_cid) else {
        return;
    };

    // A function-nested class declaration may carry a decl-site capture
    // snapshot (see CLASS_CAPTURE_VALUES). The ctor's trailing
    // `__perry_cap_<id>` params are filled from it; user args fill the rest.
    // #5957: the split is the SIGNATURE cap count, not the snapshot length.
    let caps = class_capture_values(class_cid).unwrap_or_default();
    let user_params = (total_params as usize).saturating_sub(sig_caps as usize);

    let undef = f64::from_bits(crate::value::TAG_UNDEFINED);
    let mut final_args: Vec<f64> = Vec::with_capacity(total_params as usize);
    // #wall3: a `constructor(...args)` reached via the dynamic class-REF member-new
    // path (`new ns.Sub(opts)` where ns.Sub resolves to an INT32 ClassRef at
    // runtime → js_new_function_construct → constructor_class_ref_id →
    // construct_registered_class_ref → here) must BUNDLE trailing call args into a
    // JS array for the rest slot. The rest is NOT the last ctor param (positional
    // `__perry_cap_*` capture params follow it), so call_vtable_method's own
    // `has_rest` can't pack it — we pack the rest array ourselves at the rest
    // index, then append caps. Without this the rest binds to the first arg as a
    // scalar (`args`=opts, not [opts]) and `super(...args)` spreads a bare object
    // → 0x400000000 mis-box → crash (Next.js `new c.AppPageRouteModule({...})`).
    let rest_idx = crate::closure::lookup_closure_rest(ctor_ptr as *const u8)
        .map(|ri| ri as usize)
        .filter(|ri| *ri < user_params);
    if let Some(ri) = rest_idx {
        for i in 0..ri {
            if !args_ptr.is_null() && i < args_len {
                final_args.push(*args_ptr.add(i));
            } else {
                final_args.push(undef);
            }
        }
        let mut rest_arr = crate::array::js_array_alloc(0);
        if !args_ptr.is_null() {
            let mut i = ri;
            while i < args_len {
                rest_arr = crate::array::js_array_push_f64(rest_arr, *args_ptr.add(i));
                i += 1;
            }
        }
        final_args.push(crate::value::js_nanbox_pointer(rest_arr as i64));
    } else {
        for i in 0..user_params {
            if !args_ptr.is_null() && i < args_len {
                final_args.push(*args_ptr.add(i));
            } else {
                final_args.push(undef);
            }
        }
    }
    for slot in 0..sig_caps as usize {
        final_args.push(caps.get(slot).map(|b| f64::from_bits(*b)).unwrap_or(undef));
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
