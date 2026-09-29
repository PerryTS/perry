//! `this` / `new.target` / static-`this` binding state for method dispatch
//! (split out of `object/mod.rs`, behavior-preserving).

use super::*;

use std::cell::{Cell, RefCell};

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
    /// dispatch. Unlike the `this` argument, this is not replaced by `.call`'s
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

/// Keepalive anchor — `js_this_coerce_sloppy` is called only from generated
/// code (a sloppy body's receiver prologue).
#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_JS_THIS_COERCE_SLOPPY: extern "C" fn(f64) -> f64 = js_this_coerce_sloppy;
/// [`js_static_this_resolve`] for a static method of class `class_id`: the
/// armed override if any, else the class's function object, cached in the
/// method's own zero-initialised `slot` (the object is pinned, so the cached
/// bits never go stale) — no per-call class-table lookup.
// #1561-style force-keep: only generated IR calls this.
#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_JS_STATIC_THIS_RESOLVE_CLASS: unsafe extern "C" fn(i32, *mut f64) -> f64 =
    js_static_this_resolve_class;

/// # Safety
/// `slot` is null or the calling static method's own `double` cache global.
#[no_mangle]
pub unsafe extern "C" fn js_static_this_resolve_class(class_id: i32, slot: *mut f64) -> f64 {
    let armed = STATIC_THIS_OVERRIDE.with(|c| {
        let (armed, bits) = c.get();
        if armed {
            c.set((false, crate::value::TAG_UNDEFINED));
        }
        armed.then_some(bits)
    });
    if let Some(bits) = armed {
        return f64::from_bits(bits);
    }
    if !slot.is_null() && (*slot).to_bits() != 0 {
        return *slot;
    }
    let value = super::class_value::class_value(class_id as u32);
    if !slot.is_null() {
        // GC_STORE_AUDIT(ROOT): a compiled cache slot holding a PINNED class
        // function object (never moves), also rooted by the class-value table.
        *slot = value;
    }
    value
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

/// `catch_savepoints!` capture/restore for `NEW_TARGET`: the Temporal/Intl
/// subclass `super()` bridges (`fetch_globals.rs`, `intl/subclass.rs`)
/// save/restore `new.target` with a bare statement pair around the parent
/// constructor call, which neither a `longjmp` nor a system unwind runs, so
/// every `try` captures it and `js_throw` replays it.
#[inline]
pub(crate) fn new_target_trap_savepoint() -> u64 {
    NEW_TARGET.with(|c| c.get())
}

pub(crate) fn new_target_trap_restore(bits: u64) {
    NEW_TARGET.with(|c| c.set(bits));
}

/// GC mutable-root scanner for the dispatch-binding cells: `new.target`, the
/// one-shot static-`this` override and the static private-owner stack. Each
/// holds a NaN-boxed heap value (a constructor, a receiver) for the duration
/// of a call, in plain thread-local storage the collector cannot see, so it
/// marks them and rewrites them when a moving collection relocates the value
/// (the #1813 class: a stale pre-move pointer read after a nested collection).
/// Non-pointer tags flow through `visit_nanbox_u64_slot` as no-ops, so an
/// idle cell is safe to scan.
pub fn scan_dispatch_binding_roots_mut(visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
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

/// Prologue of a static method's closure-convention entry
/// (`<static body>__clo`): the call's `this` (the entry's receiver parameter,
/// passed by whatever called the function object: `C.m()`, `f.call(x)`, a bare
/// `f()`) becomes the body's `this`; class `class_id` (the declaring class) is its
/// static-private owner, whatever the receiver. Paired with
/// [`js_static_method_entry_leave`] after the body returns.
// #1561-style force-keep: only generated IR calls this.
#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_JS_STATIC_METHOD_ENTRY_ENTER: extern "C" fn(u32, u64) = js_static_method_entry_enter;

#[no_mangle]
pub extern "C" fn js_static_method_entry_enter(class_id: u32, this_bits: u64) {
    // Minting the owner's function object allocates: root `this` across it.
    let this_scope = crate::gc::RuntimeHandleScope::new();
    let this = this_scope.root_nanbox_f64(f64::from_bits(this_bits));
    static_private_owner_push(super::class_value::class_value(class_id));
    static_this_arm(this.get_nanbox_f64());
}

/// Epilogue of a static method's closure-convention entry: pops the owner the
/// prologue pushed and drops an override the body never consumed.
// #1561-style force-keep: only generated IR calls this.
#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_JS_STATIC_METHOD_ENTRY_LEAVE: extern "C" fn() = js_static_method_entry_leave;

#[no_mangle]
pub extern "C" fn js_static_method_entry_leave() {
    static_private_owner_pop();
    static_this_disarm();
}
