//! Which heritage a `super()` sees when one class TEMPLATE is evaluated more
//! than once (#9364).
//!
//! `CLASS_DYNAMIC_PARENT_VALUE` is keyed by TEMPLATE class id and is last-wins,
//! so a factory that evaluates its class twice leaves exactly one parent
//! recorded — the last one. That is the value the compiled constructor's
//! `super()` leg reads back (`js_get_dynamic_parent_value`). When the recorded
//! parent is an EARLIER evaluation of the same template, replaying that
//! parent's constructor re-reads the same entry, resolves the same parent, and
//! re-enters the same constructor: unbounded recursion into the stack guard
//! page.
//!
//! ```text
//! function mk(P) { class D extends (P ?? Object) { constructor(d) { super(d); … } } return D; }
//! const A = mk(null);   // stash[D] = Object
//! const B = mk(A);      // stash[D] = A   <- last-wins
//! new B({});            // B.super -> A, A.super -> stash[D] -> A -> …
//! ```
//!
//! Every evaluation already carries its OWN heritage as an own property on its
//! class object (`js_class_object_pin_parent`), and the two consumers that were
//! already per-evaluation — the prototype chain
//! (`class_evaluation_prototype_value`) and the capture snapshot
//! (`pinned_class_object_for_ancestor`) — read it from there. The `super()` leg
//! could not, because a compiled constructor knows only its template class id.
//! This module supplies the missing context: the class OBJECT whose constructor
//! is currently being replayed, so a `super()` inside that replay resolves THAT
//! evaluation's parent instead of the template's.

use super::*;

crate::perry_thread_local! {
    /// NaN-boxed class OBJECTS whose constructors are being replayed on this
    /// thread, innermost last. Pushed by
    /// `class_constructors::replay_class_object_constructor` around the
    /// constructor call and popped by [`ActiveClassEvaluation`]'s `Drop`, so an
    /// unwinding constructor cannot leave a stale frame behind.
    ///
    /// These are live heap pointers held across a user constructor body, so the
    /// class side-table root scanner visits and forwards them
    /// (`gc_roots::scan_class_side_table_roots_mut`).
    pub(crate) static ACTIVE_CLASS_EVALUATIONS: RwLock<Vec<u64>> = RwLock::new(Vec::new());
}

/// Pops the frame [`push_active_class_evaluation`] pushed.
pub(crate) struct ActiveClassEvaluation {
    pushed: bool,
}

impl Drop for ActiveClassEvaluation {
    fn drop(&mut self) {
        if !self.pushed {
            return;
        }
        ACTIVE_CLASS_EVALUATIONS.with(|stack| {
            if let Ok(mut guard) = stack.write() {
                guard.pop();
            }
        });
    }
}

/// Record `class_value` as the evaluation whose constructor is about to run.
///
/// A value that is not a per-evaluation class object records nothing (a class
/// DECLARATION replayed by class id has no evaluation to distinguish), and the
/// returned guard is then inert.
pub(crate) fn push_active_class_evaluation(class_value: f64) -> ActiveClassEvaluation {
    if !is_class_object_value(class_value) {
        return ActiveClassEvaluation { pushed: false };
    }
    let bits = class_value.to_bits();
    let pushed = ACTIVE_CLASS_EVALUATIONS
        .with(|stack| {
            stack.write().ok().map(|mut guard| {
                guard.push(bits);
            })
        })
        .is_some();
    ActiveClassEvaluation { pushed }
}

/// This evaluation's pinned heritage, when `class_id` is the template of the
/// class object whose constructor is currently being replayed.
///
/// Only the innermost frame is consulted: a `super()` leg belongs to the
/// constructor that is running, and each nested replay pushes its own frame, so
/// a chain of same-template evaluations walks one step per level instead of
/// resolving to the same parent forever. `None` means "no per-evaluation answer
/// here" — the caller falls back to the template stash, which is the whole of
/// the pre-#9364 behaviour.
pub(crate) fn active_class_evaluation_parent(class_id: u32) -> Option<f64> {
    if class_id == 0 {
        return None;
    }
    let bits = ACTIVE_CLASS_EVALUATIONS
        .with(|stack| stack.read().ok().and_then(|guard| guard.last().copied()))?;
    let class_value = f64::from_bits(bits);
    if !is_class_object_value(class_value) {
        return None;
    }
    let obj = crate::value::JSValue::from_bits(bits).as_pointer::<ObjectHeader>();
    if obj.is_null() || js_object_get_class_id(obj) != class_id {
        return None;
    }
    class_object_pinned_parent(obj)
}

/// Visit + forward the class objects held by the active replay frames.
pub(crate) fn scan_active_class_evaluations_mut(visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    ACTIVE_CLASS_EVALUATIONS.with(|stack| {
        if let Ok(mut guard) = stack.write() {
            for value_bits in guard.iter_mut() {
                visitor.visit_nanbox_u64_slot(value_bits);
            }
        }
    });
}

/// Does `parent_bits` denote the very class being defined?
///
/// A class is never its own superclass, so such a heritage value is not a
/// usable parent edge — the same rejection `js_register_class_parent_dynamic`
/// already applies to its `register_class` calls (`parent_cid != class_id`).
/// The VALUE stash needs it too, and for the same reason.
///
/// It is the shared-template hazard above in its other half. A function-body
/// class DECLARATION with no captures, no private elements and no computed keys
/// keeps the shared-template lowering and has no per-evaluation class object at
/// all, so both evaluations answer to one class id and `mk(null) === mk(A)`:
///
/// ```text
/// function mk(P) { class D extends (P ?? Object) {} return D; }
/// const A = mk(null);   // stash[D] = Object
/// const B = mk(A);      // A *is* `ClassRef(D)` — a self-edge
/// new B();
/// ```
///
/// Without this guard the second evaluation records `ClassRef(D)` against D, and
/// every `super()` in D's constructor resolves its parent to D and re-enters D's
/// constructor. There is no class object to distinguish the evaluations, so the
/// active-replay override above cannot help here; rejecting the write keeps
/// whichever heritage the earlier evaluation recorded, which is the only
/// heritage this single class id can describe.
///
/// Deliberately limited to the ClassRef (INT32) form: a POINTER-tagged
/// per-evaluation class OBJECT that shares the template id is a *different*
/// class value with its own pinned heritage, and rejecting those would break the
/// factory chains that lowering already models correctly.
pub(crate) fn is_self_heritage_value(class_id: u32, parent_bits: u64) -> bool {
    const INT32_TAG: u64 = 0x7FFE_0000_0000_0000;
    parent_bits & 0xFFFF_0000_0000_0000 == INT32_TAG && parent_bits as u32 == class_id
}

/// #10624: monotone "has any class object ever pinned its own heritage"
/// flag. `js_class_object_pin_parent` arms it before its own write, so
/// anything it can EVER make true (an instance pinned to its constructing
/// class object, or a class_id that has more than one live per-evaluation
/// parent) is only reachable once this is armed. `instanceof`'s value-aware
/// chain walk (`object/instanceof.rs`'s `class_chain_reaches_dynamic`) is
/// gated on it: the overwhelming majority of programs never evaluate a
/// heritage-carrying class expression more than once, and this keeps that
/// case exactly as cheap as it was before this fix (one relaxed-cost atomic
/// load) instead of paying a table lookup at every hop of every
/// `instanceof` check. See `registry_latch.rs`.
pub(crate) static CLASS_OBJECT_HERITAGE_PIN_LATCH: crate::registry_latch::RegistryLatch =
    crate::registry_latch::RegistryLatch::new();

/// Own-property key under which a genuine INSTANCE (constructed via
/// `new <perEvaluationClassObject>()`) remembers which SPECIFIC evaluation
/// built it (#10624).
///
/// `js_class_object_pin_parent`'s pin lives on the CLASS OBJECT and answers
/// "what is MY parent" — `super()`, the prototype chain, and capture
/// resolution above all already consult it. Nothing, though, gave the
/// resulting INSTANCE a way back to that same evaluation: an instance
/// carries only its class's shared TEMPLATE `class_id`
/// (`ObjectHeader.class_id`), identical for every evaluation of the same
/// factory. `instanceof`'s class-chain walk therefore fell back to
/// `CLASS_REGISTRY`/`get_parent_class_id` — the same last-write-wins table
/// `super()` used to read before #9364 — so an instance built from an
/// EARLIER evaluation, checked after a LATER evaluation of the same
/// template has run, could construct correctly (via the class-object pin
/// above) yet fail `instanceof` against its own true parent (the later
/// evaluation's parent shadows it in that shared table).
///
/// Pinning the constructing class object onto the instance too closes that
/// gap: `object/instanceof.rs`'s `class_chain_reaches_dynamic` walks from
/// THIS value, following the exact same per-evaluation pin chain
/// `pinned_class_object_for_ancestor` already walks for capture resolution,
/// instead of the shared class_id table.
pub(crate) const INSTANCE_CONSTRUCTING_CLASS_KEY: &str = "__perry_ctor_class_object";

/// Pin the per-evaluation class OBJECT that is about to construct `inst`
/// onto `inst` itself (#10624). A no-op when `classobj_value` is not itself
/// a per-evaluation class object, or carries no heritage of its own to
/// disambiguate — an ordinary class DECLARATION (or a heritage-less class
/// expression) has none of the ambiguity this exists to resolve, and the
/// plain class_id registry is already exact for those.
pub(crate) fn pin_instance_constructing_class(inst: *mut ObjectHeader, classobj_value: f64) {
    if inst.is_null() || !is_class_object_value(classobj_value) {
        return;
    }
    let class_ptr = crate::value::js_nanbox_get_pointer(classobj_value) as *const ObjectHeader;
    if class_ptr.is_null() || class_object_pinned_parent(class_ptr).is_none() {
        return;
    }
    // `js_class_object_pin_parent` already armed `CLASS_OBJECT_HERITAGE_PIN_LATCH`
    // before writing `class_ptr`'s own pin above (the ordering rule in
    // `registry_latch.rs`) — that write happens-before this one in this
    // thread's program order, so the latch is already armed here.
    let key_bytes = INSTANCE_CONSTRUCTING_CLASS_KEY.as_bytes();
    let key = crate::string::js_string_from_bytes(key_bytes.as_ptr(), key_bytes.len() as u32);
    crate::object::js_object_set_field_by_name(inst, key, classobj_value);
}

/// Read back the pin [`pin_instance_constructing_class`] wrote, or `None`
/// when `obj` was never pinned — including the common case where the latch
/// alone already answers "no" without scanning `obj`'s own fields at all.
pub(crate) fn instance_pinned_constructing_class(obj: *const ObjectHeader) -> Option<f64> {
    if CLASS_OBJECT_HERITAGE_PIN_LATCH.is_idle() {
        return None;
    }
    class_object_own_field_bytes(obj, INSTANCE_CONSTRUCTING_CLASS_KEY.as_bytes())
}

#[cfg(test)]
#[path = "evaluation_heritage/tests.rs"]
mod tests;
