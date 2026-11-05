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
    static STATIC_PRIVATE_OWNER_STACK: RefCell<crate::exception::CatchStack<u64>> =
        const { RefCell::new(crate::exception::CatchStack::new()) };
}

/// Push the static-private owner for the calling frame's dispatch.
#[inline(always)]
pub(crate) fn static_private_owner_push(value: f64) {
    static_private_owner_push_at(crate::exception::frame_mark(), value);
}

/// Push the static-private owner for the dispatch the frame `mark` names.
pub(crate) fn static_private_owner_push_at(mark: usize, value: f64) {
    STATIC_PRIVATE_OWNER_STACK.with(|stack| stack.borrow_mut().push(mark, value.to_bits()));
}

pub(crate) fn static_private_owner_pop() {
    STATIC_PRIVATE_OWNER_STACK.with(|stack| {
        stack.borrow_mut().pop();
    });
}

pub(crate) fn static_private_owner_current() -> Option<f64> {
    STATIC_PRIVATE_OWNER_STACK.with(|stack| stack.borrow().last().copied().map(f64::from_bits))
}

pub(crate) fn static_private_owner_unwind_to(sp: usize) {
    STATIC_PRIVATE_OWNER_STACK.with(|stack| {
        stack.borrow_mut().unwind_to(sp);
    });
}

#[cfg(any(test, target_os = "wasi"))]
pub(crate) fn static_private_owner_restore_depth(depth: usize) {
    STATIC_PRIVATE_OWNER_STACK.with(|stack| {
        stack.borrow_mut().restore_depth(depth);
    });
}

pub(crate) fn static_private_owner_check_marks(sp: usize) {
    STATIC_PRIVATE_OWNER_STACK.with(|stack| {
        crate::exception::marks::check_marks(stack.borrow().marks(), sp, "static_private_owner")
    });
}

#[cfg(any(test, target_os = "wasi"))]
pub(crate) fn static_private_owner_depth() -> usize {
    STATIC_PRIVATE_OWNER_STACK.with(|stack| stack.borrow().len())
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

/// Set `new.target` and return the previous value. Code that sets it for the
/// duration of a call roots the previous value with
/// `RuntimeHandleScope::root_new_target_save` (or `js_new_target_enter`): a
/// landing pad that truncates the handle stack past that save puts the value
/// back when the call throws.
#[no_mangle]
pub extern "C" fn js_new_target_set(value: f64) -> f64 {
    NEW_TARGET.with(|c| f64::from_bits(c.replace(value.to_bits())))
}

/// Compiled `new`: publish `value` as `new.target` around the constructor
/// call and return the displaced value. The save sits on the handle stack
/// above the height the compiled frame read on entering its handler scope, so
/// the frame's own landing pad restores it when the call throws.
#[no_mangle]
pub extern "C" fn js_new_target_enter(value: f64) -> f64 {
    let previous = js_new_target_set(value);
    crate::gc::new_target_save_push(previous.to_bits());
    previous
}

/// Compiled `new`: end the innermost bridge after its call returned.
#[no_mangle]
pub extern "C" fn js_new_target_leave() {
    if let Some(previous) = crate::gc::new_target_save_pop_top() {
        NEW_TARGET.with(|c| c.set(previous));
    }
}

/// A landing pad truncated the handle stack past `new.target` saves: put back
/// the value the oldest of them displaced.
pub(crate) fn new_target_restore_after_throw(bits: u64) {
    NEW_TARGET.with(|c| c.set(bits));
}

/// Publish the construction target while a runtime-dispatched super call runs.
/// Inline construction can keep new.target only in generated locals, so recover
/// its identity from the receiver when no dynamic target is already active.
/// An explicit Reflect.construct target must win, even if its class differs.
/// A throw is covered by the bridge's landing-pad restore.
pub(crate) struct SuperNewTargetScope<'scope> {
    previous: crate::gc::RuntimeHandle<'scope>,
}

impl<'scope> SuperNewTargetScope<'scope> {
    pub(crate) fn bind(scope: &'scope crate::gc::RuntimeHandleScope, receiver: f64) -> Self {
        let previous = scope.root_new_target_save(js_new_target_get());
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
    DERIVED_SUPER_BINDING_STACK.with(|stack| {
        for entry in stack.borrow_mut().iter_mut() {
            visitor.visit_nanbox_u64_slot(&mut entry.this_bits);
        }
    });
}

/// One active derived-constructor activation: its `super()` binding cell, the
/// object it constructs, and its class.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) struct DerivedSuperBinding {
    /// The constructor's `i1` binding cell (a native alloca).
    slot: usize,
    /// The receiver the activation constructs (NaN-boxed; a GC root, see
    /// [`scan_dispatch_binding_roots_mut`]).
    this_bits: u64,
    /// The class whose constructor body this is (0 = not known to codegen).
    class_id: u32,
}

crate::perry_thread_local! {
    /// Active derived-constructor `super()` binding cells. An arrow created
    /// inside a constructor has its own codegen context, so it cannot name the
    /// outer function's alloca directly; this stack gives it the exact live
    /// binding cell without turning the state into a process-global boolean.
    ///
    /// An arrow finds ITS constructor's entry by the receiver it captured and
    /// the class it was written in, never by position: the top entry belongs
    /// to whichever derived constructor is running now, which is another
    /// object's when the arrow (written in a constructor that finished long
    /// ago) runs inside a different constructor's `super()`.
    static DERIVED_SUPER_BINDING_STACK: std::cell::RefCell<crate::exception::CatchStack<DerivedSuperBinding>> =
        const { std::cell::RefCell::new(crate::exception::CatchStack::new()) };
}

/// The binding cell of the innermost active activation constructing
/// `this_bits` as class `class_id` (0 on either side matches any class), or
/// `None` when that constructor is not running: its binding is then settled
/// and an arrow written in it reads `this` freely.
fn derived_binding_slot_for(this_bits: u64, class_id: u32) -> Option<*mut u8> {
    DERIVED_SUPER_BINDING_STACK.with(|stack| {
        stack
            .borrow()
            .iter()
            .rev()
            .find(|entry| {
                entry.this_bits == this_bits
                    && (class_id == 0 || entry.class_id == 0 || entry.class_id == class_id)
            })
            .map(|entry| entry.slot as *mut u8)
    })
}

/// Open the binding scope of the calling compiled constructor. The entry
/// belongs to the caller's frame (`caller_frame_mark`): the constructor's own
/// landing pads keep it; an in-`try` scope of an inlined constructor is closed
/// by its pad action (perry-codegen `stmt::eh_actions`).
#[no_mangle]
#[inline(never)]
pub extern "C" fn js_derived_super_scope_push(slot: *mut u8, this: f64, class_id: u32) {
    derived_super_scope_push_at(crate::exception::caller_frame_mark(), slot, this, class_id);
}

pub(crate) fn derived_super_scope_push_at(mark: usize, slot: *mut u8, this: f64, class_id: u32) {
    let entry = DerivedSuperBinding {
        slot: slot as usize,
        this_bits: this.to_bits(),
        class_id,
    };
    DERIVED_SUPER_BINDING_STACK.with(|stack| stack.borrow_mut().push(mark, entry));
}

#[no_mangle]
pub extern "C" fn js_derived_super_scope_pop() {
    DERIVED_SUPER_BINDING_STACK.with(|stack| {
        stack.borrow_mut().pop();
    });
}

pub(crate) fn derived_super_binding_len() -> usize {
    DERIVED_SUPER_BINDING_STACK.with(|stack| stack.borrow().len())
}

pub(crate) fn derived_super_binding_truncate(depth: usize) {
    DERIVED_SUPER_BINDING_STACK.with(|stack| stack.borrow_mut().truncate(depth));
}

pub(crate) fn derived_super_binding_unwind_to(sp: usize) {
    DERIVED_SUPER_BINDING_STACK.with(|stack| {
        stack.borrow_mut().unwind_to(sp);
    });
}

#[cfg(any(test, target_os = "wasi"))]
pub(crate) fn derived_super_binding_restore_depth(depth: usize) {
    DERIVED_SUPER_BINDING_STACK.with(|stack| {
        stack.borrow_mut().restore_depth(depth);
    });
}

pub(crate) fn derived_super_binding_check_marks(sp: usize) {
    DERIVED_SUPER_BINDING_STACK.with(|stack| {
        crate::exception::marks::check_marks(stack.borrow().marks(), sp, "derived_super_binding")
    });
}

/// Bind the `this` of the derived constructor an arrow was written in (the
/// activation constructing `this` as `class_id`) from that arrow's `super()`.
/// The base constructor has already run when this is called; a duplicate
/// therefore throws at binding time, as required by EvaluateCall(super()).
#[no_mangle]
pub extern "C" fn js_derived_super_bind_current(this: f64, class_id: u32) -> f64 {
    let Some(slot) = derived_binding_slot_for(this.to_bits(), class_id) else {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    };
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

/// Throw when a separately-emitted arrow reads its derived constructor's
/// lexical `this` (`this`, constructed as `class_id`) before that
/// constructor's `super()` has initialized it. A constructor that is no
/// longer running has no entry, and its arrows read `this` freely.
#[no_mangle]
pub extern "C" fn js_derived_this_check_current(this: f64, class_id: u32) -> f64 {
    if let Some(slot) = derived_binding_slot_for(this.to_bits(), class_id) {
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

/// The owner entry belongs to the calling static method's frame
/// (`caller_frame_mark`): its own landing pads keep it, its epilogue pops it.
#[no_mangle]
#[inline(never)]
pub extern "C" fn js_static_method_entry_enter(class_id: u32, this_bits: u64) {
    static_method_entry_enter_at(crate::exception::caller_frame_mark(), class_id, this_bits);
}

fn static_method_entry_enter_at(mark: usize, class_id: u32, this_bits: u64) {
    // Minting the owner's function object allocates: root `this` across it.
    let this_scope = crate::gc::RuntimeHandleScope::new();
    let this = this_scope.root_nanbox_f64(f64::from_bits(this_bits));
    static_private_owner_push_at(mark, super::class_value::class_value(class_id));
    static_this_arm(this.get_nanbox_f64());
}

/// Prologue of the closure-convention entry of a static method declared by a
/// per-evaluation class (`ClassExprFresh`): as [`js_static_method_entry_enter`],
/// except that the body runs in the evaluation the function object belongs
/// to. That evaluation is the function object's one capture, its home class
/// object (`define_class_object_own_properties`), so `const f = C.s; f()` and
/// `C.s.call(D)` read `C`'s environment, as `C.s()` does. A function object
/// without a home (the template's shared function object) runs as
/// [`js_static_method_entry_enter`] does.
#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_JS_STATIC_METHOD_ENTRY_ENTER_HOME: extern "C" fn(u32, u64, i64) =
    js_static_method_entry_enter_home;

#[no_mangle]
#[inline(never)]
pub extern "C" fn js_static_method_entry_enter_home(class_id: u32, this_bits: u64, callee: i64) {
    let mark = crate::exception::caller_frame_mark();
    let home = if callee == 0 {
        crate::value::TAG_UNDEFINED
    } else {
        crate::closure::js_closure_get_capture_bits(
            callee as *const crate::closure::ClosureHeader,
            0,
        )
    };
    let home = f64::from_bits(home);
    let is_home = super::class_registry::is_class_object_value(home)
        && super::js_object_get_class_id(
            crate::value::JSValue::from_bits(home.to_bits()).as_pointer::<super::ObjectHeader>(),
        ) == class_id;
    if !is_home {
        static_method_entry_enter_at(mark, class_id, this_bits);
        return;
    }
    static_private_owner_push_at(mark, home);
    static_this_arm(f64::from_bits(this_bits));
}

/// Prologue of `<method>__eclo`, the code of the function object a
/// per-evaluation class's prototype holds for one of its methods: the body
/// runs in the evaluation the function object belongs to, its home class
/// object (its one capture), as a call through that evaluation's method value
/// always has (`call_vtable_method_value`'s private brand). A function object
/// without a home runs in its receiver's evaluation. Returns the depth
/// [`js_class_method_entry_leave`] restores.
#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_JS_CLASS_METHOD_ENTRY_ENTER_HOME: extern "C" fn(u32, u64, i64) -> i64 =
    js_class_method_entry_enter_home;

#[no_mangle]
#[inline(never)]
pub extern "C" fn js_class_method_entry_enter_home(
    class_id: u32,
    this_bits: u64,
    callee: i64,
) -> i64 {
    // The brand entry belongs to the calling method's frame.
    let mark = crate::exception::caller_frame_mark();
    let home = if callee == 0 {
        crate::value::TAG_UNDEFINED
    } else {
        crate::closure::js_closure_get_capture_bits(
            callee as *const crate::closure::ClosureHeader,
            0,
        )
    };
    let home = f64::from_bits(home);
    let brand = if super::class_registry::is_class_object_value(home)
        && super::js_object_get_class_id(
            crate::value::JSValue::from_bits(home.to_bits()).as_pointer::<super::ObjectHeader>(),
        ) == class_id
    {
        home
    } else {
        super::private_evaluation_brand_value(f64::from_bits(this_bits))
            .unwrap_or_else(|| f64::from_bits(crate::value::TAG_UNDEFINED))
    };
    let depth = derived_super_binding_len();
    super::private_lexical_brand_push_at(mark, brand);
    depth as i64
}

#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_JS_CLASS_METHOD_ENTRY_LEAVE: extern "C" fn(i64) = js_class_method_entry_leave;

/// Epilogue of `<method>__eclo`: drops what
/// [`js_class_method_entry_enter_home`] set up.
#[no_mangle]
pub extern "C" fn js_class_method_entry_leave(depth: i64) {
    super::private_lexical_brand_pop();
    derived_super_binding_truncate(depth as usize);
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
