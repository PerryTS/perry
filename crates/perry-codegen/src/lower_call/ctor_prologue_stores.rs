//! Constructor-free construction for a class whose entire constructor is a run
//! of `this.<field> = <parameter>` stores and typed finite-offset variants.
//!
//! ## What this is for
//!
//! An object literal with a closed shape is not lowered as a literal. HIR mints
//! an anon-shape class for it (`lower/context.rs::mint_anon_shape_class`) and
//! rewrites the site to `new __AnonShape_<hash>(v, w)`, and `lower_new` then
//! routes that through the shared standalone `<Class>_constructor` symbol
//! (`new.rs`'s `force_ctor_call`, on by default since inlining the body at every
//! site cost more in IR than it saved). So `{ v, w }` and
//! `class Node { constructor(v, w) { this.v = v; this.w = w } }` compile to the
//! same thing: a bump allocation whose header is a compile-time constant,
//! followed by a call into a symbol where `this` is an **opaque parameter**.
//!
//! Being opaque is the whole cost. Every `this.f = p` inside that symbol emits
//! the full class-field precheck (`expr/class_field_inline_guard.rs`): a
//! volatile load of the policy latch, seven header loads, nine compares, and a
//! two-block diamond — per field, per object. Measured on `churn_alloc`'s
//! 20 M-allocation loop that is ~45% of the program
//! (`gc-handoff/ALLOC-NOTES.md` §6), and the corpus's own size-vs-writes
//! controls agree: `churn_alloc` (2 fields) 0.2405 s → `churn_alloc4`
//! (4 fields, **identical object bytes**) 0.3707 s → `churn_alloc8` 0.6143 s.
//! The step is field *stores*, not bytes.
//!
//! The allocation establishes the structural conditions, and the pre-constructor
//! layout declaration establishes the representation condition. This module
//! recognizes that case and stores the fields straight into their packed slots
//! at the `new` site, with no call and no per-field guard.
//!
//! ## Where the proof comes from
//!
//! Almost all of it comes from allocation with the class's canonical stamped
//! shape. The inline bump writes these facts directly; the stamped outlined
//! allocator establishes the same contract:
//!
//! | precheck condition | why it holds |
//! |---|---|
//! | `GcHeader.obj_type == GC_TYPE_OBJECT` | low byte of the packed `gc_packed` constant |
//! | not forwarded | `gc_flags` is exactly `GC_FLAG_ARENA` |
//! | receiver is an ordinary object | the emitted precheck reads `class_id` @0 and the ShapeId @4; #8113 deleted the `object_type` word this row used to name |
//! | `class_id == <this class>` | same word, `cid` is this site's class |
//! | live-slot bound > slot | the bound is the class's own field count (the ShapeId descriptor's `live_inline_slot_count` since #8113), and every slot in the plan indexes a declared field |
//! | `keys_array == @perry_class_keys_<C>` | the header store loads the same global the precheck compares against |
//! | no property descriptors | the instance is freshly allocated and unpublished |
//! | not frozen | same fresh-instance proof |
//! | typed layout INTACT | baked by #7834 for pointer-free shapes; checked after the runtime declaration otherwise |
//!
//! Nothing can invalidate any of it in between: the instance has not escaped,
//! and the arguments were lowered *before* the allocation (they are re-read from
//! their roots by `refresh_rooted_args`, so a collection during the allocation
//! is already handled).
//!
//! Three conditions are **not** static, and all are still emitted — once for the
//! whole construction rather than once per field:
//!
//! * **The policy latch** `PERRY_CLASS_FIELD_INLINE_GUARD_DISABLED`. It is
//!   sticky 0 → 1 and flips when a prototype-level accessor/descriptor install
//!   or typed-feedback tracing arms. Honouring it keeps the escape hatch real —
//!   a knob whose off-state is not on the path it claims to gate is the failure
//!   mode `CLAUDE.md`'s kill-policy section is about.
//! * **The typed-layout result** for pointer-bearing shapes. The declaration
//!   can reject an ambiguous layout, so the fast arm reads the authoritative
//!   INTACT header bit after the call. Pointer-free shapes retain #7834's
//!   compile-time bake and pay no extra check.
//! * **The values.** A raw slot may hold only a plain finite double. The
//!   conjunction of the per-value finite tests decides the whole construction,
//!   so a single non-number sends *all* fields to the constructor call, which is
//!   the unchanged path. That is why this is a diamond and not a per-field
//!   side exit: no field is stored before the decision is made.
//!
//! Because the finite test rejects every NaN-box tag (they all share the
//! all-ones exponent, including INT32-boxed integers), the bits stored here are
//! bit-identical to what the boxed path would have written — a JS number's NaN
//! box *is* its double bits. So this is correct for a slot the reader treats as
//! raw f64 and for one it treats as a NaN-boxed `JsValue` alike, and it needs no
//! `js_array_numeric_value_to_raw_f64` canonicalization (the only inputs that
//! helper rewrites are exactly the ones the finite test rejects).
//!
//! Pointer-bearing typed layouts are also eligible once
//! `js_gc_declare_typed_shape_layout` has established the descriptor. The fast
//! arm checks `GC_OBJ_TYPED_LAYOUT_INTACT` after that call, so an ambiguous or
//! rejected layout stays on the constructor path. Only raw-f64 fields need the
//! finite-value check; pointer/boxed slots keep their NaN-boxed bits.
//!
//! GC: the instance is fresh and unpublished. Pointer stores are already
//! covered by the declared shape mask, so no per-slot layout note is due; each
//! pointer/boxed field still emits the ordinary value-and-generation-tested
//! write barrier for old-generation and incremental-marking correctness.

use crate::expr::FnCtx;

/// One `this.<field> = <arg>` store the plan will emit directly.
pub(super) struct PrologueStore {
    /// Packed inline-slot index, from `class_field_global_index`.
    pub(super) slot: u32,
    /// Index into the `new` site's lowered argument vector.
    pub(super) arg_index: usize,
    /// For a numeric field initialized as `<number param> + <finite literal>`,
    /// the literal to add at the construction site. `None` is a direct
    /// parameter store.
    pub(super) numeric_addend: Option<f64>,
    /// The slot is read as a raw `f64`, so its argument must be a plain finite
    /// number before the direct store is allowed.
    pub(super) requires_raw_f64: bool,
}

/// Can `new <class_name>(…)` skip its constructor call and store the fields
/// inline? `Some(plan)` = yes, and `plan` is every store to emit, in source
/// order; `None` = emit the unchanged call.
///
/// Deliberately narrow. Everything it refuses is refused because the standalone
/// constructor symbol would have done something this path does not reproduce:
///
/// * **Heritage of any kind** — `super(…)` runs another constructor body.
/// * **Decorators, computed members, private/initialized/computed-key fields** —
///   the field-init phase then contains user expressions.
/// * **Any accessor** — `this.f = v` would have to dispatch to a setter. (The
///   per-field `class_field_global_index` lookup rejects an accessor anywhere in
///   the chain as well; both checks are kept so the refusal does not depend on
///   which one happens to run first.)
/// * **Non-plain constructor parameters** (default / rest / decorated /
///   `arguments`) — the call site's argument packing is then not positional.
/// * **Fewer arguments than parameters, or extra ones** — a capture-carrying
///   constructor appends `__perry_cap_*` arguments, and a short call leaves
///   parameters `undefined`. Requiring exact positional correspondence is what
///   makes `lowered_args[i]` the value of parameter `i`.
/// * **A body that is anything other than the full run** — every statement must
///   be `this.<f> = <param>` or the finite typed-number offset documented below,
///   and the assigned set must cover *every* declared field exactly once.
///   Partial coverage would leave a declared raw-f64 slot holding the
///   allocator's `undefined` fill under an INTACT header, which is precisely the
///   state the at-allocation declaration exists to prevent.
///
/// A general literal or pure operator tree on the right-hand side is admissible
/// to `field_init::ctor_prologue_param_assigned_fields` but NOT here: those
/// values have no corresponding entry in the call site's argument vector. The
/// one derived form admitted is `<number param> + <finite numeric literal>`;
/// it lowers to one `fadd`, exactly as the typed constructor body does, and is
/// the wide-record initialization pattern in issue #8289's `tree_wide` case.
pub(super) fn prologue_store_plan(
    ctx: &FnCtx<'_>,
    class_name: &str,
    class: &perry_hir::Class,
    lowered_arg_count: usize,
    constructor_layout_ready: bool,
) -> Option<Vec<PrologueStore>> {
    if !constructor_layout_ready {
        return None;
    }
    if class.extends.is_some()
        || class.extends_name.is_some()
        || class.native_extends.is_some()
        || class.extends_expr.is_some()
        || class.heritage_lexically_shadowed
        || !class.decorators.is_empty()
        || !class.getters.is_empty()
        || !class.setters.is_empty()
        || !class.computed_members.is_empty()
        || class.alloc_width_hint != 0
    {
        return None;
    }
    if !class.fields.iter().all(|f| {
        f.init.is_none() && f.key_expr.is_none() && f.decorators.is_empty() && !f.is_private
    }) {
        return None;
    }
    let ctor = class.constructor.as_ref()?;
    if !ctor.params.iter().all(|p| {
        p.default.is_none() && !p.is_rest && p.decorators.is_empty() && p.arguments_object.is_none()
    }) {
        return None;
    }
    if lowered_arg_count != ctor.params.len() || ctor.params.is_empty() {
        return None;
    }
    if ctor.body.len() != class.fields.len() || class.fields.is_empty() {
        return None;
    }
    let param_slot: std::collections::HashMap<u32, usize> = ctor
        .params
        .iter()
        .enumerate()
        .map(|(i, p)| (p.id, i))
        .collect();
    if param_slot.len() != ctor.params.len() {
        return None;
    }

    let mut plan: Vec<PrologueStore> = Vec::with_capacity(ctor.body.len());
    let mut assigned: std::collections::HashSet<&str> = std::collections::HashSet::new();
    for stmt in &ctor.body {
        let (property, param_id, numeric_addend) = param_store(stmt)?;
        if !assigned.insert(property) {
            return None;
        }
        let arg_index = *param_slot.get(&param_id)?;
        if numeric_addend.is_some()
            && !matches!(ctor.params[arg_index].ty, perry_hir::types::Type::Number)
        {
            return None;
        }
        // A compiled setter owns the name — never store into the slot behind it.
        if ctx
            .methods
            .contains_key(&(class.name.clone(), format!("__set_{}", property)))
        {
            return None;
        }
        let slot = crate::type_analysis::class_field_global_index(ctx, class_name, property)?;
        // Refuse an unresolved field type rather than guessing boxed: missing
        // a raw-f64 slot here would store NaN-box bits into a slot readers
        // reinterpret as a plain double.
        let field_type =
            crate::type_analysis::class_field_declared_type(ctx, class_name, property)?;
        let requires_raw_f64 = crate::typed_shape::type_is_raw_f64_candidate(&field_type);
        plan.push(PrologueStore {
            slot,
            arg_index,
            numeric_addend,
            requires_raw_f64,
        });
    }
    // Every declared field written, exactly once — see the doc comment.
    if !class
        .fields
        .iter()
        .all(|f| assigned.contains(f.name.as_str()))
    {
        return None;
    }
    Some(plan)
}

/// `Some((field, param_id, addend))` when `stmt` is exactly
/// `this.<field> = <param>` or `this.<field> = <param> + <finite number>`.
///
/// Both HIR spellings, for the same reason `field_init::prologue_assigned_field`
/// matches both: `Expr::PropertySet` is what the anon-shape lowering
/// synthesizes, `Expr::PutValueSet` is what user source produces. Matching one
/// of them covers exactly half the population — #7512 is the bug that shape
/// caused.
fn param_store(stmt: &perry_hir::Stmt) -> Option<(&str, u32, Option<f64>)> {
    use perry_hir::{BinaryOp, Expr, Stmt};
    fn value_spec(expr: &Expr) -> Option<(u32, Option<f64>)> {
        match expr {
            Expr::LocalGet(id) => Some((*id, None)),
            Expr::Binary {
                op: BinaryOp::Add,
                left,
                right,
            } => {
                let Expr::LocalGet(id) = left.as_ref() else {
                    return None;
                };
                let addend = match right.as_ref() {
                    Expr::Integer(value) => *value as f64,
                    Expr::Number(value) => *value,
                    _ => return None,
                };
                addend.is_finite().then_some((*id, Some(addend)))
            }
            _ => None,
        }
    }
    match stmt {
        Stmt::Expr(Expr::PropertySet {
            object,
            property,
            value,
        }) if matches!(object.as_ref(), Expr::This) => {
            let (id, addend) = value_spec(value.as_ref())?;
            Some((property.as_str(), id, addend))
        }
        Stmt::Expr(Expr::PutValueSet {
            target,
            key,
            value,
            receiver,
            strict: _,
        }) if matches!(target.as_ref(), Expr::This) && matches!(receiver.as_ref(), Expr::This) => {
            let Expr::String(property) = key.as_ref() else {
                return None;
            };
            let (id, addend) = value_spec(value.as_ref())?;
            Some((property.as_str(), id, addend))
        }
        _ => None,
    }
}
