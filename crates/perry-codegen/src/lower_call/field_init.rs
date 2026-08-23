//! Recursive field-initializer application for `new ClassName(...)`.
//!
//! Extracted from `new.rs` (pure move, no behavior change) to keep that
//! file under the 2,000-LOC CI size gate. Holds the `FieldInitMode` enum
//! and `apply_field_initializers_recursive`, which walks a class's
//! inheritance chain and installs each class's field initializers onto
//! `this` per the requested mode.

use anyhow::Result;
use perry_hir::{Expr, Stmt};

use crate::expr::{lower_expr, FnCtx};
use crate::nanbox::{double_literal, POINTER_MASK_I64};
use crate::types::{DOUBLE, I32, I64, PTR};

/// The field name a constructor-prologue statement assigns from a plain
/// parameter, or `None` if the statement is not of that shape.
///
/// **Two HIR shapes mean the same thing here, and matching only one of them is
/// what #7512 was** (`new Node(v, w)` measured 63% slower than the equivalent
/// `{v, w}` literal, the reverse of the expected ordering):
///
/// - `Expr::PropertySet` is what the compiler SYNTHESIZES. Every anon-shape
///   object-literal constructor (`lower/context.rs::mint_anon_shape_class`)
///   and the destructuring lowering emit it directly.
/// - `Expr::PutValueSet` is what USER SOURCE lowers to. `lower_expr`'s
///   assignment arm (`perry-hir/src/lower/lower_expr/assignment.rs`) turns
///   *every* source-level `obj.prop = value` — `this.v = v` in a hand-written
///   constructor included — into the spec `PutValue` node. Nothing a user can
///   type produces `Expr::PropertySet`.
///
/// So #7469's elision, which only ever matched `PropertySet`, fired on the
/// synthesized literal ctor and was structurally unreachable for the declared
/// class it was documented as covering. The class paid two extra full
/// class-field-set IC diamonds per construction — a guard call plus a
/// by-name `js_class_field_set_fallback` each, since a fresh instance has no
/// typed-shape descriptor yet and the raw-f64 guard therefore cannot pass —
/// writing a compile-time-constant `undefined` that the next two statements
/// overwrite.
///
/// The proof obligation is identical for both shapes and is entirely about the
/// *operand* expressions, not the store opcode: neither `This` nor any RHS
/// [`prologue_rhs_cannot_observe_this`] admits can observe `this`, so the
/// assignment is reached before anything that could read the field.
///
/// **What the elided write is NOT** (the obvious objection, and it is
/// measurably wrong — `test-files/test_class_field_init_proto_setter.ts`): it
/// is not an observable `[[Set]]`, so eliding it cannot change how many times
/// an accessor runs. A class field declaration is a `CreateDataProperty` — a
/// DEFINE — per `ClassFieldDefinitionRecord` evaluation, so it never consults
/// an inherited accessor, and it installs an OWN data property that the
/// prologue's assignment then writes directly rather than dispatching past.
/// A setter installed on the prototype *after* compilation (which the
/// `class.setters` check below cannot see, by construction) runs **zero**
/// times either way, matching Node exactly. The reading in which the field
/// init is a `[[Set]]` and the setter therefore fires twice is the legacy
/// `useDefineForClassFields: false` behaviour, which neither this compiler nor
/// Node implements. The `class.setters` refusal below exists for the separate
/// case of a setter the class DECLARES, where Perry's own class-field-set
/// lowering does dispatch to the synthesized `__set_<name>` method.
///
/// `PutValueSet` additionally requires a constant string key (a computed key
/// is an arbitrary expression that can run user code) and `receiver` to be
/// `This` as well, since codegen evaluates both.
fn prologue_assigned_field<'a>(
    stmt: &'a Stmt,
    param_ids: &std::collections::HashSet<u32>,
) -> Option<&'a str> {
    let admissible = |e: &Expr| prologue_rhs_cannot_observe_this(e, param_ids);
    match stmt {
        // Synthesized (anon-shape ctor, destructuring lowering).
        Stmt::Expr(Expr::PropertySet {
            object,
            property,
            value,
        }) if matches!(object.as_ref(), Expr::This) && admissible(value.as_ref()) => {
            Some(property.as_str())
        }
        // User-written `this.f = p;`.
        Stmt::Expr(Expr::PutValueSet {
            target,
            key,
            value,
            receiver,
            strict: _,
        }) if matches!(target.as_ref(), Expr::This)
            && matches!(receiver.as_ref(), Expr::This)
            && admissible(value.as_ref()) =>
        {
            match key.as_ref() {
                Expr::String(property) => Some(property.as_str()),
                _ => None,
            }
        }
        _ => None,
    }
}

/// The RHS forms a prologue statement may carry.
///
/// The whole prologue guarantee is "this statement cannot throw, allocate, or
/// **observe `this`**" — see [`ctor_prologue_param_assigned_fields`]. A plain
/// parameter read was the original (and only) admitted form; #7469's widening
/// adds the two other expression families that satisfy it by construction:
///
/// * **Literals** (`null`, `undefined`, a number, a string, a bool). `this.next
///   = null` is the single most common opening statement of a linked-structure
///   constructor, and refusing it truncated the prologue at statement 0 —
///   which, since #7510 consults the same set, also denied the class an
///   at-allocation layout declaration and left every store in its constructor
///   on the by-name fallback.
/// * **Pure operator trees over those two** (`s + 1`, `-n`, `a * b + 1`). Every
///   leaf is a parameter read or a literal, and `Binary`/`Unary`/`Compare`/
///   `Logical` evaluate their operands and combine them — no member access, no
///   call, no `new`, no closure, and (decisively) no `This` anywhere in the
///   tree. `s + 1` can still *allocate* when `s` is a string, and that is fine:
///   the guarantee this predicate underwrites is about observability of `this`,
///   not about the absence of a collection. A GC that scans the
///   still-constructing instance reads the allocator's `undefined` fill through
///   the declared descriptor and rejects it at the tag check.
///
/// Deliberately NOT admitted: `PropertyGet` (a getter runs user code),
/// `Call`/`New` (arbitrary user code), `Closure` (captures), `Await`/`Yield`
/// (suspension), and anything containing `This`. None of those can reach the
/// half-built instance today — it has not escaped — but each makes the
/// "cannot observe `this`" claim rest on a reachability argument instead of on
/// the expression's own shape, and this predicate is consumed by two callers
/// with different failure modes (a dead-store elision and a GC layout
/// declaration).
fn prologue_rhs_cannot_observe_this(
    expr: &Expr,
    param_ids: &std::collections::HashSet<u32>,
) -> bool {
    match expr {
        Expr::LocalGet(id) => param_ids.contains(id),
        Expr::Undefined
        | Expr::Null
        | Expr::Bool(_)
        | Expr::Number(_)
        | Expr::Integer(_)
        | Expr::String(_) => true,
        Expr::Binary { left, right, .. }
        | Expr::Compare { left, right, .. }
        | Expr::Logical { left, right, .. } => {
            prologue_rhs_cannot_observe_this(left, param_ids)
                && prologue_rhs_cannot_observe_this(right, param_ids)
        }
        Expr::Unary { operand, .. } => prologue_rhs_cannot_observe_this(operand, param_ids),
        _ => false,
    }
}

/// Field names whose default-`undefined` initializer write is provably dead
/// because the class's own constructor unconditionally overwrites them before
/// anything can observe `this` (#7469; extended to user-written constructors
/// by #7512).
///
/// A field declared without an initializer must normally be written as
/// `undefined` in the init phase (#486: `new C().x === undefined` is spec, not
/// zero-bytes-from-the-allocator). But the most common constructor shape —
/// every synthesized anon-shape literal ctor
/// (`lower/context.rs::mint_anon_shape_class`), and the hand-written
/// `constructor(v, w) { this.v = v; this.w = w }` — opens with a run of plain
/// `this.f = <param>` statements. For those fields the `undefined` write is a
/// dead store: it is overwritten before any code that could read `this.f`
/// runs. On `churn.ts` that dead store was 2 of the 4 guarded field-store
/// sequences per object literal — a set-guard FFI call, a string addref, and a
/// layout note per field, 20M times, all storing a compile-time constant that
/// the very next statements overwrite. (On the `js_object_alloc_class_inline_keys`
/// allocation path the slots are ALREADY `undefined`-prefilled (#4717), making
/// the write doubly dead — but this elision does not rely on that: the
/// prologue overwrite guarantee is allocator-independent.)
///
/// Returns the empty set — i.e. elides nothing — unless every condition holds:
///
/// - **The class extends nothing** (`extends`/`extends_name`/`native_extends`/
///   `extends_expr` all `None`). A base class is still fine to *be* extended:
///   its field-init phase and ctor body may then be separated by a derived
///   ctor's pre-`super()` statements, but those cannot touch `this` (TDZ), so
///   the skipped slot stays unobservable. What this condition excludes is the
///   class having its OWN super machinery in between.
/// - **No field anywhere on the class carries an initializer expression or a
///   computed key, and the class and its fields are undecorated.** Initializer
///   and key expressions run during the init phase and may legally read
///   `this.<f>` of an earlier field — eliding f's `undefined` write would let
///   them observe whatever the allocator left in the slot. All-`init: None`
///   fields mean the init phase contains no user expression at all.
/// - **Every constructor parameter is plain**: no default (a default expression
///   evaluates before the prologue and, in the general lowering, could observe
///   `this`), no rest, no decorators, no `arguments` materialization.
/// - **No setter shares a name with a prologue-assigned field** — the store
///   would dispatch to the setter instead of writing the slot, and the elided
///   `undefined` write was the only slot write.
/// - The field itself is public and non-computed (`is_private` false,
///   `key_expr` none).
///
/// The prologue is the maximal leading run of statements that
/// [`prologue_assigned_field`] recognizes as `this.<f> = <expr that cannot
/// observe `this`>` — a plain parameter read, a literal, or a pure operator
/// tree over those (see [`prologue_rhs_cannot_observe_this`] for why those
/// three and nothing else). None of them can reach the half-built instance, so
/// every field they assign is written before anything can read it — which is
/// exactly the guarantee that makes the earlier `undefined` write dead.
///
/// Admitting literals is not a cosmetic widening. `constructor(v) { this.next
/// = null; this.v = v; }` is the canonical linked-structure constructor, and
/// under the param-only rule its prologue truncated at statement 0 and came
/// back EMPTY — so neither field's dead `undefined` write was elided and, since
/// #7510 consults the same set, the class was also refused an at-allocation
/// layout declaration.
pub(crate) fn ctor_prologue_param_assigned_fields(
    class: &perry_hir::Class,
) -> std::collections::HashSet<String> {
    ctor_prologue_assigned_fields_inner(class, false).unwrap_or_default()
}

/// Does `expr`'s subtree contain `this`?
///
/// Recurses through [`perry_hir::walker::walk_expr_children`], which is
/// exhaustive over `Expr` and drift-checked against its `_mut` twin by
/// `walker_arms_match` — so a new expression variant cannot silently smuggle a
/// `This` past this scan. That completeness is the whole reason the trailing
/// region is screened at the EXPRESSION level; there is no equivalent shared
/// `Stmt` walker in the HIR, and hand-rolling one would make a missed variant a
/// silent wrong answer.
fn expr_mentions_this(expr: &Expr) -> bool {
    if matches!(expr, Expr::This) {
        return true;
    }
    let mut found = false;
    perry_hir::walker::walk_expr_children(expr, &mut |child: &Expr| {
        if !found && expr_mentions_this(child) {
            found = true;
        }
    });
    found
}

/// May `stmt` follow the prologue run without endangering a raw-f64 slot that
/// a LATER constructor on the chain has yet to write?
///
/// A whitelist, deliberately: only `Stmt::Expr(e)` with no `this` anywhere in
/// `e`. Everything else — `Return` (would skip a later assignment), `If` /
/// loops / `Try` (would make the assignments conditional), `Let`, `Throw` — is
/// refused by not being matched, so adding a `Stmt` variant cannot widen this
/// by accident. `Shape.made = Shape.made + 1` is the motivating admission: a
/// static-field bump between `this.tag = tag` and the subclass's `this.w = w`,
/// which cannot reach the half-built instance because `this` never appears in
/// it.
fn stmt_is_this_free_expr(stmt: &Stmt) -> bool {
    match stmt {
        Stmt::Expr(e) => !expr_mentions_this(e),
        _ => false,
    }
}

/// Is `stmt` a `super(...)` whose arguments cannot observe `this`?
fn stmt_is_safe_super_call(stmt: &Stmt, param_ids: &std::collections::HashSet<u32>) -> bool {
    match stmt {
        Stmt::Expr(Expr::SuperCall(args)) => args
            .iter()
            .all(|a| prologue_rhs_cannot_observe_this(a, param_ids)),
        _ => false,
    }
}

/// `None` = the class is DISQUALIFIED (its constructor's effect on `this`
/// cannot be bounded). `Some(set)` = qualified, and `set` is the field names
/// its prologue unconditionally assigns — possibly EMPTY, which is a real and
/// useful answer for a fieldless subclass like `Marker` whose whole body is
/// `super(x, y)`. The two are different facts and the pre-#7512-followup code
/// conflated them into one empty set, which is why a chain could not be
/// analysed a class at a time.
fn ctor_prologue_assigned_fields_inner(
    class: &perry_hir::Class,
    allow_heritage: bool,
) -> Option<std::collections::HashSet<String>> {
    // A dynamic, native, or lexically-shadowed parent is out of scope in both
    // arms: `super()` then runs a built-in or a runtime-resolved constructor
    // whose effect on `this` this analysis cannot see.
    if class.native_extends.is_some()
        || class.extends_expr.is_some()
        || class.heritage_lexically_shadowed
        || !class.decorators.is_empty()
    {
        return None;
    }
    let has_heritage = class.extends.is_some() || class.extends_name.is_some();
    if has_heritage && !allow_heritage {
        return None;
    }
    // Checked BEFORE the missing-constructor early return: a field initializer
    // runs during the init phase whether or not the class declares a ctor, and
    // it may legally read `this.<f>` of an earlier field.
    let all_fields_bare = class.fields.iter().all(|f| {
        f.init.is_none() && f.key_expr.is_none() && f.decorators.is_empty() && !f.is_private
    });
    if !all_fields_bare {
        return None;
    }
    let Some(ctor) = class.constructor.as_ref() else {
        // No constructor of its own: it assigns nothing, but it also cannot
        // observe anything. A chain containing it is still analysable — its
        // raw-f64 fields simply go uncovered, which the caller's coverage test
        // then rejects.
        return Some(std::collections::HashSet::new());
    };
    let params_plain = ctor.params.iter().all(|p| {
        p.default.is_none() && !p.is_rest && p.decorators.is_empty() && p.arguments_object.is_none()
    });
    if !params_plain {
        return None;
    }
    let param_ids: std::collections::HashSet<_> = ctor.params.iter().map(|p| p.id).collect();
    let mut assigned = std::collections::HashSet::new();
    let mut body = ctor.body.as_slice();
    if allow_heritage {
        // A leading `super(...)` is not a prologue assignment, so the maximal
        // leading run used to truncate at statement 0. Skip it — the argument
        // check is what keeps the parent from being handed the half-built
        // instance.
        if let Some(first) = body.first() {
            if stmt_is_safe_super_call(first, &param_ids) {
                body = &body[1..];
            } else if has_heritage {
                // A derived constructor that opens with anything else may run
                // arbitrary code before `super()`.
                return None;
            }
        } else if has_heritage {
            return None;
        }
    }
    let mut rest = body;
    while let Some(stmt) = rest.first() {
        match prologue_assigned_field(stmt, &param_ids) {
            Some(property) => {
                assigned.insert(property.to_string());
                rest = &rest[1..];
            }
            None => break,
        }
    }
    if allow_heritage {
        // Everything after the run runs BEFORE a subclass's own field writes,
        // so it must not be able to read a raw-f64 slot that is still holding
        // the allocator's `undefined` fill.
        if !rest.iter().all(stmt_is_this_free_expr) {
            return None;
        }
    }
    if class
        .setters
        .iter()
        .any(|(name, _)| assigned.contains(name))
    {
        return None;
    }
    Some(assigned)
}

/// [`ctor_prologue_param_assigned_fields`] for a class that DOES extend a plain
/// user class — the shape the no-heritage rule above refuses outright.
///
/// Refusing it is #7512 repeating one level up. A subclass instance never gets
/// an at-allocation typed-shape declaration, so *every* raw-f64 field store in
/// *every* constructor on its chain — including the base class's own
/// `this.x = x`, which is textually in a heritage-free class — misses its
/// `GC_OBJ_TYPED_LAYOUT_INTACT` guard and falls back to `js_put_value_set`.
/// Measured on `shapes.ts`: 528 000 by-name field stores, and a two-class
/// probe (`gc-handoff/bench/shapes_baseclass_field.ts`) runs **2.0x** slower
/// than the hand-flattened single class doing identical work.
///
/// The extra obligations heritage brings, and how each is discharged:
///
/// * **A leading `super(...)` is not a prologue assignment**, so the maximal
///   leading run truncated at statement 0 and came back empty. It is skipped
///   here instead — but only when every argument satisfies
///   [`prologue_rhs_cannot_observe_this`], so the parent constructor cannot be
///   handed the half-built instance.
/// * **A non-leaf constructor's trailing statements run BEFORE the leaf's field
///   writes.** `Shape`'s body finishes before `Rect` assigns `this.w`, so a
///   trailing `this.w` read there would see a raw-f64-masked slot that still
///   holds `undefined`'s NaN-box bits and yield `NaN` instead of `undefined`.
///   The heritage arm therefore requires the ctor body to contain **no `This`
///   at all after the prologue run** — a whole-subtree scan, not a top-level
///   one. (The no-heritage arm keeps its existing, laxer rule: a class that is
///   never extended has no later writer, and one that IS extended is caught by
///   this same check when the chain is walked from the leaf.)
/// * **An early `return` would skip a later assignment**, leaving a raw-f64
///   field unwritten, so any `Return` anywhere in the body is rejected.
/// Per-class prologue sets for `leaf`'s whole inheritance chain, root → leaf,
/// or `None` if ANY class on it is disqualified.
pub(crate) fn chain_prologue_assigned_fields(
    classes: &std::collections::HashMap<String, &perry_hir::Class>,
    leaf: &str,
) -> Option<Vec<(String, std::collections::HashSet<String>)>> {
    let mut chain: Vec<&perry_hir::Class> = Vec::new();
    let mut cur = classes.get(leaf).copied();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    while let Some(c) = cur {
        if !seen.insert(c.name.clone()) || chain.len() > 64 {
            return None;
        }
        chain.push(c);
        cur = match c.extends_name.as_deref() {
            // A named parent this module cannot resolve is a class whose
            // constructor we cannot analyse — refuse rather than assume it is
            // inert.
            Some(parent) => match classes.get(parent).copied() {
                Some(pc) => Some(pc),
                None => return None,
            },
            None => None,
        };
    }
    chain.reverse();
    let mut out = Vec::with_capacity(chain.len());
    for c in chain {
        let assigned = ctor_prologue_assigned_fields_inner(c, true)?;
        out.push((c.name.clone(), assigned));
    }
    Some(out)
}

/// Walk the inheritance chain from the root down and apply each class's
/// field initializers to `this`. Call this inside `lower_new` after the
/// `this` slot is pushed but before the constructor body is inlined.
///
/// Initializers run in declaration order: root parent first, then each
/// child, matching JavaScript / TypeScript class semantics where fields
/// are initialized before user-written constructor code executes (field
/// initializers are conceptually prepended to the constructor body).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum FieldInitMode {
    /// Apply field initializers for the entire chain root → leaf.
    All,
    /// Apply only the ancestors' field initializers (skip the leaf class).
    /// Used to set up parent fields before a parent ctor body runs.
    AncestorsOnly,
    /// Apply only the named class's own field initializers (skip ancestors).
    /// Used after a parent ctor body has run to install the leaf's fields,
    /// which may reference state set by the parent body (e.g.
    /// `enumValues = this.config.enumValues` in drizzle's PgText). Refs #420.
    SelfOnly,
    /// Issue #631-followup: apply fields for the chain root → `stop_at`
    /// (inclusive). Used in the no-own-ctor path BEFORE the inherited-
    /// ctor body runs, so only the inherited-ctor class's chain has its
    /// fields set up. Intermediate classes between `stop_at` and the leaf
    /// (e.g. SQLiteBaseInteger between SQLiteColumn and SQLiteInteger)
    /// have their fields applied AFTER the inherited-ctor body, via
    /// `BetweenExclusiveTo`.
    UpToInclusive(String),
    /// Apply fields for chain (`stop_at` exclusive) → leaf (inclusive).
    /// Mirror of `UpToInclusive` for the post-body chain. Skips
    /// `stop_at` itself because that class's SelfOnly fields are
    /// applied via the SuperCall site inside the inlined body.
    BetweenExclusiveTo(String),
    /// Apply every class after the root ancestor through the leaf. Used
    /// when a default-derived constructor chain has no explicit inherited
    /// constructor body, so there is no SuperCall site to apply intermediate
    /// class fields.
    AfterRoot,
}

pub(crate) fn apply_field_initializers_recursive(
    ctx: &mut FnCtx<'_>,
    class_name: &str,
    mode: FieldInitMode,
) -> Result<()> {
    // Issue #26 / #321: prefer the authoritative, source-prefix-disambiguated
    // ancestor chain (built once in `compile_module` alongside the per-class
    // keys global). Walking `ctx.classes` by `extends_name` mis-resolves
    // same-named cross-module parents (effect's `Type` in SchemaAST.ts vs
    // ParseResult.ts) and writes that wrong parent's fields onto the instance
    // as `undefined`, surfacing as spurious enumerable keys (`_tag,ast,actual,
    // message` on a `PropertySignature`). The authoritative chain is root →
    // leaf and carries each ancestor's resolved fields, so we use both its
    // ORDER (for the mode filter) and its FIELDS (per class below).
    // #7512-followup: computed once for the LEAF, then consulted per class in
    // the chain below. `Some` only when the chain form is what authorizes the
    // at-allocation declaration, so a chain that stays on the old path keeps
    // exactly its old elision set.
    let chain_prologue_assigned: Option<Vec<(String, std::collections::HashSet<String>)>> =
        chain_prologue_assigned_fields(ctx.classes, class_name).filter(|chain| {
            crate::typed_shape::class_chain_layout_declarable_at_allocation(ctx.classes, chain)
        });
    let mut chain_field_override: std::collections::HashMap<String, Vec<perry_hir::ClassField>> =
        std::collections::HashMap::new();
    // Collect the inheritance chain from root down.
    let mut chain: Vec<String> = Vec::new();
    if let Some(auth) = ctx.class_init_chains.get(class_name) {
        for (name, fields) in auth {
            chain.push(name.clone());
            chain_field_override.insert(name.clone(), fields.clone());
        }
    } else {
        let mut cur = Some(class_name.to_string());
        while let Some(c) = cur {
            let Some(class) = ctx.classes.get(&c).copied() else {
                break;
            };
            chain.push(c.clone());
            cur = class.extends_name.clone();
        }
        chain.reverse();
    }

    // Apply mode filter:
    //   All: keep entire chain
    //   AncestorsOnly: drop the leaf (last entry)
    //   SelfOnly: keep only the leaf
    //   UpToInclusive(stop_at): keep chain[0..=index_of(stop_at)]
    //   BetweenExclusiveTo(stop_at): keep chain[index_of(stop_at)+1..]
    //   AfterRoot: keep chain[1..]
    let chain: Vec<String> = match &mode {
        FieldInitMode::All => chain,
        FieldInitMode::AncestorsOnly => {
            // Issue #631-followup: keep only the ROOT class's fields.
            // Per ECMAScript spec, derived-class field initializers run
            // AFTER super() returns (so they may depend on parent body
            // state, e.g. drizzle's `class SQLiteBaseInteger extends
            // SQLiteColumn { autoIncrement = this.config.autoIncrement }`
            // — `this.config` is set by Column's body two levels up).
            // Pre-#631 this kept all-ancestors-but-leaf which incorrectly
            // ran SQLiteBaseInteger's init before Column's body.
            //
            // Each intermediate class's fields are applied via the
            // SuperCall site (`expr.rs::Expr::SuperCall`'s post-body
            // intermediate-walk added in this commit). Root's fields
            // need to be applied here because root has no super() and
            // its body may reference its own fields directly.
            if chain.len() <= 1 {
                Vec::new()
            } else {
                vec![chain[0].clone()]
            }
        }
        FieldInitMode::SelfOnly => {
            if let Some(last) = chain.last().cloned() {
                vec![last]
            } else {
                Vec::new()
            }
        }
        FieldInitMode::UpToInclusive(stop_at) => {
            if let Some(idx) = chain.iter().position(|n| n == stop_at) {
                chain[..=idx].to_vec()
            } else {
                Vec::new()
            }
        }
        FieldInitMode::BetweenExclusiveTo(stop_at) => {
            if let Some(idx) = chain.iter().position(|n| n == stop_at) {
                if idx + 1 < chain.len() {
                    chain[idx + 1..].to_vec()
                } else {
                    Vec::new()
                }
            } else {
                Vec::new()
            }
        }
        FieldInitMode::AfterRoot => {
            if chain.len() > 1 {
                chain[1..].to_vec()
            } else {
                // The leaf directly extends a NON-user parent (a built-in like
                // `Error`, or an imported class) — such a parent is not in the
                // user-class `chain`, so there is no root ancestor to skip. Its
                // own field initializers must still run after the (built-in)
                // super; without this a no-own-ctor `class A extends Error {
                // v = 42 }` left `v` at the raw-0 slot, and a later
                // `this.arr.includes(x)` on an unset field threw
                // `Cannot read properties of undefined`.
                chain
            }
        }
    };

    for class_name_in_chain in chain {
        // Issue #26: prefer the authoritative chain's resolved fields for this
        // class (correct cross-module parent layout); fall back to the
        // name-keyed `ctx.classes` only when no authoritative entry exists.
        // Local classes carry their real init exprs here; imported/inherited
        // fields carry `init: None` (→ `undefined`), exactly as before — just
        // resolved against the RIGHT parent.
        let class_fields: Vec<perry_hir::ClassField> =
            if let Some(fields) = chain_field_override.get(&class_name_in_chain) {
                fields.clone()
            } else {
                match ctx.classes.get(&class_name_in_chain).copied() {
                    Some(c) => c.fields.clone(),
                    None => continue,
                }
            };
        // Collect (property_name, init_expr) pairs up-front to avoid
        // holding an immutable borrow of ctx.classes across lower_expr.
        // Computed-key fields (`[Symbol.for("k")]` etc.) live in a parallel
        // list since their key is an expression that needs runtime evaluation.
        //
        // Fields declared without an initializer (`#x;` / `x: any;`) must
        // still be written in the constructor as `undefined` — JS semantics
        // is `new C().x === undefined`, not zero-bytes from the allocator.
        // Without the explicit write, regular methods see `undefined` (the
        // field-by-name dispatcher returns undefined for absent fields),
        // but arrow-class-field bodies that load `this.x` through the
        // captured-this slot read raw zero bytes — `0 ?? fallback` then
        // takes the wrong branch (0 is falsy but not nullish), breaking
        // common patterns like `this.#preparedHeaders ?? new Headers()`
        // in hono's Context. Lower the missing-init case to
        // `Expr::Undefined` so the constructor writes the spec-correct
        // value into the field slot. Refs #486.
        // #7469: default-`undefined` writes that the class's own ctor prologue
        // provably overwrites are dead — see the function doc for the proof
        // obligations. Computed from the leaf-authoritative `ctx.classes` entry
        // (an ancestor resolved only through `chain_field_override` has no
        // visible ctor here and gets the conservative empty set).
        // #7512-followup: when the LEAF's whole chain is declarable at
        // allocation, the two consumers of this set must agree — the declared
        // raw-f64 mask is live from birth, so a field-init `undefined` write
        // into one of those slots would fail `layout_raw_f64_bits` and
        // downgrade the descriptor on the spot, making the declaration
        // worthless. So use the chain-aware per-class set exactly when the
        // chain form is what authorized the declaration.
        let prologue_assigned = chain_prologue_assigned
            .as_ref()
            .and_then(|chain| {
                chain
                    .iter()
                    .find(|(name, _)| *name == class_name_in_chain)
                    .map(|(_, set)| set.clone())
            })
            .unwrap_or_else(|| {
                ctx.classes
                    .get(&class_name_in_chain)
                    .copied()
                    .map(ctor_prologue_param_assigned_fields)
                    .unwrap_or_default()
            });
        let mut init_pairs: Vec<(String, Expr, bool)> = Vec::new();
        let mut init_pairs_computed: Vec<(String, Expr)> = Vec::new();
        for field in &class_fields {
            // Wall 46: synthesized capture fields (`__perry_cap_*`) are populated
            // EXCLUSIVELY by the constructor's capture-param assignments — for a
            // class constructed directly, by its own ctor; for a subclass of an
            // (inherited) dynamic parent, by super()'s parent-ctor run. They carry
            // `init: None`, so the default `Expr::Undefined` write below would
            // re-initialize them to `undefined` during the derived field-init
            // phase (which runs AFTER super()), CLOBBERING the real captured value
            // super already stored. That is the Next.js `NextNodeServer extends
            // _baseserver.default` failure: base-server's `_iserror`/`_utils`/
            // `_log` read `undefined` in inherited methods. Field-init must never
            // touch these — skip them so the ctor param assignment is the sole
            // writer (verified: captures are correct at the parent ctor end and
            // only vanish during the derived ctor's post-super field-init).
            if field.key_expr.is_none() && field.name.starts_with("__perry_cap_") {
                continue;
            }
            // #7469: skip the dead default write for prologue-overwritten
            // fields. `ctor_prologue_param_assigned_fields` returns non-empty
            // only when EVERY field on the class is bare (`init: None`, named
            // key, undecorated, public), so this arm can only ever drop
            // `Expr::Undefined` writes — never a real initializer.
            if field.init.is_none()
                && field.key_expr.is_none()
                && prologue_assigned.contains(&field.name)
            {
                continue;
            }
            let init = match &field.init {
                Some(e) => e.clone(),
                None => Expr::Undefined,
            };
            match &field.key_expr {
                Some(_) => init_pairs_computed.push((field.name.clone(), init)),
                None => init_pairs.push((field.name.clone(), init, field.is_private)),
            }
        }
        let (class_has_private_elements, class_has_private_brand) = ctx
            .classes
            .get(&class_name_in_chain)
            .copied()
            .map(|class| {
                (
                    class.has_private_instance_elements(),
                    class.has_private_instance_brand(),
                )
            })
            .unwrap_or((false, false));
        if init_pairs.is_empty() && init_pairs_computed.is_empty() && !class_has_private_elements {
            continue;
        }

        // Temporarily swap class_stack so `this.field` in the init
        // resolves against the correct class.
        ctx.class_stack.push(class_name_in_chain.clone());
        // Private methods/accessors are installed before fields and share a
        // single per-class brand. Private fields are added individually below
        // so their initializer ordering and duplicate check remain observable.
        if class_has_private_brand {
            let this_val = ctx
                .this_stack
                .last()
                .cloned()
                .map(|slot| ctx.block().load(DOUBLE, &slot))
                .unwrap_or_else(|| double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)));
            let class_id = ctx
                .class_ids
                .get(&class_name_in_chain)
                .copied()
                .unwrap_or(0)
                .to_string();
            ctx.block().call(
                DOUBLE,
                "js_private_brand_add",
                &[(DOUBLE, &this_val), (I32, &class_id)],
            );
        }
        for (prop, init_expr, is_private) in init_pairs {
            if is_private {
                let value = lower_expr(ctx, &init_expr)?;
                let this_val = ctx
                    .this_stack
                    .last()
                    .cloned()
                    .map(|slot| ctx.block().load(DOUBLE, &slot))
                    .unwrap_or_else(|| {
                        double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
                    });
                let key_idx = ctx.strings.intern(&prop);
                let key_global = format!("@{}", ctx.strings.entry(key_idx).handle_global);
                let key = ctx.block().load(DOUBLE, &key_global);
                let class_id = ctx
                    .class_ids
                    .get(&class_name_in_chain)
                    .copied()
                    .unwrap_or(0)
                    .to_string();
                ctx.block().call(
                    DOUBLE,
                    "js_private_field_add",
                    &[
                        (DOUBLE, &this_val),
                        (I32, &class_id),
                        (DOUBLE, &key),
                        (DOUBLE, &value),
                    ],
                );
                continue;
            }
            // Issue #263: arrow-function class fields like
            // `arrowField = () => this.value` need their reserved `this`
            // capture slot patched with the constructor's `this` AFTER
            // the closure is built — same pattern `lower_object_literal`
            // already uses for object-literal methods. Without this, the
            // arrow's body reads slot `auto_captures.len()` of the
            // closure's capture array (initialized to 0.0 by the
            // closure-build site at expr.rs:3294-3304), then `this.value`
            // dereferences address 0 and SIGSEGVs.
            if let Expr::Closure {
                params: cparams,
                body: cbody,
                captures: ccaps,
                captures_this: true,
                ..
            } = &init_expr
            {
                let auto_caps =
                    crate::type_analysis::compute_auto_captures(ctx, cparams, cbody, ccaps);
                let this_idx = auto_caps.len() as u32;

                // Lower the closure expression to a NaN-boxed pointer.
                let closure_val = lower_expr(ctx, &init_expr)?;

                // Read the current `this` from the constructor's this_stack.
                let this_val = if let Some(slot) = ctx.this_stack.last().cloned() {
                    ctx.block().load(DOUBLE, &slot)
                } else {
                    double_literal(0.0)
                };

                // Patch the closure's reserved this-slot in-place, then
                // store the closure as the field via the runtime FFI.
                let blk = ctx.block();
                let bits = blk.bitcast_double_to_i64(&closure_val);
                let closure_handle = blk.and(I64, &bits, POINTER_MASK_I64);
                let idx_str = this_idx.to_string();
                let this_bits = blk.bitcast_double_to_i64(&this_val);
                blk.call_void(
                    "js_closure_set_capture_bits",
                    &[(I64, &closure_handle), (I32, &idx_str), (I64, &this_bits)],
                );

                // Now store the patched closure as the field. Emit the
                // property-write call directly, mirroring PropertySet's
                // codegen path (expr.rs:2559+) — we can't go through
                // `lower_expr` again because that would re-lower the
                // closure expression and produce a fresh, unpatched
                // closure pointer.
                let key_idx = ctx.strings.intern(&prop);
                let key_handle_global = format!("@{}", ctx.strings.entry(key_idx).handle_global);
                let blk = ctx.block();
                let key_box = blk.load(DOUBLE, &key_handle_global);
                blk.call(
                    DOUBLE,
                    "js_class_field_add",
                    &[
                        (DOUBLE, &this_val),
                        (DOUBLE, &key_box),
                        (DOUBLE, &closure_val),
                    ],
                );
                continue;
            }

            // DefineField uses CreateDataProperty semantics: an inherited
            // setter must not run, while a Proxy receiver must observe its
            // `defineProperty` trap. `js_class_field_add` provides both, but it
            // is a full [[DefineOwnProperty]] behind a handle scope — per field,
            // per construction. #8648: `shapes.ts` pays it ~2M times (7 classes
            // x ~2-3 fields x 120k constructions) and measured 3.14x.
            //
            // The two semantics coincide when neither difference can arise:
            //
            //   * no accessor anywhere on the chain -- `class_field_global_index`
            //     already answers exactly this, returning `None` the moment an
            //     accessor (or a re-declaration) appears on the chain
            //     (`class_field_inline_guard`, #5654); and
            //   * the receiver is provably the freshly allocated ordinary
            //     instance -- no constructor on the chain hands back a
            //     replacement `this` via `js_ctor_return_override`, which is the
            //     only way a Proxy can become the field-initializer receiver.
            //
            // Both hold for an ordinary class, so lower through the optimized
            // `PropertySet` path (inline shape precheck -> direct slot store)
            // exactly as this did before #8630. Anything else keeps the full
            // DefineField call.
            let chain_can_replace_this = chain_constructor_returns_value(ctx, &class_name_in_chain);
            let no_accessor_on_chain =
                crate::type_analysis::class_field_global_index(ctx, &class_name_in_chain, &prop)
                    .is_some();
            if no_accessor_on_chain && !chain_can_replace_this {
                let set_expr = Expr::PropertySet {
                    object: Box::new(Expr::This),
                    property: prop,
                    value: Box::new(init_expr),
                };
                let _ = lower_expr(ctx, &set_expr)?;
                continue;
            }
            let value = lower_expr(ctx, &init_expr)?;
            let this_val = ctx
                .this_stack
                .last()
                .cloned()
                .map(|slot| ctx.block().load(DOUBLE, &slot))
                .unwrap_or_else(|| double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)));
            let key_idx = ctx.strings.intern(&prop);
            let key_global = format!("@{}", ctx.strings.entry(key_idx).handle_global);
            let key = ctx.block().load(DOUBLE, &key_global);
            ctx.block().call(
                DOUBLE,
                "js_class_field_add",
                &[(DOUBLE, &this_val), (DOUBLE, &key), (DOUBLE, &value)],
            );
        }

        // Computed-key fields reuse the PropertyKey resolved once during
        // ClassDefinitionEvaluation. DefineField uses CreateDataProperty
        // semantics (including Proxy [[DefineOwnProperty]]), not assignment.
        // arrow-with-this-capture is
        // unusual on a computed-key field; if it ever surfaces in real code
        // we extend this branch the same way the string-keyed loop above
        // does.
        for (key_slot, init_expr) in init_pairs_computed {
            let value = lower_expr(ctx, &init_expr)?;
            let this_val = ctx
                .this_stack
                .last()
                .cloned()
                .map(|slot| ctx.block().load(DOUBLE, &slot))
                .unwrap_or_else(|| double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)));
            let class_id = ctx
                .class_ids
                .get(&class_name_in_chain)
                .copied()
                .unwrap_or(0)
                .to_string();
            let key_idx = ctx.strings.intern(&key_slot);
            let entry = ctx.strings.entry(key_idx);
            let key_bytes = format!("@{}", entry.bytes_global);
            let key_len = entry.byte_len.to_string();
            let key = ctx.block().call(
                DOUBLE,
                "js_class_computed_field_key",
                &[
                    (DOUBLE, &this_val),
                    (I32, &class_id),
                    (PTR, &key_bytes),
                    (I64, &key_len),
                ],
            );
            ctx.block().call(
                DOUBLE,
                "js_class_field_add",
                &[(DOUBLE, &this_val), (DOUBLE, &key), (DOUBLE, &value)],
            );
        }
        ctx.class_stack.pop();
    }
    Ok(())
}

#[cfg(test)]
mod tests;

/// #8648: does any constructor on `leaf`'s inheritance chain `return` a value?
///
/// A value-returning constructor can hand back a replacement `this`
/// (`js_ctor_return_override`), and that replacement may be a Proxy — which
/// DefineField must reach through `defineProperty`, not `set`. Conservative on
/// every edge it cannot see: a native base, a dynamic `extends`, or a class
/// missing from `ctx.classes` all answer `true`.
fn chain_constructor_returns_value(ctx: &crate::expr::FnCtx<'_>, leaf: &str) -> bool {
    let mut name = leaf.to_string();
    for _ in 0..32 {
        let Some(class) = ctx.classes.get(&name).copied() else {
            return true;
        };
        if class.native_extends.is_some() || class.extends_expr.is_some() {
            return true;
        }
        if let Some(ctor) = class.methods.iter().find(|m| m.name == "constructor") {
            if crate::collectors::body_returns_value(&ctor.body) {
                return true;
            }
        }
        // `extends` is a class id; `extends_name` is the textual parent. Only
        // the latter can be followed through `ctx.classes`, so an id-only edge
        // is treated as unknown.
        match class.extends_name.as_ref() {
            Some(parent) => name = parent.clone(),
            None if class.extends.is_some() => return true,
            None => return false,
        }
    }
    true
}
