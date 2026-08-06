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
use crate::types::{DOUBLE, I32, I64};

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
/// *operand* expressions, not the store opcode: `This` and `LocalGet(<plain
/// param>)` cannot throw, allocate, or observe `this`, so the assignment is
/// reached before any other effect of the constructor. Both nodes' misses
/// route through a `[[Set]]` that honours an inherited setter, so neither adds
/// prototype-chain exposure the other lacks — and the caller separately
/// refuses any field the class itself declares a setter for.
///
/// `PutValueSet` additionally requires a constant string key (a computed key
/// is an arbitrary expression that can run user code) and `receiver` to be
/// `This` as well, since codegen evaluates both.
fn prologue_assigned_field<'a>(
    stmt: &'a Stmt,
    param_ids: &std::collections::HashSet<u32>,
) -> Option<&'a str> {
    let is_plain_param = |e: &Expr| matches!(e, Expr::LocalGet(id) if param_ids.contains(id));
    match stmt {
        // Synthesized (anon-shape ctor, destructuring lowering).
        Stmt::Expr(Expr::PropertySet {
            object,
            property,
            value,
        }) if matches!(object.as_ref(), Expr::This) && is_plain_param(value.as_ref()) => {
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
            && is_plain_param(value.as_ref()) =>
        {
            match key.as_ref() {
                Expr::String(property) => Some(property.as_str()),
                _ => None,
            }
        }
        _ => None,
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
/// [`prologue_assigned_field`] recognizes as `this.<f> = <plain param>`. A
/// `LocalGet` of a plain parameter cannot throw, allocate, or observe `this`,
/// so every field it assigns is written before ANY other effect of the
/// constructor — which is exactly the guarantee that makes the earlier
/// `undefined` write dead.
fn ctor_prologue_param_assigned_fields(
    class: &perry_hir::Class,
) -> std::collections::HashSet<String> {
    let empty = std::collections::HashSet::new();
    if class.extends.is_some()
        || class.extends_name.is_some()
        || class.native_extends.is_some()
        || class.extends_expr.is_some()
        || !class.decorators.is_empty()
    {
        return empty;
    }
    let Some(ctor) = class.constructor.as_ref() else {
        return empty;
    };
    let all_fields_bare = class.fields.iter().all(|f| {
        f.init.is_none() && f.key_expr.is_none() && f.decorators.is_empty() && !f.is_private
    });
    if !all_fields_bare {
        return empty;
    }
    let params_plain = ctor.params.iter().all(|p| {
        p.default.is_none() && !p.is_rest && p.decorators.is_empty() && p.arguments_object.is_none()
    });
    if !params_plain {
        return empty;
    }
    let param_ids: std::collections::HashSet<_> = ctor.params.iter().map(|p| p.id).collect();
    let mut assigned = std::collections::HashSet::new();
    for stmt in &ctor.body {
        match prologue_assigned_field(stmt, &param_ids) {
            Some(property) => {
                assigned.insert(property.to_string());
            }
            None => break,
        }
    }
    if assigned.is_empty() {
        return empty;
    }
    if class
        .setters
        .iter()
        .any(|(name, _)| assigned.contains(name))
    {
        return empty;
    }
    assigned
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
        let prologue_assigned = ctx
            .classes
            .get(&class_name_in_chain)
            .copied()
            .map(ctor_prologue_param_assigned_fields)
            .unwrap_or_default();
        let mut init_pairs: Vec<(String, Expr)> = Vec::new();
        let mut init_pairs_computed: Vec<(Expr, Expr)> = Vec::new();
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
                Some(key) => init_pairs_computed.push((key.clone(), init)),
                None => init_pairs.push((field.name.clone(), init)),
            }
        }
        if init_pairs.is_empty() && init_pairs_computed.is_empty() {
            continue;
        }

        // Temporarily swap class_stack so `this.field` in the init
        // resolves against the correct class.
        ctx.class_stack.push(class_name_in_chain.clone());
        for (prop, init_expr) in init_pairs {
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
                let key_bits = blk.bitcast_double_to_i64(&key_box);
                let key_raw = blk.and(I64, &key_bits, POINTER_MASK_I64);
                let this_bits = blk.bitcast_double_to_i64(&this_val);
                let this_raw = blk.and(I64, &this_bits, POINTER_MASK_I64);
                blk.call_void(
                    "js_object_set_field_by_name",
                    &[(I64, &this_raw), (I64, &key_raw), (DOUBLE, &closure_val)],
                );
                continue;
            }

            // Non-closure (or non-this-capturing closure) initializer:
            // build a PropertySet { this, prop, init_expr } and lower
            // through the existing path.
            let set_expr = Expr::PropertySet {
                object: Box::new(Expr::This),
                property: prop,
                value: Box::new(init_expr),
            };
            let _ = lower_expr(ctx, &set_expr)?;
        }

        // Computed-key fields: `[Parent.Symbol.X] = init` lowers to
        // `this[Parent.Symbol.X] = init`. The key expression is evaluated
        // at construction time per ES spec — `Object.defineProperty(this, k, …)`
        // semantics through the IndexSet path. arrow-with-this-capture is
        // unusual on a computed-key field; if it ever surfaces in real code
        // we extend this branch the same way the string-keyed loop above
        // does.
        for (key_expr, init_expr) in init_pairs_computed {
            let set_expr = Expr::IndexSet {
                object: Box::new(Expr::This),
                index: Box::new(key_expr),
                value: Box::new(init_expr),
            };
            let _ = lower_expr(ctx, &set_expr)?;
        }
        ctx.class_stack.pop();
    }
    Ok(())
}

#[cfg(test)]
mod tests;
