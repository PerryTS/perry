//! Constructor-body analysis helpers for `new ClassName(args…)` lowering.
//!
//! Pure predicate walkers (no codegen side effects) split out of `new.rs`
//! to keep that file under the file-size gate. They classify a constructor
//! body — does it call `super()`, dereference `this`, value-return, etc. —
//! to drive `lower_new`'s static no-super-throw / inline-vs-call decisions,
//! plus `node_stream_parent_kind` and `collect_decl_local_ids`.

use perry_hir::{Class, Expr};

use crate::expr::FnCtx;
use crate::types::{DOUBLE, I32};

/// The native base classes perry models by STAMPING state onto the INSTANCE at
/// construction time instead of giving it a real builtin prototype: the instance
/// is a plain `ObjectHeader`, and `super()` installs the base's surface on it —
/// an own-property method bag for `EventEmitter`, a hidden backing
/// `MapHeader`/`SetHeader` for `Map`/`Set`, the standard event fields for
/// `Event`/`CustomEvent`.
///
/// Because the install rides on `super()`, it used to be keyed on the class
/// naming the base LITERALLY in its own `extends` clause. That misses the base
/// in two directions:
///
///   * a class with no own constructor writes no `super()` at all, so the init
///     never ran — `class M extends Map {}` produced an instance with no
///     collection storage and therefore no `set`/`get`/`size` (#6325); and
///   * an INDIRECT subclass names an intermediate USER class, not the base, so
///     the init never ran either — `class D extends B {}` with `class B extends
///     EventEmitter {}` produced an object with no emitter surface (#6326).
///
/// Both are the same hole, and both close by triggering on "the class CHAIN
/// reaches the base" — see [`native_instance_base_in_chain`].
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum NativeInstanceBase {
    EventEmitter,
    Map,
    Set,
    Event,
    CustomEvent,
}

/// The native base a parent NAME denotes, if any.
///
/// Deliberately narrow. The other builtin bases perry supports have their own
/// construction machinery that the chain walk must not preempt: `Error` &
/// friends are handled by the Error-init arms, the `node:stream` classes by
/// `node_stream_parent_kind` (which already walks the chain), `Request`/
/// `Response` by the fetch-handle shims, `Promise` by the backing-cell shim,
/// and `Array` by `js_array_subclass_init` (whose `super(n)` argument is a
/// length, not an iterable — a no-arg implicit ctor has nothing to forward).
pub(crate) fn native_instance_base(name: &str) -> Option<NativeInstanceBase> {
    match name {
        "EventEmitter" => Some(NativeInstanceBase::EventEmitter),
        "Map" => Some(NativeInstanceBase::Map),
        "Set" => Some(NativeInstanceBase::Set),
        "Event" => Some(NativeInstanceBase::Event),
        "CustomEvent" => Some(NativeInstanceBase::CustomEvent),
        _ => None,
    }
}

/// The native base `class` ultimately derives from, found by walking
/// `extends_name` through user classes that carry no constructor of their own.
///
/// The walk STOPS (yielding `None`) at any ancestor that HAS a constructor —
/// local or imported. Such an ancestor's `super()` performs the base init
/// itself, and running it a second time here would re-stamp the surface over
/// already-live state: a fresh listener bag over an emitter the ancestor's ctor
/// already registered listeners on, a fresh empty backing over a seeded Map.
/// This is the same ctor-less walk `node_stream_parent_kind` performs for the
/// classic `node:stream` bases — generalized, not invented.
///
/// A user class in this module SHADOWS the builtin name: with `class Map {}` in
/// the source, `ctx.classes` resolves `Map` first, so the walk descends into the
/// user class and never reports a native base.
pub(crate) fn native_instance_base_in_chain(
    ctx: &FnCtx<'_>,
    class: &Class,
) -> Option<NativeInstanceBase> {
    let mut cur = class.extends_name.as_deref();
    for _ in 0..32 {
        let name = cur?;
        if ctx.imported_class_ctors.contains_key(name) {
            return None;
        }
        match ctx.classes.get(name).copied() {
            // A user class in this module: keep walking only while it delegates
            // construction upward (no ctor of its own).
            Some(parent) => {
                if parent.constructor.is_some() {
                    return None;
                }
                cur = parent.extends_name.as_deref();
            }
            // Not a class in this module — the chain has reached a builtin.
            None => return native_instance_base(name),
        }
    }
    None
}

/// Install a native base's surface on `this_box`.
///
/// `lowered_args` are the already-lowered constructor arguments. The JS spec's
/// implicit derived constructor is `constructor(...args) { super(...args) }`, so
/// forwarding them is exactly what a written-out `super(...)` would have done —
/// which is how `new Seeded([["k", 9]])` on a `class Seeded extends Map {}`
/// seeds its backing, and how `new (class extends Event {})("tick")` gets its
/// `type`.
pub(crate) fn emit_native_instance_base_init(
    ctx: &mut FnCtx<'_>,
    base: NativeInstanceBase,
    this_box: &str,
    lowered_args: &[String],
) {
    let undef = crate::nanbox::double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
    match base {
        NativeInstanceBase::EventEmitter => {
            // The bare emitter seeds no state from its options bag, so the args
            // (already lowered for their side effects) are not forwarded.
            crate::expr::lower_event_emitter_subclass_init(ctx, this_box);
        }
        NativeInstanceBase::Map | NativeInstanceBase::Set => {
            let kind: i32 = if base == NativeInstanceBase::Map {
                0
            } else {
                1
            };
            // `new Map(iterable)` / `new Set(iterable)` — extra args are ignored
            // by the builtin, and `undefined` seeds an empty collection.
            let iterable = lowered_args.first().cloned().unwrap_or(undef);
            ctx.block().call(
                DOUBLE,
                "js_map_set_subclass_init",
                &[
                    (DOUBLE, this_box),
                    (I32, &kind.to_string()),
                    (DOUBLE, &iterable),
                ],
            );
        }
        NativeInstanceBase::Event | NativeInstanceBase::CustomEvent => {
            let arg0 = lowered_args
                .first()
                .cloned()
                .unwrap_or_else(|| undef.clone());
            let arg1 = lowered_args
                .get(1)
                .cloned()
                .unwrap_or_else(|| undef.clone());
            // `argc` drives the runtime's missing-`type` throw, matching Node's
            // `new Event()` TypeError.
            let argc = lowered_args.len().min(2).to_string();
            let is_custom = if base == NativeInstanceBase::CustomEvent {
                "1"
            } else {
                "0"
            }
            .to_string();
            ctx.block().call(
                DOUBLE,
                "js_event_subclass_init",
                &[
                    (DOUBLE, this_box),
                    (DOUBLE, &arg0),
                    (DOUBLE, &arg1),
                    (I32, &argc),
                    (I32, &is_custom),
                ],
            );
        }
    }
}

/// Emit `js_promise_subclass_init(this, executor)` for a no-own-ctor
/// `class X extends Promise {}` on the runtime `new X(executor)` path. Runs the
/// ECMA-262 Promise constructor against a hidden backing cell stashed on the
/// freshly-allocated instance. `lowered_args` are the already-lowered `new`
/// arguments; the first is the executor.
pub(crate) fn emit_promise_subclass_init(ctx: &mut FnCtx<'_>, lowered_args: &[String]) {
    let undef = crate::nanbox::double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
    let executor = lowered_args
        .first()
        .cloned()
        .unwrap_or_else(|| undef.clone());
    let this_box = ctx
        .this_stack
        .last()
        .cloned()
        .map(|slot| ctx.block().load(DOUBLE, &slot))
        .unwrap_or(undef);
    ctx.block().call(
        DOUBLE,
        "js_promise_subclass_init",
        &[(DOUBLE, &this_box), (DOUBLE, &executor)],
    );
}

/// Generic "does any statement in this ctor body satisfy `stmt_pred` or
/// contain an expression satisfying `expr_pred`" walker, shared by the
/// no-super static-throw heuristics below.
fn ctor_body_any(
    body: &[perry_hir::Stmt],
    expr_pred: &dyn Fn(&Expr) -> bool,
    stmt_pred: &dyn Fn(&perry_hir::Stmt) -> bool,
) -> bool {
    body.iter().any(|s| stmt_any(s, expr_pred, stmt_pred))
}

fn stmt_any(
    stmt: &perry_hir::Stmt,
    expr_pred: &dyn Fn(&Expr) -> bool,
    stmt_pred: &dyn Fn(&perry_hir::Stmt) -> bool,
) -> bool {
    use perry_hir::Stmt;
    if stmt_pred(stmt) {
        return true;
    }
    match stmt {
        Stmt::Let { init, .. } => init.as_ref().is_some_and(expr_pred),
        Stmt::Expr(e) | Stmt::Throw(e) => expr_pred(e),
        Stmt::Return(opt) => opt.as_ref().is_some_and(expr_pred),
        Stmt::If {
            condition,
            then_branch,
            else_branch,
        } => {
            expr_pred(condition)
                || ctor_body_any(then_branch, expr_pred, stmt_pred)
                || else_branch
                    .as_ref()
                    .is_some_and(|b| ctor_body_any(b, expr_pred, stmt_pred))
        }
        Stmt::While { condition, body } | Stmt::DoWhile { body, condition } => {
            expr_pred(condition) || ctor_body_any(body, expr_pred, stmt_pred)
        }
        Stmt::For {
            init,
            condition,
            update,
            body,
        } => {
            init.as_deref()
                .is_some_and(|s| stmt_any(s, expr_pred, stmt_pred))
                || condition.as_ref().is_some_and(expr_pred)
                || update.as_ref().is_some_and(expr_pred)
                || ctor_body_any(body, expr_pred, stmt_pred)
        }
        Stmt::Labeled { body, .. } => stmt_any(body, expr_pred, stmt_pred),
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            ctor_body_any(body, expr_pred, stmt_pred)
                || catch
                    .as_ref()
                    .is_some_and(|c| ctor_body_any(&c.body, expr_pred, stmt_pred))
                || finally
                    .as_ref()
                    .is_some_and(|f| ctor_body_any(f, expr_pred, stmt_pred))
        }
        Stmt::Switch {
            discriminant,
            cases,
        } => {
            expr_pred(discriminant)
                || cases.iter().any(|c| {
                    c.test.as_ref().is_some_and(expr_pred)
                        || ctor_body_any(&c.body, expr_pred, stmt_pred)
                })
        }
        Stmt::Break
        | Stmt::Continue
        | Stmt::LabeledBreak(_)
        | Stmt::LabeledContinue(_)
        | Stmt::PreallocateBoxes(_)
        | Stmt::PreallocateTdzBoxes(_) => false,
    }
}

const NO_STMT_PRED: &dyn Fn(&perry_hir::Stmt) -> bool = &|_| false;

/// True when a DIRECT `super(...)` call appears in this constructor body
/// (`walk_expr_children` does not descend into `Expr::Closure` bodies). A
/// derived constructor that never calls `super()` leaves `this`
/// uninitialized — ECMAScript then throws ReferenceError at the implicit
/// `return this`. We detect the static no-super case at compile time so
/// `new Sub()` throws instead of returning a half-built object.
pub(crate) fn ctor_body_calls_super(body: &[perry_hir::Stmt]) -> bool {
    ctor_body_any(body, &expr_calls_super, NO_STMT_PRED)
}

fn expr_calls_super(expr: &Expr) -> bool {
    if matches!(expr, Expr::SuperCall(_) | Expr::SuperCallSpread(_)) {
        return true;
    }
    let mut found = false;
    perry_hir::walker::walk_expr_children(expr, &mut |child| {
        if !found && expr_calls_super(child) {
            found = true;
        }
    });
    found
}

/// True when a closure (arrow) created in the ctor body contains a
/// `super(...)` call. Such an arrow can run DURING construction (e.g.
/// stored on an iterator and invoked from its `return()` while the ctor's
/// for-of is still iterating), so the static no-super throw must not fire —
/// unless the body also dereferences `this` directly (see the call site).
/// Refs class/subclass/derived-class-return-override-{for-of,finally-super}-arrow.
pub(crate) fn ctor_body_closure_calls_super(body: &[perry_hir::Stmt]) -> bool {
    ctor_body_any(body, &expr_calls_super_incl_closures, NO_STMT_PRED)
}

fn expr_calls_super_incl_closures(expr: &Expr) -> bool {
    if matches!(expr, Expr::SuperCall(_) | Expr::SuperCallSpread(_)) {
        return true;
    }
    if let Expr::Closure { body, .. } = expr {
        return ctor_body_any(body, &expr_calls_super_incl_closures, NO_STMT_PRED);
    }
    let mut found = false;
    perry_hir::walker::walk_expr_children(expr, &mut |child| {
        if !found && expr_calls_super_incl_closures(child) {
            found = true;
        }
    });
    found
}

/// True when the ctor body dereferences `this` OUTSIDE nested closures.
/// Combined with `ctor_body_closure_calls_super`: a direct `this` access in
/// a no-direct-super derived ctor throws ReferenceError per spec before any
/// closure could run `super()`, so the static entry throw stays correct
/// (test262 class/elements/privatefieldset-evaluation-order-1).
pub(crate) fn ctor_body_uses_this(body: &[perry_hir::Stmt]) -> bool {
    ctor_body_any(body, &expr_uses_this_direct, NO_STMT_PRED)
}

fn expr_uses_this_direct(expr: &Expr) -> bool {
    if matches!(expr, Expr::This) {
        return true;
    }
    if matches!(expr, Expr::Closure { .. }) {
        return false;
    }
    let mut found = false;
    perry_hir::walker::walk_expr_children(expr, &mut |child| {
        if !found && expr_uses_this_direct(child) {
            found = true;
        }
    });
    found
}

/// #2768: true when the constructor body reads `new.target` — directly, or
/// lexically from an arrow/closure that captured it. The default `new C()`
/// path calls the standalone `<class>_constructor` symbol (a separate compiled
/// function whose only `new.target` source is the runtime cell), so the cell
/// must be set around that call. Gating on this keeps the common ctor (no
/// `new.target`) on the zero-overhead fast path — no per-`new`-site cell writes.
pub(crate) fn ctor_body_uses_new_target(body: &[perry_hir::Stmt]) -> bool {
    ctor_body_any(body, &expr_uses_new_target, NO_STMT_PRED)
}

fn expr_uses_new_target(expr: &Expr) -> bool {
    match expr {
        Expr::NewTarget => true,
        // A closure's precomputed flag is authoritative; don't descend (the
        // walk below would otherwise re-scan its body).
        Expr::Closure {
            captures_new_target,
            ..
        } => *captures_new_target,
        _ => {
            let mut found = false;
            perry_hir::walker::walk_expr_children(expr, &mut |child| {
                if !found && expr_uses_new_target(child) {
                    found = true;
                }
            });
            found
        }
    }
}

/// True when the constructor body contains a value-bearing `return` in its
/// own body (closures excluded; a bare `return undefined` does NOT count —
/// spec falls back to the uninitialized `this` and still throws). The
/// return-override path initializes the `new` expression's value without
/// `super()`, so the static no-super ReferenceError must not fire —
/// `js_ctor_return_override` still enforces the derived-ctor rules on the
/// returned value at runtime. Refs
/// class/subclass/class-definition-null-proto-contains-return-override and
/// class/subclass/builtin-objects/Object/constructor-return-undefined-throws.
pub(crate) fn ctor_body_has_value_return(body: &[perry_hir::Stmt]) -> bool {
    ctor_body_any(
        body,
        &|_| false,
        &|s| matches!(s, perry_hir::Stmt::Return(Some(e)) if !matches!(e, Expr::Undefined)),
    )
}

pub(super) fn node_stream_parent_kind(
    ctx: &FnCtx<'_>,
    class: &perry_hir::Class,
) -> Option<&'static str> {
    let mut cur = class.extends_name.as_deref();
    let mut depth = 0usize;
    while let Some(name) = cur {
        match name {
            "Readable" => return Some("readable"),
            "Duplex" => return Some("duplex"),
            "Transform" => return Some("transform"),
            _ => {}
        }
        if ctx.imported_class_ctors.contains_key(name) {
            return None;
        }
        let Some(parent) = ctx.classes.get(name).copied() else {
            return None;
        };
        if parent.constructor.is_some() {
            return None;
        }
        cur = parent.extends_name.as_deref();
        depth += 1;
        if depth > 32 {
            break;
        }
    }
    None
}

/// Collect every LocalId DECLARED (via `Stmt::Let`, incl. nested in compound
/// statements) within a constructor body. Used to detect the wall-44 inline
/// collision: a ctor local whose id is also a capture of the enclosing closure.
/// Mirrors `collect_let_ids` in `class_members.rs`.
pub(super) fn collect_decl_local_ids(
    stmts: &[perry_hir::Stmt],
    out: &mut std::collections::HashSet<u32>,
) {
    use perry_hir::Stmt;
    for s in stmts {
        match s {
            Stmt::Let { id, .. } => {
                out.insert(*id);
            }
            Stmt::If {
                then_branch,
                else_branch,
                ..
            } => {
                collect_decl_local_ids(then_branch, out);
                if let Some(e) = else_branch {
                    collect_decl_local_ids(e, out);
                }
            }
            Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => {
                collect_decl_local_ids(body, out)
            }
            Stmt::For { init, body, .. } => {
                if let Some(init_stmt) = init {
                    if let Stmt::Let { id, .. } = init_stmt.as_ref() {
                        out.insert(*id);
                    }
                }
                collect_decl_local_ids(body, out);
            }
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                collect_decl_local_ids(body, out);
                if let Some(c) = catch {
                    collect_decl_local_ids(&c.body, out);
                }
                if let Some(f) = finally {
                    collect_decl_local_ids(f, out);
                }
            }
            Stmt::Switch { cases, .. } => {
                for case in cases {
                    collect_decl_local_ids(&case.body, out);
                }
            }
            Stmt::Labeled { body, .. } => {
                collect_decl_local_ids(std::slice::from_ref(body.as_ref()), out)
            }
            _ => {}
        }
    }
}

pub(crate) fn effective_constructor_param_count(ctx: &FnCtx<'_>, class: &Class) -> usize {
    if let Some(ctor) = class.constructor.as_ref() {
        return ctor.params.len();
    }
    let mut parent = class.extends_name.as_deref();
    while let Some(pname) = parent {
        if let Some(ctor) = ctx.imported_class_ctors.get(pname) {
            if ctor.stops_constructor_walk() {
                return ctor.param_count;
            }
        }
        match ctx.classes.get(pname).copied() {
            Some(pc) => {
                if let Some(pctor) = pc.constructor.as_ref() {
                    return pctor.params.len();
                }
                parent = pc.extends_name.as_deref();
            }
            None => break,
        }
    }
    0
}

/// True when the standalone `<class>_constructor` symbol exists (so the
/// recursion-guard / capture-collision redirect can call it instead of
/// inlining). Mirrors the lookup in `call_local_constructor_symbol`.
pub(crate) fn local_constructor_symbol_exists(ctx: &FnCtx<'_>, class: &Class) -> bool {
    let ctor_method_name = format!("{}_constructor", class.name);
    ctx.methods
        .contains_key(&(class.name.clone(), ctor_method_name))
}

/// #2768: true when the standalone `<class>_constructor` symbol's body reads
/// `new.target` — either the class's OWN ctor body, or an ancestor ctor body
/// it reaches through `super(...)`. The symbol is a separately compiled
/// function whose only `new.target` source is the runtime cell, and a
/// `super(...)` call inlines the parent ctor body into that same symbol, so an
/// ancestor that reads `new.target` (e.g. an abstract-class guard in a base)
/// still observes the cell. Gating the cell write on the WHOLE chain keeps
/// `new Child()` correct when only the inherited body reads `new.target`, while
/// a chain with no reader anywhere stays on the zero-overhead fast path. The
/// walk follows `extends_name` through the codegen class map; an unresolved
/// parent name just stops the walk, and a depth cap guards a cyclic graph.
pub(crate) fn ctor_chain_uses_new_target(ctx: &FnCtx<'_>, class: &Class) -> bool {
    let reads = |c: &Class| {
        c.constructor
            .as_ref()
            .is_some_and(|f| ctor_body_uses_new_target(&f.body))
    };
    if reads(class) {
        return true;
    }
    let mut parent = class.extends_name.as_deref();
    let mut depth = 0;
    while let Some(parent_name) = parent {
        depth += 1;
        if depth > 64 {
            break;
        }
        let Some(pc) = ctx.classes.get(parent_name).copied() else {
            break;
        };
        if reads(pc) {
            return true;
        }
        parent = pc.extends_name.as_deref();
    }
    false
}
