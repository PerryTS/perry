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
//! receiver shape and header checks, compares, and a
//! two-block diamond — per field, per object. Measured on `churn_alloc`'s
//! 20 M-allocation loop that is ~45% of the program
//! (`gc-handoff/ALLOC-NOTES.md` §6), and the corpus's own size-vs-writes
//! controls agree: `churn_alloc` (2 fields) 0.2405 s → `churn_alloc4`
//! (4 fields, **identical object bytes**) 0.3707 s → `churn_alloc8` 0.6143 s.
//! The step is field *stores*, not bytes.
//!
//! The allocation establishes the structural conditions, and the pre-constructor
//! birth ShapeId establishes the representation condition. This module
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
//! | not forwarded | fresh nursery birth: `GC_FLAG_ARENA` plus live runtime birth flags; never forwarded |
//! | receiver is an ordinary object | the emitted precheck reads `class_id` @0 and the ShapeId @4; #8113 deleted the `object_type` word this row used to name |
//! | `class_id == <this class>` | same word, `cid` is this site's class |
//! | live-slot bound > slot | the bound is the class's own field count (the ShapeId descriptor's `live_inline_slot_count` since #8113), and every slot in the plan indexes a declared field |
//! | `keys_array == @perry_class_keys_<C>` | the header store loads the same global the precheck compares against |
//! | no property descriptors | the instance is freshly allocated and unpublished |
//! | not frozen | same fresh-instance proof |
//! | field representation | the birth ShapeId carries each F64 lane |
//!
//! Nothing can invalidate any of it in between: the instance has not escaped,
//! and the arguments were lowered *before* the allocation (they are re-read from
//! their roots by `refresh_rooted_args`, so a collection during the allocation
//! is already handled).
//!
//! The value representation is checked once for the whole construction:
//!
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
//! Pointer-bearing layouts share the same birth ShapeId protocol: the rep
//! is part of the shape identity and cannot be confused with a header bit.
//!
//! GC: the instance is fresh and unpublished. Pointer stores are already
//! covered by ShapeId tracing, so no per-slot layout note is due; each
//! pointer/boxed field on an outlined birth still emits the ordinary barrier.
//! Emitted Eden births carry Fresh(parent): the complete NoCollect prologue
//! finishes before publication or a statepoint. No remembered edge is needed,
//! and a black birth's pending seed traces its completed fields.

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
    /// The stored key, when the slot is a family layout's (its
    /// `PERRY_PROBE_VERIFY` witness).
    pub(super) family_key: Option<String>,
}

impl PrologueStore {
    /// Must the stored value pass the plain-finite-number test before this
    /// store may skip the constructor?
    ///
    /// * A raw-`f64` lane holds only plain finite doubles.
    /// * A `<param> + <literal>` store lowers to one `fadd`, which is JS `+`
    ///   only when the argument is a Number. The parameter's declared TS
    ///   `number` type does not make it one: untyped code can pass a string,
    ///   and `"s" + 1` is `"s1"`, while the `fadd` of a NaN-boxed string
    ///   keeps the string's bits. A Number argument needs no test when the
    ///   whole-program facts prove it (a numeric literal, or a local in
    ///   `facts_number_locals` / `number_by_construction_locals`); any other
    ///   argument is tested. A non-Number argument is a NaN box, `fadd`
    ///   propagates a NaN, so the result fails the finite test and the
    ///   construction takes the ordinary constructor.
    ///
    /// P3 B4b: a Number has one encoding, the plain double, so a value the
    /// facts or its structure prove a Number
    /// (`expr_produces_canonical_raw_f64`) is the word a raw-`f64` lane
    /// holds and needs no test ([`proven_number`](Self::proven_number)).
    /// Check mode tests it instead (`Fact::CanonicalNumber`).
    pub(super) fn requires_number_check(&self, ctx: &FnCtx<'_>, args: &[perry_hir::Expr]) -> bool {
        if self.proven_number(ctx, args) {
            return false;
        }
        self.requires_raw_f64
            || (self.numeric_addend.is_some()
                && !addend_operand_is_proven_number(ctx, args.get(self.arg_index)))
    }

    /// This store writes a raw-`f64` lane with no number test because its
    /// argument is a proven Number (P3 B4b; `PERRY_FACTS_NUMBER_BIRTHS=0`
    /// keeps the test).
    pub(super) fn proven_number(&self, ctx: &FnCtx<'_>, args: &[perry_hir::Expr]) -> bool {
        self.requires_raw_f64
            && number_births()
            && !ctx.wpfacts_off
            && args
                .get(self.arg_index)
                .is_some_and(|arg| crate::type_analysis::expr_produces_canonical_raw_f64(ctx, arg))
    }
}

/// A raw-`f64` lane store took `value` with no number test
/// ([`PrologueStore::proven_number`]): count the claim, and in check mode
/// test that the value is a plain double (a miss exits 86,
/// `Fact::CanonicalNumber`). Called before the allocation, where a call is
/// allowed.
pub(super) fn witness_proven_number(
    ctx: &mut FnCtx<'_>,
    value: &str,
    slot: u32,
) -> anyhow::Result<()> {
    crate::verify_facts::consumed(
        crate::verify_facts::Fact::CanonicalNumber,
        || serde_json::json!({ "birth_slot": slot }),
    );
    crate::facts_consumed::used();
    crate::expr::facts_canonical::witness(
        ctx,
        value,
        crate::expr::facts_canonical::Num::Number,
        "birth lane",
    )
}

/// `PERRY_FACTS_NUMBER_BIRTHS=0` (`off`, `false`): every raw-`f64` lane store
/// of a birth keeps its plain-finite-number test.
pub(crate) fn number_births() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| {
        !matches!(
            std::env::var("PERRY_FACTS_NUMBER_BIRTHS").as_deref(),
            Ok("0") | Ok("off") | Ok("false")
        )
    })
}

/// The facts prove `arg` a Number (a Number's representation is its raw
/// double): a numeric literal, or a local the whole-program facts
/// (`codegen/facts_number_locals.rs`) or its structure
/// (`number_by_construction_locals`) prove holds only Numbers. Never the
/// declared type.
fn addend_operand_is_proven_number(ctx: &FnCtx<'_>, arg: Option<&perry_hir::Expr>) -> bool {
    match arg {
        Some(perry_hir::Expr::Number(_) | perry_hir::Expr::Integer(_)) => true,
        Some(perry_hir::Expr::LocalGet(id)) => {
            ctx.facts_number_locals.contains(id) || ctx.number_by_construction_locals.contains(id)
        }
        _ => false,
    }
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
        || class.has_evaluated_members()
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

    let family = ctx
        .class_ids
        .get(class_name)
        .filter(|&&cid| super::new_alloc::class_born_on_family(ctx, class, cid))
        .and_then(|cid| ctx.program_layouts.families.get(cid));
    let mut plan: Vec<PrologueStore> = Vec::with_capacity(ctor.body.len());
    let mut assigned: std::collections::HashSet<&str> = std::collections::HashSet::new();
    for stmt in &ctor.body {
        let (property, param_id, numeric_addend) = param_store(stmt)?;
        if !assigned.insert(property) {
            return None;
        }
        let arg_index = *param_slot.get(&param_id)?;
        // Admission only: the store's correctness rests on
        // `PrologueStore::requires_number_check`, not on this declared type.
        if numeric_addend.is_some()
            && !matches!(ctor.params[arg_index].ty, perry_hir::types::Type::Number)
        {
            return None;
        }
        // A reservation field is added by this `[[Set]]`, not defined before
        // the body runs: an inherited setter or non-writable key decides it,
        // so the store is a slot store only when no prototype can intercept
        // the add (the census). A defined field is already own, and a
        // literal's keys are definitions.
        if !class.is_literal_shape()
            && class
                .fields
                .iter()
                .any(|f| f.name == property && f.origin.is_reservation())
            && !crate::proto_constants::create_add_free(property)
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
        // A family-born instance (object model step 2c) stores into its
        // family layout's slots, whose lanes are boxed values.
        let (slot, requires_raw_f64) = if let Some(fam) = family {
            (fam.keys.iter().find(|(k, _)| k == property)?.1, false)
        } else {
            let slot = crate::type_analysis::class_field_global_index(ctx, class_name, property)?;
            // Refuse an unresolved field type rather than guessing boxed: missing
            // a raw-f64 slot here would store NaN-box bits into a slot readers
            // reinterpret as a plain double.
            crate::type_analysis::class_field_declared_type(ctx, class_name, property)?;
            // Charter step 5, P4: raw exactly for an `F64` lane of the birth rep.
            let requires_raw_f64 = crate::expr::class_field_inline_guard::class_birth_slot_is_f64(
                ctx, class_name, slot,
            );
            (slot, requires_raw_f64)
        };
        plan.push(PrologueStore {
            slot,
            arg_index,
            numeric_addend,
            requires_raw_f64,
            family_key: family.map(|_| property.to_owned()),
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
            ..
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
            ..
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
