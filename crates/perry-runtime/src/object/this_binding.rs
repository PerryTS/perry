//! `this` / `new.target` / static-`this` binding state for method dispatch
//! (split out of `object/mod.rs`, behavior-preserving).

use super::*;

use std::cell::{Cell, RefCell};

// Implicit `this` for closure-typed class fields invoked method-style.
//
// Issue #519: when `obj.fn(args)` calls a closure stored as a class field,
// the field-scan dispatch in `js_native_call_method` could not bind `this`
// through the closure ABI, which then had no `this` slot (it has one since
// this-as-a-parameter stage 1, `perry_abi::JS_BODY_THIS_PARAM`, but bodies
// still read this cell). Hono's RegExpRouter does this with `match = match` (the
// imported function from matcher.js), and the function body's
// `this.buildAllMatchers()` reads `this = 0` and TypeErrors out.
//
// Codegen for `Expr::This` (perry-codegen/src/expr.rs) reads from this
// thread-local when the lexical `this_stack` is empty (i.e. inside a
// non-arrow function body or top-level closure body). The field-scan
// dispatch saves the previous value, sets it to the receiver, calls the
// closure, then restores. Direct function calls (`fn(args)`) don't touch
// this slot, so non-method invocations don't pollute it across calls.
//
// Defaults to `TAG_UNDEFINED`. JS spec says top-level `this` is undefined
// in strict mode, which matches.
// The implicit-`this` cell itself lives INLINE in this thread's
// `tls_hot::HotTls` (`implicit_this`), so the save/restore pair around every
// dynamically-dispatched call — two calls per call — can be performed by
// generated code on Apple aarch64 without entering the runtime at all; the
// accessors below are the portable path and the fallback.
#[inline(always)]
fn implicit_this_cell() -> &'static Cell<u64> {
    &crate::tls_hot::hot().implicit_this
}

/// The `IMPLICIT_THIS.with(|cell| …)` shape the runtime's dispatch paths and
/// prototype thunks already use, re-backed by the inline `HotTls` field above
/// instead of a `perry_thread_local!` slot. Preserving the shape keeps the
/// move invisible at those ~120 call sites while making every one of them one
/// hop shorter — and it is the same cell generated code now reads and writes
/// directly on Apple aarch64.
pub(crate) struct ImplicitThisSlot;

impl ImplicitThisSlot {
    #[inline(always)]
    pub(crate) fn with<R>(&self, f: impl FnOnce(&Cell<u64>) -> R) -> R {
        f(implicit_this_cell())
    }
}

pub(crate) static IMPLICIT_THIS: ImplicitThisSlot = ImplicitThisSlot;

crate::perry_thread_local! {
    pub(crate) static NEW_TARGET: Cell<u64> = const { Cell::new(crate::value::TAG_UNDEFINED) };
    // One-shot receiver override for STATIC method bodies. A compiled static
    // method's `this` slot used to be a compile-time class-ref literal, so
    // `C.m.call({})` / `D.m()` (inherited) ran with `this === C` and static
    // private brand checks could never throw (test262 class/elements
    // static-private-*). Armed by the dynamic dispatch paths that know the
    // real receiver (`js_class_static_method_call`, the Function.prototype
    // call/apply arms for a static bound-method value); consumed (take
    // semantics) by `js_static_this_resolve` in the static-method prologue.
    // Direct compiled calls never arm it, so they keep the lexical class-ref.
    static STATIC_THIS_OVERRIDE: Cell<(bool, u64)> =
        const { Cell::new((false, crate::value::TAG_UNDEFINED)) };
    /// Lexical ClassDefinitionEvaluation owner for static method/accessor
    /// dispatch. Unlike IMPLICIT_THIS, this is not replaced by `.call`'s
    /// visible receiver. A stack makes nested dispatch frame-local.
    static STATIC_PRIVATE_OWNER_STACK: RefCell<crate::exception::CatchStack<u64>> = const {
        RefCell::new(crate::exception::CatchStack::new(
            crate::exception::catch_subsystem::STATIC_PRIVATE_OWNER,
        ))
    };
}

pub(crate) fn static_private_owner_push(value: f64) {
    STATIC_PRIVATE_OWNER_STACK.with(|stack| stack.borrow_mut().push(value.to_bits()));
}

pub(crate) fn static_private_owner_pop() {
    STATIC_PRIVATE_OWNER_STACK.with(|stack| {
        stack.borrow_mut().pop();
    });
}

pub(crate) fn static_private_owner_current() -> Option<f64> {
    STATIC_PRIVATE_OWNER_STACK.with(|stack| stack.borrow().last().copied().map(f64::from_bits))
}

#[inline]
pub(crate) fn static_private_owner_stack_savepoint() -> usize {
    STATIC_PRIVATE_OWNER_STACK.with(|stack| stack.borrow().len())
}

pub(crate) fn static_private_owner_stack_restore(depth: usize) {
    STATIC_PRIVATE_OWNER_STACK.with(|stack| stack.borrow_mut().truncate(depth));
}

/// Arm the static-`this` override unconditionally (used by the call/apply
/// receiver paths, which take precedence over the inner dynamic dispatch).
pub(crate) fn static_this_arm(value: f64) {
    STATIC_THIS_OVERRIDE.with(|c| c.set((true, value.to_bits())));
}

/// Arm the static-`this` override only when no outer caller has already armed
/// it — `js_class_static_method_call` runs INSIDE the call/apply plumbing, and
/// the outermost receiver (the `.call(x)` thisArg) must win.
pub(crate) fn static_this_arm_if_unarmed(value: f64) {
    STATIC_THIS_OVERRIDE.with(|c| {
        if !c.get().0 {
            c.set((true, value.to_bits()));
        }
    });
}

/// Disarm without consuming (paired with arm sites as a safety net in case
/// the invoked target never reached a static-method prologue).
pub(crate) fn static_this_disarm() {
    STATIC_THIS_OVERRIDE.with(|c| c.set((false, crate::value::TAG_UNDEFINED)));
}

/// Arm the static-`this` override with a class constructor ref. Emitted by
/// codegen immediately before a direct call to an INHERITED static method
/// (`D.f()` where `f` lives on a parent class) so the body sees the dispatch
/// base (`this === D`) instead of the lexical defining class — spec
/// OrdinaryCallBindThis for `D.f()`, and what makes static-private brand
/// checks on subclass receivers throw (test262 static-private-method-
/// subclass-receiver).
// #1561-style force-keep: only generated IR calls this.
#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_JS_STATIC_THIS_ARM_CLASSREF: extern "C" fn(u32) = js_static_this_arm_classref;

#[no_mangle]
pub extern "C" fn js_static_this_arm_classref(class_id: u32) {
    if class_id != 0 {
        static_this_arm(native_module::class_constructor_ref_value(class_id));
    }
}

/// Arm the static-`this` override with an arbitrary receiver value. Emitted
/// by the codegen static-dispatch tower (`D.f()` where the receiver is a
/// class-ref expression and the method resolves on a parent class at compile
/// time) right before the direct call.
// #1561-style force-keep: only generated IR calls this.
#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_JS_STATIC_THIS_ARM_VALUE: extern "C" fn(f64) = js_static_this_arm_value;

#[no_mangle]
pub extern "C" fn js_static_this_arm_value(value: f64) {
    static_this_arm(value);
}

/// Static-method prologue `this` resolution: take the armed override if any,
/// else the lexical class-ref the codegen passes in.
// #1561-style force-keep: only generated IR calls this.
#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_JS_STATIC_THIS_RESOLVE: extern "C" fn(f64) -> f64 = js_static_this_resolve;

#[no_mangle]
pub extern "C" fn js_static_this_resolve(default_this: f64) -> f64 {
    STATIC_THIS_OVERRIDE.with(|c| {
        let (armed, bits) = c.get();
        if armed {
            c.set((false, crate::value::TAG_UNDEFINED));
            f64::from_bits(bits)
        } else {
            default_this
        }
    })
}

/// The implicit-`this` cell's raw bits: the receiver a JS body called right
/// now would read. `closure::JsThis::current` is the body-call funnel's
/// spelling of it (through `agent_ptrs` everywhere but Apple aarch64, where
/// this `HotTls` read is the fast path).
#[cfg(all(
    target_vendor = "apple",
    target_arch = "aarch64",
    target_pointer_width = "64"
))]
#[inline(always)]
pub(crate) fn implicit_this_bits() -> u64 {
    implicit_this_cell().get()
}

static THIS_WITNESS_CHECKS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static THIS_WITNESS_MISMATCHES: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static THIS_WITNESS_REPORT: std::sync::Once = std::sync::Once::new();

/// How many mismatches [`js_this_param_witness`] names individually before it
/// only counts.
const THIS_WITNESS_NAMED: u64 = 64;

/// The this-as-a-parameter stage-1 witness: the prologue of every compiled
/// body that reads the implicit-`this` cell calls this with its `this`
/// PARAMETER (`perry_abi::JS_BODY_THIS_PARAM`) and its own name, and it
/// compares the two. Stage 1 requires that every caller passes exactly the
/// receiver the cell holds, so a mismatch names a caller that does not.
///
/// Emitted ONLY by a build compiled with `PERRY_THIS_WITNESS=1` — a
/// COMPILE-time knob, so a product build contains no call and pays nothing.
/// Reported once, on stderr, at exit, as one `PERRY_THIS_WITNESS checks=N
/// mismatches=M` line; the first mismatches are also named as they happen.
/// A leaf: it never allocates on the JS heap and never collects.
#[no_mangle]
pub extern "C" fn js_this_param_witness(param_bits: u64, site: *const u8, site_len: u64) {
    use std::sync::atomic::Ordering;
    THIS_WITNESS_REPORT.call_once(|| unsafe {
        libc::atexit(this_witness_report);
    });
    THIS_WITNESS_CHECKS.fetch_add(1, Ordering::Relaxed);
    let cell_bits = implicit_this_cell().get();
    if cell_bits == param_bits {
        return;
    }
    let n = THIS_WITNESS_MISMATCHES.fetch_add(1, Ordering::Relaxed);
    if n < THIS_WITNESS_NAMED {
        let name = if site.is_null() {
            "?".into()
        } else {
            String::from_utf8_lossy(unsafe { std::slice::from_raw_parts(site, site_len as usize) })
        };
        eprintln!(
            "PERRY_THIS_WITNESS mismatch in {name}: this param={param_bits:#018x} cell={cell_bits:#018x}"
        );
    }
}

/// `(checks, mismatches)` so far (tests).
#[cfg(test)]
pub(crate) fn this_witness_counts() -> (u64, u64) {
    use std::sync::atomic::Ordering;
    (
        THIS_WITNESS_CHECKS.load(Ordering::Relaxed),
        THIS_WITNESS_MISMATCHES.load(Ordering::Relaxed),
    )
}

extern "C" fn this_witness_report() {
    use std::sync::atomic::Ordering;
    eprintln!(
        "PERRY_THIS_WITNESS checks={} mismatches={}",
        THIS_WITNESS_CHECKS.load(Ordering::Relaxed),
        THIS_WITNESS_MISMATCHES.load(Ordering::Relaxed)
    );
}

/// Keepalive anchor — `js_this_param_witness` is called only from generated
/// code of a witness build, so the auto-optimize whole-program build would
/// otherwise dead-strip it.
#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_JS_THIS_PARAM_WITNESS: extern "C" fn(u64, *const u8, u64) = js_this_param_witness;

/// Keepalive anchor — `js_this_coerce_sloppy` is called only from generated
/// code (a sloppy body's receiver prologue).
#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_JS_THIS_COERCE_SLOPPY: extern "C" fn(f64) -> f64 = js_this_coerce_sloppy;

/// Read the current implicit `this` (issue #519).
#[no_mangle]
pub extern "C" fn js_implicit_this_get() -> f64 {
    f64::from_bits(implicit_this_cell().get())
}

/// Read implicit `this` using ordinary (non-strict) function binding rules.
#[no_mangle]
pub extern "C" fn js_implicit_this_get_sloppy() -> f64 {
    js_this_coerce_sloppy(js_implicit_this_get())
}

/// OrdinaryCallBindThis for a non-strict body: `undefined`/`null` become
/// `globalThis`, a boolean/string/number its wrapper object; objects and
/// class refs are unchanged. Emitted at the entry of a sloppy body that reads
/// its receiver (the `this` parameter, `perry_abi::JS_BODY_THIS_PARAM`) —
/// only on the non-object path. Can allocate: a GC safepoint.
#[no_mangle]
pub extern "C" fn js_this_coerce_sloppy(value: f64) -> f64 {
    let jv = crate::value::JSValue::from_bits(value.to_bits());
    if jv.is_undefined() || jv.is_null() {
        return js_get_global_this();
    }
    if jv.is_bool() {
        return crate::builtins::js_boxed_boolean_new(value);
    }
    if jv.is_any_string() {
        return crate::builtins::js_boxed_string_new(value, 1);
    }
    // #5515: a class reference is an INT32-tagged class id, but it is
    // conceptually the class constructor OBJECT, not a primitive number.
    // `C.viaFn()` / `f.call(C)` bind `this` to the class ref; boxing it as a
    // Number here (the `is_int32()` arm below) makes a regular-function static
    // data property observe `this !== C` and lose access to the static chain.
    // Return the class ref unchanged so `this === C` and `this.staticData` work.
    if class_ref_id(value).is_some() {
        return value;
    }
    let bits = value.to_bits();
    if jv.is_int32()
        || (jv.is_number() && ((bits >> 48) != 0 || bits <= crate::gc::GC_HEADER_SIZE as u64))
    {
        return crate::builtins::js_boxed_number_new(value);
    }
    value
}

/// Set the implicit `this` and return the previous value.
/// Callers must restore the previous value to scope the binding to the
/// duration of a single method-style call.
///
/// **The returned previous value is a GC-managed pointer held in a bare Rust
/// local** (#9417, #9445). It is the caller's receiver, and the call this
/// save/restore brackets is user code, so an evacuating young-gen minor inside
/// it moves that object and rewrites every slot the collector can see — a
/// Rust local is not one. Restoring the stale bits then installs a retired
/// from-space address as the caller's `this`, and the failure is silent:
/// `js_object_get_own_field_or_undef` fails its type check on the recycled cell
/// and answers `undefined`, so the caller's next `this.<field>` throws a
/// TypeError naming a property nowhere near the defect. Every runtime site
/// therefore roots the saved value in a `RuntimeHandleScope` and re-reads it at
/// the restore:
///
/// ```ignore
/// let this_scope = crate::gc::RuntimeHandleScope::new();
/// let prev = this_scope.root_nanbox_f64(js_implicit_this_set(receiver));
/// … user code …
/// js_implicit_this_set(prev.get_nanbox_f64());
/// ```
///
/// or, when the restore must also run as the callee unwinds, the
/// [`ImplicitThisScope`] guard, which is that idiom with the restore in `Drop`.
///
/// This is longjmp-safe: `exception.rs` saves and restores the handle stack at
/// trap boundaries, so a throw through the window truncates the scope exactly
/// as a normal drop would. Inside a loop, open the scope PER ITERATION (or
/// reuse an existing per-iteration scope) so the handle stack does not grow by
/// one slot per callback.
///
/// This is part of every dynamically-dispatched call's save/restore path.
/// Keep the shipped path to one TLS replacement; default-off diagnostics here
/// still impose their mode checks millions of times on closure-heavy programs.
#[no_mangle]
pub extern "C" fn js_implicit_this_set(value: f64) -> f64 {
    f64::from_bits(implicit_this_cell().replace(value.to_bits()))
}

/// Bind `IMPLICIT_THIS` for the lifetime of the guard and restore the
/// displaced value on the way out — including when the callee unwinds, which
/// the runtime's `extern "C-unwind"` dispatch surfaces make an ordinary
/// outcome: a plain set/restore pair leaks the receiver into every later
/// implicit-`this` read once a callback throws (#9244).
///
/// The displaced value is the CALLER's receiver, held across the user code the
/// guard brackets, so it lives in a slot of the borrowed `RuntimeHandleScope`
/// — marked, and rewritten when an evacuating minor moves it — and `Drop`
/// re-reads that slot. A guard that keeps it in a plain field is the #9445
/// shape with the restore moved into `Drop`, where a sweep for
/// `let prev = js_implicit_this_set(..)` cannot see it: the private guards this
/// replaced reinstalled a retired from-space address as the caller's `this`
/// after `Object.setPrototypeOf(o, proto); o.run()` ran an allocating method
/// (#10490), and after every `Array.prototype` callback method.
///
/// The borrow is what makes the order safe: the scope must be declared before
/// the guard, so the guard's `Drop` runs while its slot is still on the handle
/// stack.
pub struct ImplicitThisScope<'scope> {
    previous: crate::gc::RuntimeHandle<'scope>,
}

impl<'scope> ImplicitThisScope<'scope> {
    #[inline]
    pub fn bind(scope: &'scope crate::gc::RuntimeHandleScope, receiver: f64) -> Self {
        Self {
            previous: scope.root_nanbox_f64(js_implicit_this_set(receiver)),
        }
    }

    /// Bind `this` to `undefined`: OrdinaryCallBindThis for a callback the
    /// runtime invokes with no receiver (`sort` comparators, `reduce`
    /// callbacks, an absent `thisArg`). Without it the callee reads whatever
    /// the enclosing method dispatch left in the cell (#11419).
    #[inline]
    pub fn bind_undefined(scope: &'scope crate::gc::RuntimeHandleScope) -> Self {
        Self::bind(scope, f64::from_bits(crate::value::TAG_UNDEFINED))
    }
}

impl Drop for ImplicitThisScope<'_> {
    #[inline]
    fn drop(&mut self) {
        js_implicit_this_set(self.previous.get_nanbox_f64());
    }
}

/// Read the current `new.target` value for ordinary function bodies.
#[no_mangle]
pub extern "C" fn js_new_target_get() -> f64 {
    NEW_TARGET.with(|c| f64::from_bits(c.get()))
}

/// Set `new.target` and return the previous value.
#[no_mangle]
pub extern "C" fn js_new_target_set(value: f64) -> f64 {
    NEW_TARGET.with(|c| f64::from_bits(c.replace(value.to_bits())))
}

/// Publish the construction target while a runtime-dispatched super call runs.
/// Inline construction can keep new.target only in generated locals, so recover
/// its identity from the receiver when no dynamic target is already active.
/// An explicit Reflect.construct target must win, even if its class differs.
/// Throws are also covered by the NEW_TARGET catch savepoint below.
pub(crate) struct SuperNewTargetScope<'scope> {
    previous: crate::gc::RuntimeHandle<'scope>,
}

impl<'scope> SuperNewTargetScope<'scope> {
    pub(crate) fn bind(scope: &'scope crate::gc::RuntimeHandleScope, receiver: f64) -> Self {
        let previous = scope.root_nanbox_f64(js_new_target_get());
        if previous.get_nanbox_f64().to_bits() == crate::value::TAG_UNDEFINED {
            let target = super::private_evaluation_brand_value(receiver).or_else(|| unsafe {
                let value = crate::value::JSValue::from_bits(receiver.to_bits());
                if !value.is_pointer() {
                    return None;
                }
                let obj = value.as_pointer::<super::ObjectHeader>();
                if !super::object_is_shaped(obj) || (*obj).class_id == 0 {
                    return None;
                }
                Some(super::class_constructor_ref_value((*obj).class_id))
            });
            if let Some(target) = target {
                js_new_target_set(target);
            }
        }
        Self { previous }
    }
}

impl Drop for SuperNewTargetScope<'_> {
    fn drop(&mut self) {
        js_new_target_set(self.previous.get_nanbox_f64());
    }
}

/// `catch_savepoints!` capture/restore for `IMPLICIT_THIS` (PR #10564 review
/// finding). Several runtime guards displace `IMPLICIT_THIS` around a call
/// they don't control — a `super()` bridge, a prototype-walk accessor
/// dispatch, a stdlib listener/getter dispatcher — with a bare
/// save/call/restore statement sequence, not `ImplicitThisScope`. Neither a
/// `longjmp` nor a system unwind runs the restore statement that follows the
/// call, so a throw crossing one of those sites leaves the callee's receiver
/// installed for every later implicit-`this` read. This closes that gap the
/// same way `runtime_handles`/`call_method` already do: captured at every `try`,
/// replayed by `js_throw` before the exception transports, regardless of
/// transport. It is an unconditional `set`, so it composes safely with a
/// `ImplicitThisScope::drop` that also fires on the unwind path: whichever
/// runs last for a given frame reproduces the same locally-correct value.
#[inline]
pub(crate) fn implicit_this_trap_savepoint() -> u64 {
    implicit_this_cell().get()
}

pub(crate) fn implicit_this_trap_restore(bits: u64) {
    implicit_this_cell().set(bits);
}

/// `catch_savepoints!` capture/restore for `NEW_TARGET`. Same rationale as
/// [`implicit_this_trap_savepoint`]: the Temporal/Intl subclass `super()`
/// bridges (`fetch_globals.rs`, `intl/subclass.rs`) save/restore `new.target`
/// with a bare statement pair around the parent constructor call.
#[inline]
pub(crate) fn new_target_trap_savepoint() -> u64 {
    NEW_TARGET.with(|c| c.get())
}

pub(crate) fn new_target_trap_restore(bits: u64) {
    NEW_TARGET.with(|c| c.set(bits));
}

/// GC mutable-root scanner for the implicit-`this` cell (issue #1813).
///
/// `IMPLICIT_THIS` holds the NaN-boxed receiver for the duration of a
/// dynamically-dispatched non-arrow method body — set then restored by
/// `js_native_call_method` and by the codegen `js_implicit_this_set`
/// save/restore around `js_native_call_value`. That receiver is a live
/// heap object for the whole call, but the cell is plain thread-local
/// storage, so before this scanner it was invisible to GC: not a root.
///
/// When a moving GC runs *during* the method body — e.g. a nested stdlib
/// pump draining network IO for `@perryts/mysql`'s `Pool.acquire` →
/// handshake → `nativeScramble` under concurrent load — the receiver is
/// evacuated/copied. Without a root slot to rewrite, the cell kept the
/// stale pre-move pointer and the body's next `this`-derived dispatch
/// dereferenced freed/relocated memory: the concurrent-load SIGSEGV in
/// `js_native_call_method` reported in #1813. (It only surfaced under
/// memory pressure because nursery copying / old-gen evacuation only move
/// objects then — hence the load-dependent heisenbug.)
///
/// Marking also keeps `this` reachable when the cell is its only root.
/// Non-pointer tags (the `TAG_UNDEFINED` default, plus null/int/bool)
/// flow through `visit_nanbox_bits` as no-ops, so scanning the idle cell
/// is safe.
pub fn scan_implicit_this_roots_mut(visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    {
        let c = implicit_this_cell();
        let mut bits = c.get();
        if visitor.visit_nanbox_u64_slot(&mut bits) {
            c.set(bits);
        }
    }
    NEW_TARGET.with(|c| {
        let mut bits = c.get();
        if visitor.visit_nanbox_u64_slot(&mut bits) {
            c.set(bits);
        }
    });
    STATIC_THIS_OVERRIDE.with(|c| {
        let (armed, mut bits) = c.get();
        if visitor.visit_nanbox_u64_slot(&mut bits) {
            c.set((armed, bits));
        }
    });
    STATIC_PRIVATE_OWNER_STACK.with(|stack| {
        for bits in stack.borrow_mut().iter_mut() {
            visitor.visit_nanbox_u64_slot(bits);
        }
    });
}

crate::perry_thread_local! {
    /// Active inline derived-constructor `super()` binding cells. An arrow
    /// created inside a constructor has its own codegen context, so it cannot
    /// name the outer function's alloca directly; this stack gives it the
    /// exact live binding cell without turning the state into a process-global
    /// boolean.
    static DERIVED_SUPER_BINDING_STACK: std::cell::RefCell<crate::exception::CatchStack<usize>> = const {
        std::cell::RefCell::new(crate::exception::CatchStack::new(
            crate::exception::catch_subsystem::DERIVED_SUPER_BINDING,
        ))
    };
}

#[no_mangle]
pub extern "C" fn js_derived_super_scope_push(slot: *mut u8) {
    DERIVED_SUPER_BINDING_STACK.with(|stack| stack.borrow_mut().push(slot as usize));
}

#[no_mangle]
pub extern "C" fn js_derived_super_scope_pop() {
    DERIVED_SUPER_BINDING_STACK.with(|stack| {
        stack.borrow_mut().pop();
    });
}

#[inline]
pub(crate) fn derived_super_binding_stack_savepoint() -> usize {
    DERIVED_SUPER_BINDING_STACK.with(|stack| stack.borrow().len())
}

pub(crate) fn derived_super_binding_stack_restore(depth: usize) {
    DERIVED_SUPER_BINDING_STACK.with(|stack| stack.borrow_mut().truncate(depth));
}

/// Bind the innermost derived constructor's `this` from a nested arrow. The
/// base constructor has already run when this is called; a duplicate therefore
/// throws at binding time, as required by EvaluateCall(super()).
#[no_mangle]
pub extern "C" fn js_derived_super_bind_current() -> f64 {
    let slot = DERIVED_SUPER_BINDING_STACK.with(|stack| stack.borrow().last().copied());
    let Some(slot) = slot else {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    };
    let slot = slot as *mut u8;
    unsafe {
        if slot.read() != 0 {
            return crate::error::js_throw_reference_error_this_before_super();
        }
        // GC_STORE_AUDIT(STACK): generated derived-constructor binding cell
        // stores a pointer-free byte.
        slot.write(1);
    }
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

/// Throw when a separately-emitted arrow reads the active derived
/// constructor's lexical `this` before `super()` has initialized it.
/// Ordinary functions have no active binding stack entry, so the helper is a
/// cheap no-op for their `this` reads.
#[no_mangle]
pub extern "C" fn js_derived_this_check_current() -> f64 {
    let slot = DERIVED_SUPER_BINDING_STACK.with(|stack| stack.borrow().last().copied());
    if let Some(slot) = slot {
        let slot = slot as *const u8;
        unsafe {
            if slot.read() == 0 {
                return crate::error::js_throw_reference_error_this_before_super();
            }
        }
    }
    f64::from_bits(crate::value::TAG_UNDEFINED)
}
