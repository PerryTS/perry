//! CommonJS module-scaffolding `Object.defineProperty` sites (#7139).
//!
//! Narrows the `Ptr<Shape>` rule-5 module-wide barrier
//! ([`super::ptr_shape`] doc, rule 5) so that the two `defineProperty` calls
//! every `cjs_wrap`-compiled CommonJS module contains — neither of them
//! anything to do with user objects — stop disabling shape promotion for the
//! whole module.
//!
//! ## The two sites
//!
//! 1. **Perry's own CJS preamble.** `cjs_wrap` emits
//!    `Object.defineProperty(require, 'name', { value: 'require', … })` into
//!    *every* wrapped module (`perry/src/commands/compile/cjs_wrap/wrap.rs`,
//!    the `cjs_preamble` literal). This alone means **100 %** of the
//!    CommonJS dependency graph carried `shape_barrier_sites = true`
//!    regardless of what the package source does.
//! 2. **The transpiled-CJS interop marker.**
//!    `Object.defineProperty(exports, "__esModule", { value: true })` — the
//!    single line tsc/Babel/esbuild put at the top of every emitted CJS file.
//!
//! Exempting only (2) would have recovered nothing, because (1) fires first
//! and unconditionally. Both are recognised here, and nothing else is.
//!
//! ## Predicate
//!
//! An `Expr::ObjectDefineProperty(target, key, desc)` node is exempt iff
//!
//! * `target` is `Expr::LocalGet(id)`;
//! * the module binds `id` with a `Stmt::Let` named `exports` (resp.
//!   `require`);
//! * **every** `Stmt::Let` binding of `id` in the module has an initializer in
//!   [`init_is_never_a_seed`]'s whitelist (`PropertyGet` / `Closure` /
//!   `Undefined` / absent);
//! * `key` is the string literal `"__esModule"` (resp. `"name"`).
//!
//! Every other `defineProperty` target, every other key, every computed key,
//! and every other barrier family (`delete`, `setPrototypeOf` / `__proto__`
//! write, `new Proxy`, mutating `Reflect.*`) keep the module-wide kill
//! untouched.
//!
//! ## Why it is sound
//!
//! A `defineProperty` can only invalidate a `Ptr<Shape>` local's proof if the
//! object it mutates *is* the object that local holds. Two independent facts
//! rule that out:
//!
//! * **The target is not a candidate.** `Ptr<Shape>` candidates are seeded by
//!   [`super::find_new_candidates`] from `Stmt::Let { init: Some(Expr::New
//!   { .. }), .. }`. The third clause above admits only initializers that are
//!   not fresh allocations at all — a field read, a function value, or nothing
//!   — so an exempted target can never be promoted, and, being a whitelist,
//!   it stays true if the seed set widens (#7034 §4's return-shape calls).
//!   The check is on the HIR, not on an expectation about the wrap template,
//!   so a future template change degrades to *no exemption* rather than to an
//!   unsound one.
//! * **No promoted object can reach the target.** Rule 2 (containment) admits
//!   a local only when *every* use of it is a declared-chain field
//!   read/write/update or a vetted method call; reassignment, aliasing,
//!   capture, and passing it as a call/constructor argument all disqualify.
//!   A promoted object therefore never flows into another binding, so it can
//!   never be the value of `exports` or `require`.
//!
//! The descriptor argument is deliberately unconstrained. A descriptor is a
//! plain value; if it contains an accessor closure, that closure's body is a
//! `Vec<Stmt>` the module barrier walk descends into on its own
//! (`for_each_expr` recurses through `Expr::Closure`), so a barrier *inside*
//! a descriptor still sets the flag.
//!
//! The flag this feeds (`ModuleDispatchFacts::shape_barrier_sites`) is also
//! read by `ptr_numarray` and `proven_this`. The argument above is about the
//! identity of the mutated object, not about which analysis consults the
//! flag, so it carries over unchanged: `exports` and `require` are likewise
//! never `Ptr<NumArray>` locals nor proven-`this` receivers.
//!
//! ---
//!
//! # The preamble's own ALLOCATIONS (#7152)
//!
//! Everything above is about the rule-5 barrier. [`preamble_in_region`] is
//! about a different self-inflicted wound in the same template: the objects
//! the preamble *allocates* are counted as `Ptr<Shape>` candidates and then
//! denied, in every CommonJS module, and on real dependency code they are the
//! majority of the report.
//!
//! Measured over 195 `__esModule` CJS modules from `scriptc/node_modules`:
//! 136 of the 140 rule-2 "bare reference" denials, 55 of the 187 rule-5
//! denials, and 188 of the 194 "constructor argument" unbound-allocation
//! denials are one of these two statements — 379 rows, ~2 per module, 31 % of
//! every `Ptr<Shape>` candidate in the corpus. #7139 read its share of them as
//! "containment is the wall in dependency JS" and scheduled #7149 on it; #7152
//! re-measured and put the wall at rule 1. Both readings were partly about
//! Perry's own scaffolding.
//!
//! ## What is recognised
//!
//! One **region** (a lowered statement list — the `cjs_wrap` IIFE's body, or
//! module init on the flat path) is a CommonJS preamble when its top level
//! carries all four of:
//!
//! * **R1** `Stmt::Let` named `__cjs_module`, `mutable: false`, initialized by
//!   an `Expr::New` of an `__AnonShape_…` class (an object literal);
//! * **R2** that literal is exactly `{ exports: {} }` — one field whose value
//!   is an argument-less `__AnonShape_…` allocation;
//! * **R3** exactly one top-level statement satisfying R1+R2, so "the record"
//!   is unambiguous;
//! * **R4** the same top level binds `var module = __cjs_module` —
//!   `Stmt::Let { mutable: true, init: LocalGet(<the record>) }`.
//!
//! Then, and only then, three things stop being reported: the record local
//! itself, the `{}` inside it, and the object literals of the preamble
//! statements [`CjsPreamble::stmt_allocates_only_scaffolding`] names.
//!
//! ## Why it is sound
//!
//! **This is a candidate SUPPRESSION, not a proof relaxation** — the opposite
//! direction from the barrier exemption above. Dropping a candidate can only
//! remove facts, never add one, so it cannot make codegen unsound; the entire
//! obligation is to show it never removes a fact that would otherwise exist.
//!
//! **R4 discharges that obligation outright.** It is not a heuristic about
//! the template, it is the denial itself: a `Stmt::Let` whose init is a bare
//! `Expr::LocalGet` of the record, with `mutable: true` so the alias pre-pass
//! in `ptr_shape.rs` refuses to track it, walks into `UseWalk`'s `LocalGet`
//! arm under the default escape context and disqualifies the record with
//! `ESC_BARE_REFERENCE` on every path. A region satisfying R4 therefore
//! *cannot* promote its record, so removing it from the candidate set leaves
//! the returned `HashMap` bit-identical. R1-R3 only make the recognition
//! unambiguous; they carry no soundness weight, and if any of them drifts the
//! candidate simply reappears in the report.
//!
//! And the escape R4 names is not incidental to the template — it is load
//! bearing. `var module` is what CommonJS bodies write `module.exports = X`
//! through, and the preamble goes on to store it into `require.main`. The
//! record is genuinely, permanently escaped; there is no narrowing of rule 2
//! that could promote it, which is why this is a suppression and not an
//! exemption.
//!
//! The allocation-site half is **report-only** in the strongest sense:
//! `ptr_shape_report::unbound_new_sites` is called exclusively under
//! `opt_report::enabled()`, and its output feeds nothing but the report.

use std::collections::HashSet;

use perry_hir::{Expr, Module, Stmt};

use super::scalar_method_dispatch::{for_each_expr, for_each_expr_in_stmts};

/// Binding name / property-key pairs the exemption recognises. Deliberately
/// exhaustive and literal — see the module doc.
const EXPORTS_BINDING: &str = "exports";
const EXPORTS_KEY: &str = "__esModule";
const REQUIRE_BINDING: &str = "require";
const REQUIRE_KEY: &str = "name";

/// The module's CommonJS-scaffolding bindings, resolved to `LocalId`s.
#[derive(Debug, Default)]
pub(super) struct CjsScaffolding {
    exports: HashSet<u32>,
    require: HashSet<u32>,
}

impl CjsScaffolding {
    /// Is `expr` one of the two recognised scaffolding `defineProperty`
    /// sites? Callers use this to *skip* setting
    /// `ModuleDispatchFacts::shape_barrier_sites`; it is never consulted for
    /// any other barrier family.
    pub(super) fn exempts_shape_barrier(&self, expr: &Expr) -> bool {
        let Expr::ObjectDefineProperty(target, key, _descriptor) = expr else {
            return false;
        };
        let Expr::LocalGet(id) = target.as_ref() else {
            return false;
        };
        let Expr::String(key) = key.as_ref() else {
            return false;
        };
        (key == EXPORTS_KEY && self.exports.contains(id))
            || (key == REQUIRE_KEY && self.require.contains(id))
    }
}

/// Resolve the module's `exports` / `require` scaffolding bindings.
///
/// Mirrors [`super::scalar_method_dispatch::collect_module_dispatch_facts`]'s
/// coverage: module init, every function body, every class member body, and
/// class field initializers / computed keys — plus every closure body nested
/// in any of them, which is where `cjs_wrap`'s IIFE puts the whole CommonJS
/// body.
pub(super) fn collect(module: &Module) -> CjsScaffolding {
    let mut acc = Acc::default();
    note_stmt_root(&module.init, &mut acc);
    for function in &module.functions {
        note_stmt_root(&function.body, &mut acc);
    }
    for class in &module.classes {
        if let Some(ctor) = &class.constructor {
            note_stmt_root(&ctor.body, &mut acc);
        }
        for method in class
            .methods
            .iter()
            .chain(class.static_methods.iter())
            .chain(class.getters.iter().map(|(_, f)| f))
            .chain(class.setters.iter().map(|(_, f)| f))
            .chain(class.computed_members.iter().map(|m| &m.function))
        {
            note_stmt_root(&method.body, &mut acc);
        }
        for field in class.fields.iter().chain(class.static_fields.iter()) {
            for expr in field.init.iter().chain(field.key_expr.iter()) {
                note_expr_root(expr, &mut acc);
            }
        }
        for member in &class.computed_members {
            note_expr_root(&member.key_expr, &mut acc);
        }
    }

    CjsScaffolding {
        exports: acc.exports.difference(&acc.disqualified).copied().collect(),
        require: acc.require.difference(&acc.disqualified).copied().collect(),
    }
}

#[derive(Default)]
struct Acc {
    exports: HashSet<u32>,
    require: HashSet<u32>,
    /// Every local with an initializer outside [`init_is_never_a_seed`]'s
    /// whitelist. Subtracted from both sets.
    disqualified: HashSet<u32>,
}

impl Acc {
    fn note_let(&mut self, stmt: &Stmt) {
        let Stmt::Let { id, name, init, .. } = stmt else {
            return;
        };
        match name.as_str() {
            EXPORTS_BINDING => {
                self.exports.insert(*id);
            }
            REQUIRE_BINDING => {
                self.require.insert(*id);
            }
            _ => {}
        }
        if !init_is_never_a_seed(init.as_ref()) {
            self.disqualified.insert(*id);
        }
    }
}

/// Can this initializer never make its local a `Ptr<Shape>` provenance seed?
///
/// Deliberately a **whitelist** of the three shapes `cjs_wrap` actually emits
/// for its scaffolding bindings, not a blacklist of `Expr::New`. Rule 1's seed
/// set is a moving target — #7034 §4 added return-shape facts so that a call to
/// a proven function *is* a provenance seed, and that machinery
/// (`ModuleDispatchFacts::return_shape_class`) already exists. A blacklist would
/// silently widen this exemption the day such a seed is wired into
/// [`super::find_new_candidates`]; a whitelist fails closed instead.
///
/// * `PropertyGet` — `var exports = __cjs_module.exports`. A field read of an
///   existing object, never a fresh allocation.
/// * `Closure` — `function require(specifier) { … }`. A function value.
/// * `Undefined` / no initializer — the hoisted `var` pre-declaration.
fn init_is_never_a_seed(init: Option<&Expr>) -> bool {
    matches!(
        init,
        None | Some(Expr::Undefined) | Some(Expr::PropertyGet { .. }) | Some(Expr::Closure { .. })
    )
}

// ── #7152: the preamble's own allocations, region by region ────────────────

/// The `cjs_wrap` module record's binding name. Perry writes it; no
/// transpiler emits it, and a user writing it is not a reason to promote.
const RECORD_BINDING: &str = "__cjs_module";
/// The `var module = __cjs_module;` alias — R4, the denial itself.
const MODULE_BINDING: &str = "module";
/// Object literals lower to `Expr::New` of a synthesised class with this
/// prefix (`perry-hir`'s anon-shape naming).
const ANON_SHAPE_PREFIX: &str = "__AnonShape_";
/// The two `require` properties the preamble installs with an object-literal
/// value. `require.resolve` / `require.resolve.paths` take closures, which
/// `unbound_new_sites` does not descend into, so they need no arm here.
const REQUIRE_LITERAL_KEYS: [&str; 2] = ["cache", "extensions"];

/// Perry's `cjs_wrap` preamble as it appears in ONE lowered region.
///
/// The record local and the scaffolding bindings live in ONE `Option` on
/// purpose. "Recognition failed" then has no representation in which anything
/// could still be suppressed — the `Default` is not a flag every reader has to
/// remember to test, it is the absence of the data they would need. Every
/// recognition failure lands there.
#[derive(Debug, Default)]
pub(super) struct CjsPreamble {
    /// `(the `const __cjs_module = { exports: {} }` local, the region's
    /// `exports` / `require` bindings resolved by the same whitelist [`collect`]
    /// applies module-wide)` — `Some` only when R1-R4 all hold.
    recognised: Option<(u32, CjsScaffolding)>,
}

impl CjsPreamble {
    /// Is `id` the recognised CommonJS module record? `ptr_shape.rs` drops it
    /// from the `Ptr<Shape>` candidate set — see the module doc for why that
    /// cannot change a fact.
    pub(super) fn is_module_record(&self, id: u32) -> bool {
        matches!(self.recognised, Some((record, _)) if record == id)
    }

    /// Does this statement allocate **only** preamble scaffolding, so that
    /// `ptr_shape_report::unbound_new_sites` should not walk it?
    ///
    /// Four shapes, all from the one template, all gated on the record having
    /// been recognised:
    ///
    /// 1. `const __cjs_module = { exports: {} };` — the inner `{}` is
    ///    `module.exports`, reported today as an unbound allocation in
    ///    *constructor-argument* position. It is the single most common
    ///    `Ptr<Shape>` denial in dependency JS.
    /// 2/3. The two `defineProperty` sites #7139 already recognises. Their
    ///    descriptor is a literal allocated in the call; the target check is
    ///    [`CjsScaffolding::exempts_shape_barrier`] verbatim, so the two
    ///    exemptions can never disagree about what "scaffolding" means.
    /// 4. `require.cache = {}` and `require.extensions = { … }`.
    ///
    /// Nothing else. A `Stmt::Expr` of anything else, and any statement in a
    /// region whose record was not recognised, is walked exactly as before.
    pub(super) fn stmt_allocates_only_scaffolding(&self, stmt: &Stmt) -> bool {
        let Some((record, scaffolding)) = &self.recognised else {
            return false;
        };
        match stmt {
            Stmt::Let { id, .. } => id == record,
            Stmt::Expr(expr) => match expr {
                Expr::ObjectDefineProperty(..) => scaffolding.exempts_shape_barrier(expr),
                Expr::PutValueSet { target, key, .. } => {
                    matches!(
                        (target.as_ref(), key.as_ref()),
                        (Expr::LocalGet(id), Expr::String(k))
                            if scaffolding.require.contains(id)
                                && REQUIRE_LITERAL_KEYS.contains(&k.as_str())
                    )
                }
                _ => false,
            },
            _ => false,
        }
    }
}

/// Recognise the `cjs_wrap` preamble in one lowered region.
///
/// Runs for **every** region of every module, so the first thing it does is a
/// single pass over the region's top-level statements looking for R1/R2. Only
/// `cjs_wrap` output binds `__cjs_module` to that literal, so on ordinary
/// TypeScript this returns `Default` after one `Vec` scan.
///
/// Deliberately NOT gated on [`crate::opt_report::enabled`], even though the
/// only observable effect is on the report. Gating it would make the candidate
/// set differ between a reporting build and an ordinary one — the facts would
/// still be identical (that is R4's argument), but "the report describes a
/// different compile than the one you ran" is the exact confusion this whole
/// report exists to remove. The cost of not gating it is one string compare per
/// top-level statement per region.
pub(super) fn preamble_in_region(stmts: &[Stmt]) -> CjsPreamble {
    // R1 + R2 (`record_binding`), on top-level bindings of R1's name.
    let records: Vec<u32> = stmts
        .iter()
        .filter(|stmt| matches!(stmt, Stmt::Let { name, .. } if name == RECORD_BINDING))
        .filter_map(record_binding)
        .collect();
    // R3: two records in one region means "the record" is ambiguous, so
    // recognise neither and let both stay candidates.
    let [record] = records[..] else {
        return CjsPreamble::default();
    };
    // R4: the bare reference that denies it. Without this the suppression
    // would be a claim about the template; with it, it is a claim about this
    // region's own statements.
    if !binds_module_alias(stmts, record) {
        return CjsPreamble::default();
    }
    let mut acc = Acc::default();
    note_stmt_root(stmts, &mut acc);
    CjsPreamble {
        recognised: Some((
            record,
            CjsScaffolding {
                exports: acc.exports.difference(&acc.disqualified).copied().collect(),
                require: acc.require.difference(&acc.disqualified).copied().collect(),
            },
        )),
    }
}

/// What the `Ptr<Shape>` report suppresses as `cjs_wrap` scaffolding in one
/// module. See [`census`].
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct CjsPreambleCensus {
    /// Regions whose CommonJS module record was recognised (R1-R4) and
    /// therefore dropped from `Ptr<Shape>` candidacy.
    pub module_records: usize,
    /// Top-level preamble statements, summed over those regions, whose object
    /// literals `unbound_new_sites` no longer walks.
    pub preamble_alloc_stmts: usize,
}

/// Count what [`preamble_in_region`] recognises across every lowering region
/// of `module`.
///
/// Exists **solely** for the `perry` crate's `cjs_wrap` template canary
/// (`commands/compile/cjs_wrap/preamble_canary_tests.rs`), the same coupling
/// [`crate::module_has_ptr_shape_barrier`] exists for: the template is in
/// `perry`, the recogniser is here, and a template edit would otherwise
/// silently un-recognise the preamble with no test going red. Nothing in the
/// compile pipeline calls it.
pub fn census(module: &Module) -> CjsPreambleCensus {
    let mut out = CjsPreambleCensus::default();
    let mut roots: Vec<&[Stmt]> = vec![&module.init];
    for function in &module.functions {
        roots.push(&function.body);
    }
    for class in &module.classes {
        if let Some(ctor) = &class.constructor {
            roots.push(&ctor.body);
        }
        for method in class
            .methods
            .iter()
            .chain(class.static_methods.iter())
            .chain(class.getters.iter().map(|(_, f)| f))
            .chain(class.setters.iter().map(|(_, f)| f))
            .chain(class.computed_members.iter().map(|m| &m.function))
        {
            roots.push(&method.body);
        }
    }
    for root in roots {
        note_region(root, &mut out);
        // Recurses through `Expr::Closure`, so this reaches every closure body
        // at any nesting depth exactly once — including the `cjs_wrap` IIFE,
        // which is where the preamble actually lives.
        for_each_expr_in_stmts(root, &mut |expr| {
            if let Expr::Closure { body, .. } = expr {
                note_region(body, &mut out);
            }
        });
    }
    out
}

/// Recognise one region and fold it into the census.
fn note_region(stmts: &[Stmt], out: &mut CjsPreambleCensus) {
    let preamble = preamble_in_region(stmts);
    if preamble.recognised.is_none() {
        return;
    }
    out.module_records += 1;
    out.preamble_alloc_stmts += stmts
        .iter()
        .filter(|s| preamble.stmt_allocates_only_scaffolding(s))
        .count();
}

/// R1 + R2 for a statement the caller has already matched on R1's binding
/// NAME. The name is checked exactly once, in [`preamble_in_region`]: two
/// enforcement points for one conjunct is how a sabotage hole gets in — either
/// one can be deleted with every test still green.
fn record_binding(stmt: &Stmt) -> Option<u32> {
    let Stmt::Let {
        id,
        mutable: false,
        init: Some(Expr::New {
            class_name, args, ..
        }),
        ..
    } = stmt
    else {
        return None;
    };
    if !class_name.starts_with(ANON_SHAPE_PREFIX) {
        return None;
    }
    // Exactly one field, whose value is an argument-less object literal. A
    // record with more fields, or a non-literal field value, is not the
    // template's `{ exports: {} }` and keeps its candidacy.
    let [Expr::New {
        class_name: inner,
        args: inner_args,
        ..
    }] = args.as_slice()
    else {
        return None;
    };
    (inner.starts_with(ANON_SHAPE_PREFIX) && inner_args.is_empty()).then_some(*id)
}

/// R4: `var module = __cjs_module;` at the region's top level.
///
/// `mutable: true` is required, not incidental: a `const` alias WOULD be
/// tracked by `ptr_shape.rs`'s alias pre-pass, and the record would then be
/// denied for a different reason (or, in principle, not at all) — so the
/// fact-neutrality argument would no longer hold.
fn binds_module_alias(stmts: &[Stmt], record: u32) -> bool {
    stmts.iter().any(|stmt| {
        matches!(
            stmt,
            Stmt::Let {
                name,
                mutable: true,
                init: Some(Expr::LocalGet(src)),
                ..
            } if name == MODULE_BINDING && *src == record
        )
    })
}

/// A statement list plus every closure body reachable from it.
fn note_stmt_root(stmts: &[Stmt], acc: &mut Acc) {
    for_each_stmt(stmts, &mut |stmt| acc.note_let(stmt));
    // `for_each_expr_in_stmts` already recurses through `Expr::Closure`, so
    // this yields every closure body at any nesting depth exactly once.
    for_each_expr_in_stmts(stmts, &mut |expr| {
        if let Expr::Closure { body, .. } = expr {
            for_each_stmt(body, &mut |stmt| acc.note_let(stmt));
        }
    });
}

/// Same, rooted at a bare expression (a class field initializer / computed
/// key). Bindings can only appear inside a closure from here.
fn note_expr_root(expr: &Expr, acc: &mut Acc) {
    for_each_expr(expr, &mut |node| {
        if let Expr::Closure { body, .. } = node {
            for_each_stmt(body, &mut |stmt| acc.note_let(stmt));
        }
    });
}

/// Every statement in `stmts`, descending through nested statement lists but
/// NOT into closure bodies (`note_stmt_root` reaches those separately).
fn for_each_stmt(stmts: &[Stmt], f: &mut dyn FnMut(&Stmt)) {
    for stmt in stmts {
        f(stmt);
        match stmt {
            Stmt::If {
                then_branch,
                else_branch,
                ..
            } => {
                for_each_stmt(then_branch, f);
                if let Some(branch) = else_branch {
                    for_each_stmt(branch, f);
                }
            }
            Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => for_each_stmt(body, f),
            Stmt::For { init, body, .. } => {
                if let Some(init) = init {
                    for_each_stmt(std::slice::from_ref(init.as_ref()), f);
                }
                for_each_stmt(body, f);
            }
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                for_each_stmt(body, f);
                if let Some(catch) = catch {
                    for_each_stmt(&catch.body, f);
                }
                if let Some(finally) = finally {
                    for_each_stmt(finally, f);
                }
            }
            Stmt::Switch { cases, .. } => {
                for case in cases {
                    for_each_stmt(&case.body, f);
                }
            }
            Stmt::Labeled { body, .. } => for_each_stmt(std::slice::from_ref(body.as_ref()), f),
            Stmt::Expr(_)
            | Stmt::Throw(_)
            | Stmt::Return(_)
            | Stmt::Let { .. }
            | Stmt::Break
            | Stmt::Continue
            | Stmt::LabeledBreak(_)
            | Stmt::LabeledContinue(_)
            | Stmt::PreallocateBoxes(_)
            | Stmt::PreallocateTdzBoxes(_) => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::ptr_shape::collect_shape_proven_ptr_locals;
    use super::super::scalar_method_dispatch::collect_module_dispatch_facts;
    use super::*;
    use perry_hir::types::Type;
    use perry_hir::{Class, ClassField, Function};
    use std::collections::HashMap;

    const REQUIRE_ID: u32 = 10;
    const CJS_MODULE_ID: u32 = 12;
    const EXPORTS_ID: u32 = 7;
    const MODULE_ID: u32 = 6;
    const POINT_ID: u32 = 42;

    fn closure(func_id: u32, body: Vec<Stmt>) -> Expr {
        Expr::Closure {
            func_id,
            params: Vec::new(),
            return_type: Type::Any,
            body,
            captures: Vec::new(),
            mutable_captures: Vec::new(),
            captures_this: false,
            captures_new_target: false,
            enclosing_class: None,
            is_arrow: false,
            is_async: false,
            is_generator: false,
            is_strict: true,
        }
    }

    fn let_stmt(id: u32, name: &str, init: Expr) -> Stmt {
        Stmt::Let {
            id,
            name: name.to_string(),
            ty: Type::Any,
            mutable: true,
            init: Some(init),
        }
    }

    /// `const <name> = <init>;` — `mutable: false`, which is what the template
    /// emits for `require` and `__cjs_module` and what R1 requires.
    fn let_const(id: u32, name: &str, init: Expr) -> Stmt {
        Stmt::Let {
            id,
            name: name.to_string(),
            ty: Type::Any,
            mutable: false,
            init: Some(init),
        }
    }

    fn anon_shape(class_name: &str, args: Vec<Expr>) -> Expr {
        Expr::New {
            class_name: class_name.to_string(),
            args,
            type_args: Vec::new(),
            byte_offset: 0,
            cap_args_appended: 0,
        }
    }

    /// `{ value: true }` — how a literal descriptor reaches HIR.
    fn descriptor() -> Expr {
        anon_shape("__AnonShape_desc", vec![Expr::Bool(true)])
    }

    fn define_property(target: Expr, key: Expr) -> Stmt {
        Stmt::Expr(Expr::ObjectDefineProperty(
            Box::new(target),
            Box::new(key),
            Box::new(descriptor()),
        ))
    }

    /// `const __cjs_module = { exports: {} };` — R1 + R2, exactly as the
    /// template lowers (checked against `--print-hir` on a wrapped module).
    fn record_stmt() -> Stmt {
        let_const(
            CJS_MODULE_ID,
            "__cjs_module",
            anon_shape(
                "__AnonShape_module",
                vec![anon_shape("__AnonShape_exports", Vec::new())],
            ),
        )
    }

    /// The `cjs_wrap` preamble as ONE lowered region, verbatim in HIR shape:
    /// the `require` closure, the `__cjs_module` record, the `var module`
    /// alias that denies it (R4), and `var exports = __cjs_module.exports`.
    /// `extra` is appended as the CommonJS body.
    fn cjs_region(extra: Vec<Stmt>) -> Vec<Stmt> {
        let mut body = vec![
            let_const(REQUIRE_ID, "require", closure(7, Vec::new())),
            record_stmt(),
            let_stmt(MODULE_ID, "module", Expr::LocalGet(CJS_MODULE_ID)),
            let_stmt(
                EXPORTS_ID,
                "exports",
                Expr::PropertyGet {
                    byte_offset: 0,
                    object: Box::new(Expr::LocalGet(CJS_MODULE_ID)),
                    property: "exports".to_string(),
                },
            ),
        ];
        body.extend(extra);
        body
    }

    /// The same region wrapped in the IIFE the wrap emits, as a whole module.
    fn cjs_module(extra: Vec<Stmt>) -> perry_hir::Module {
        let mut module = perry_hir::Module::new("node_modules/dep/index.js");
        module.init.push(let_stmt(
            0,
            "_cjs",
            Expr::Call {
                callee: Box::new(closure(2, cjs_region(extra))),
                args: Vec::new(),
                type_args: Vec::new(),
                byte_offset: 0,
            },
        ));
        module
    }

    /// The two scaffolding sites every `cjs_wrap` module carries.
    fn scaffolding_sites() -> Vec<Stmt> {
        vec![
            define_property(
                Expr::LocalGet(REQUIRE_ID),
                Expr::String(REQUIRE_KEY.to_string()),
            ),
            define_property(
                Expr::LocalGet(EXPORTS_ID),
                Expr::String(EXPORTS_KEY.to_string()),
            ),
        ]
    }

    fn barrier(extra: Vec<Stmt>) -> bool {
        collect_module_dispatch_facts(&cjs_module(extra)).has_shape_barrier_sites()
    }

    #[test]
    fn cjs_scaffolding_define_property_sites_do_not_arm_the_module_barrier() {
        assert!(!barrier(scaffolding_sites()));
    }

    /// Each half on its own — so a regression in either recogniser is named.
    #[test]
    fn each_scaffolding_site_is_exempt_on_its_own() {
        for site in scaffolding_sites() {
            assert!(!barrier(vec![site]));
        }
    }

    /// A module with NO scaffolding recogniser at all still has to arm — this
    /// is the anti-vacuity check for every assertion above.
    #[test]
    fn a_define_property_on_an_unrelated_target_still_arms_the_barrier() {
        let mut sites = scaffolding_sites();
        sites.push(define_property(
            Expr::LocalGet(99),
            Expr::String(EXPORTS_KEY.to_string()),
        ));
        assert!(barrier(sites));
    }

    /// Only the two recognised keys. `defineProperty(exports, "foo", …)` is a
    /// real named-export install and keeps the kill.
    #[test]
    fn another_key_on_the_exports_binding_still_arms_the_barrier() {
        assert!(barrier(vec![define_property(
            Expr::LocalGet(EXPORTS_ID),
            Expr::String("someNamedExport".to_string()),
        )]));
    }

    #[test]
    fn another_key_on_the_require_binding_still_arms_the_barrier() {
        assert!(barrier(vec![define_property(
            Expr::LocalGet(REQUIRE_ID),
            Expr::String("cache".to_string()),
        )]));
    }

    /// A computed key could be `"__esModule"` at runtime, but it could be
    /// anything else too; the predicate demands a literal.
    #[test]
    fn a_computed_key_on_the_exports_binding_still_arms_the_barrier() {
        assert!(barrier(vec![define_property(
            Expr::LocalGet(EXPORTS_ID),
            Expr::LocalGet(55),
        )]));
    }

    /// A user binding that happens to be named `exports`, initialized by
    /// something outside the scaffolding whitelist, is never exempt.
    ///
    /// `new` is the soundness hinge today: such a binding IS a rule-1
    /// `Ptr<Shape>` candidate. `Call` guards the forward direction — #7034 §4's
    /// return-shape facts already make a call to a proven function a provenance
    /// seed, so a blacklist of `Expr::New` would silently widen this exemption
    /// the day that seed is wired into `find_new_candidates`.
    #[test]
    fn an_exports_binding_outside_the_init_whitelist_is_not_exempt() {
        let inits = [
            anon_shape("__AnonShape_user", Vec::new()),
            Expr::Call {
                callee: Box::new(Expr::LocalGet(77)),
                args: Vec::new(),
                type_args: Vec::new(),
                byte_offset: 0,
            },
        ];
        for init in inits {
            let mut m = perry_hir::Module::new("m.ts");
            m.init.push(let_stmt(EXPORTS_ID, "exports", init.clone()));
            m.init.push(define_property(
                Expr::LocalGet(EXPORTS_ID),
                Expr::String(EXPORTS_KEY.to_string()),
            ));
            assert!(
                collect_module_dispatch_facts(&m).has_shape_barrier_sites(),
                "expected a barrier for an `exports` bound to {init:?}"
            );
        }
    }

    /// A LATER binding of the same id outside the whitelist disqualifies the
    /// scaffolding binding too — `var` redeclaration reuses the `LocalId`.
    #[test]
    fn a_disqualifying_rebinding_of_the_exports_id_removes_the_exemption() {
        let mut extra = scaffolding_sites();
        extra.push(let_stmt(
            EXPORTS_ID,
            "exports",
            anon_shape("__AnonShape_user", Vec::new()),
        ));
        assert!(barrier(extra));
    }

    /// Untouched barrier families: the exemption is scoped to
    /// `ObjectDefineProperty`, and only to two targets.
    #[test]
    fn the_other_barrier_families_are_untouched() {
        let others = [
            Expr::Delete(Box::new(Expr::LocalGet(EXPORTS_ID))),
            Expr::ObjectSetPrototypeOf(Box::new(Expr::LocalGet(EXPORTS_ID)), Box::new(Expr::Null)),
            Expr::ObjectDefineProperties(
                Box::new(Expr::LocalGet(EXPORTS_ID)),
                Box::new(descriptor()),
            ),
            Expr::ReflectSet {
                target: Box::new(Expr::LocalGet(EXPORTS_ID)),
                key: Box::new(Expr::String(EXPORTS_KEY.to_string())),
                value: Box::new(Expr::Bool(true)),
                receiver: Box::new(Expr::LocalGet(EXPORTS_ID)),
            },
        ];
        for other in others {
            let mut sites = scaffolding_sites();
            sites.push(Stmt::Expr(other.clone()));
            assert!(barrier(sites), "expected a barrier for {other:?}");
        }
    }

    // ---- end-to-end: the exemption actually recovers a promotion ----

    fn point_class() -> Class {
        Class {
            id: 0,
            name: "Point".to_string(),
            type_params: Vec::new(),
            extends: None,
            extends_name: None,
            native_extends: None,
            extends_expr: None,
            heritage_lexically_shadowed: false,
            fields: ["x", "y"]
                .iter()
                .map(|n| ClassField {
                    name: n.to_string(),
                    key_expr: None,
                    ty: Type::Number,
                    init: None,
                    is_private: false,
                    is_readonly: false,
                    decorators: Vec::new(),
                })
                .collect(),
            constructor: None,
            methods: Vec::new(),
            getters: Vec::new(),
            setters: Vec::new(),
            static_fields: Vec::new(),
            static_methods: Vec::new(),
            computed_members: Vec::new(),
            decorators: Vec::new(),
            is_exported: false,
            aliases: Vec::new(),
            is_nested: false,
            alloc_width_hint: 0,
            static_accessor_names: Vec::new(),
            static_accessor_fn_ids: Vec::new(),
        }
    }

    /// `const p = new Point(); p.x = 1; return p.x;` — a textbook rule-1..4
    /// promotion, so the ONLY thing that can deny it is the rule-5 kill.
    fn promotable_body() -> Vec<Stmt> {
        vec![
            let_stmt(POINT_ID, "p", anon_shape("Point", Vec::new())),
            Stmt::Expr(Expr::PropertySet {
                object: Box::new(Expr::LocalGet(POINT_ID)),
                property: "x".to_string(),
                value: Box::new(Expr::Number(1.0)),
            }),
            Stmt::Return(Some(Expr::PropertyGet {
                byte_offset: 0,
                object: Box::new(Expr::LocalGet(POINT_ID)),
                property: "x".to_string(),
            })),
        ]
    }

    fn compute_fn() -> Function {
        Function {
            id: 17,
            name: "compute".to_string(),
            type_params: Vec::new(),
            params: Vec::new(),
            return_type: Type::Number,
            body: promotable_body(),
            is_async: false,
            is_generator: false,
            is_strict: true,
            is_exported: true,
            captures: Vec::new(),
            decorators: Vec::new(),
            was_plain_async: false,
            was_unrolled: false,
        }
    }

    fn promotes_with(extra: Vec<Stmt>) -> bool {
        let mut module = cjs_module(extra);
        module.classes.push(point_class());
        module.functions.push(compute_fn());
        let facts = collect_module_dispatch_facts(&module);
        let point = point_class();
        let classes = HashMap::from([("Point".to_string(), &point)]);
        !collect_shape_proven_ptr_locals(
            &promotable_body(),
            &HashSet::new(),
            &HashMap::new(),
            &classes,
            &facts,
            &HashSet::new(),
            // #7034 §3: this fixture builds no array, so the element facts are
            // empty either way — computed rather than defaulted so the two
            // passes cannot drift apart here.
            &crate::collectors::ptr_shape_elements::collect_element_shape_facts(
                &promotable_body(),
                &HashSet::new(),
                &HashMap::new(),
                &classes,
                &facts,
            ),
        )
        .is_empty()
    }

    /// Red before #7139, green after: the CommonJS scaffolding no longer
    /// denies an eligible local elsewhere in the module.
    #[test]
    fn an_eligible_local_promotes_in_a_module_carrying_only_scaffolding_sites() {
        assert!(promotes_with(scaffolding_sites()));
    }

    /// Sabotage in the other direction: one genuine barrier anywhere in the
    /// module still denies that same local, so the test above is not asserting
    /// a promotion that would happen regardless.
    #[test]
    fn the_same_local_is_denied_when_a_real_barrier_is_present() {
        let mut sites = scaffolding_sites();
        sites.push(Stmt::Expr(Expr::Delete(Box::new(Expr::LocalGet(
            EXPORTS_ID,
        )))));
        assert!(!promotes_with(sites));
    }

    // ── #7152: the preamble's own allocations ──────────────────────────────

    /// `require.cache = {}` / `require.extensions = { … }`.
    fn require_install(key: &str, value: Expr) -> Stmt {
        Stmt::Expr(Expr::PutValueSet {
            target: Box::new(Expr::LocalGet(REQUIRE_ID)),
            key: Box::new(Expr::String(key.to_string())),
            value: Box::new(value),
            receiver: Box::new(Expr::LocalGet(REQUIRE_ID)),
            strict: true,
        })
    }

    /// Every ALLOCATING statement the preamble emits after the record itself,
    /// in template order.
    fn preamble_alloc_stmts() -> Vec<Stmt> {
        vec![
            define_property(
                Expr::LocalGet(REQUIRE_ID),
                Expr::String(REQUIRE_KEY.to_string()),
            ),
            require_install("cache", anon_shape("__AnonShape_cache", Vec::new())),
            require_install(
                "extensions",
                anon_shape("__AnonShape_ext", vec![closure(12, Vec::new())]),
            ),
            define_property(
                Expr::LocalGet(EXPORTS_ID),
                Expr::String(EXPORTS_KEY.to_string()),
            ),
        ]
    }

    /// A user allocation in statement position — never suppressed, and the
    /// anti-vacuity control for every suppression assertion below.
    fn user_alloc_stmt() -> Stmt {
        Stmt::Expr(anon_shape("__AnonShape_user", Vec::new()))
    }

    fn recognises(region: &[Stmt]) -> bool {
        preamble_in_region(region).is_module_record(CJS_MODULE_ID)
    }

    /// Swap the region's record `Let` for `replacement`.
    fn region_with_record(replacement: Stmt) -> Vec<Stmt> {
        let mut region = cjs_region(Vec::new());
        let at = region
            .iter()
            .position(|s| matches!(s, Stmt::Let { id, .. } if *id == CJS_MODULE_ID))
            .expect("the fixture binds the record");
        region[at] = replacement;
        region
    }

    #[test]
    fn the_wrap_preamble_record_is_recognised() {
        assert!(recognises(&cjs_region(Vec::new())));
    }

    /// **Anti-vacuity for everything in this section.** The record IS a rule-1
    /// provenance seed: without the suppression it becomes a `Ptr<Shape>`
    /// candidate, is denied, and is reported. If this ever goes red the
    /// suppression is a no-op and the tests below prove nothing.
    #[test]
    fn the_record_is_a_ptr_shape_seed_in_the_first_place() {
        let mut seeds = HashMap::new();
        super::super::find_new_candidates(
            &cjs_region(Vec::new()),
            &HashSet::new(),
            &HashMap::new(),
            &mut seeds,
        );
        assert!(
            seeds.contains_key(&CJS_MODULE_ID),
            "the record stopped being a rule-1 seed; the #7152 suppression now \
             removes nothing and every assertion in this section is vacuous"
        );
    }

    /// **The fact-neutrality argument, tested.** R4's statement shape IS the
    /// rule-2 denial: a `var` alias of a local that otherwise promotes denies
    /// it. So a region carrying that alias can never promote its record, and
    /// dropping the record from candidacy cannot lose a promotion.
    #[test]
    fn a_var_alias_denies_a_local_that_otherwise_promotes() {
        let point = point_class();
        let classes = HashMap::from([("Point".to_string(), &point)]);
        let facts = collect_module_dispatch_facts(&perry_hir::Module::new("m.ts"));
        let proven = |body: &[Stmt]| {
            !collect_shape_proven_ptr_locals(
                body,
                &HashSet::new(),
                &HashMap::new(),
                &classes,
                &facts,
                &HashSet::new(),
                &crate::collectors::ptr_shape_elements::ElementShapeFacts::default(),
            )
            .is_empty()
        };
        // Control: the same body without the alias DOES promote.
        assert!(proven(&promotable_body()));
        let mut aliased = promotable_body();
        aliased.insert(1, let_stmt(MODULE_ID, "module", Expr::LocalGet(POINT_ID)));
        assert!(
            !proven(&aliased),
            "a `var m = p` alias no longer denies `p`. R4 is then not the \
             denial it is documented to be, and the #7152 suppression could \
             be dropping a value that would have been promoted."
        );
    }

    // ---- sabotage: one red set per conjunct ----

    /// R1: `mutable: false`. A reassignable record is not the template's.
    #[test]
    fn a_mutable_record_binding_is_not_recognised() {
        assert!(!recognises(&region_with_record(let_stmt(
            CJS_MODULE_ID,
            "__cjs_module",
            anon_shape(
                "__AnonShape_module",
                vec![anon_shape("__AnonShape_exports", Vec::new())],
            ),
        ))));
    }

    /// R1: the binding name. Only `cjs_wrap` writes this one.
    #[test]
    fn a_differently_named_record_binding_is_not_recognised() {
        assert!(!recognises(&region_with_record(let_const(
            CJS_MODULE_ID,
            "__cjs_modul",
            anon_shape(
                "__AnonShape_module",
                vec![anon_shape("__AnonShape_exports", Vec::new())],
            ),
        ))));
    }

    /// R1: an object literal, not a user class. `new Wrapper({})` is a value
    /// with a constructor that can do anything.
    #[test]
    fn a_record_of_a_declared_class_is_not_recognised() {
        assert!(!recognises(&region_with_record(let_const(
            CJS_MODULE_ID,
            "__cjs_module",
            anon_shape(
                "Wrapper",
                vec![anon_shape("__AnonShape_exports", Vec::new())]
            ),
        ))));
    }

    /// R2: exactly one field. `{ exports: {}, id: {} }` is not the template.
    #[test]
    fn a_record_literal_with_a_second_field_is_not_recognised() {
        assert!(!recognises(&region_with_record(let_const(
            CJS_MODULE_ID,
            "__cjs_module",
            anon_shape(
                "__AnonShape_module",
                vec![
                    anon_shape("__AnonShape_exports", Vec::new()),
                    anon_shape("__AnonShape_extra", Vec::new()),
                ],
            ),
        ))));
    }

    /// R2: the field's value is an EMPTY literal. `{ exports: { a: 1 } }`
    /// carries state the suppression makes no claim about.
    #[test]
    fn a_record_whose_exports_literal_is_not_empty_is_not_recognised() {
        assert!(!recognises(&region_with_record(let_const(
            CJS_MODULE_ID,
            "__cjs_module",
            anon_shape(
                "__AnonShape_module",
                vec![anon_shape("__AnonShape_exports", vec![Expr::Number(1.0)])],
            ),
        ))));
    }

    /// R2: the field's value is an allocation at all.
    #[test]
    fn a_record_whose_exports_field_is_not_an_allocation_is_not_recognised() {
        assert!(!recognises(&region_with_record(let_const(
            CJS_MODULE_ID,
            "__cjs_module",
            anon_shape("__AnonShape_module", vec![Expr::Undefined]),
        ))));
    }

    /// R3: two top-level bindings of the name — "the record" is ambiguous, so
    /// neither is recognised and both keep their candidacy.
    #[test]
    fn two_record_bindings_in_one_region_are_ambiguous() {
        let mut region = cjs_region(Vec::new());
        region.push(let_const(
            CJS_MODULE_ID + 100,
            "__cjs_module",
            anon_shape(
                "__AnonShape_module",
                vec![anon_shape("__AnonShape_exports", Vec::new())],
            ),
        ));
        assert!(!recognises(&region));
    }

    /// R4: no alias at all. Without the escape that denies it, the record is
    /// a candidate like any other and must stay in the report.
    #[test]
    fn without_the_module_alias_the_record_is_not_recognised() {
        let region: Vec<Stmt> = cjs_region(Vec::new())
            .into_iter()
            .filter(|s| !matches!(s, Stmt::Let { id, .. } if *id == MODULE_ID))
            .collect();
        assert!(!recognises(&region));
    }

    /// R4: `mutable: true`. A `const` alias is TRACKED by `ptr_shape.rs`'s
    /// alias pre-pass rather than treated as an escape, so it does not
    /// discharge the fact-neutrality obligation.
    #[test]
    fn a_const_module_alias_does_not_satisfy_r4() {
        let mut region = cjs_region(Vec::new());
        let at = region
            .iter()
            .position(|s| matches!(s, Stmt::Let { id, .. } if *id == MODULE_ID))
            .expect("the fixture binds the alias");
        region[at] = let_const(MODULE_ID, "module", Expr::LocalGet(CJS_MODULE_ID));
        assert!(!recognises(&region));
    }

    /// R4: the alias must be of THIS record.
    #[test]
    fn a_module_alias_of_another_local_does_not_satisfy_r4() {
        let mut region = cjs_region(Vec::new());
        let at = region
            .iter()
            .position(|s| matches!(s, Stmt::Let { id, .. } if *id == MODULE_ID))
            .expect("the fixture binds the alias");
        region[at] = let_stmt(MODULE_ID, "module", Expr::LocalGet(REQUIRE_ID));
        assert!(!recognises(&region));
    }

    // ---- the allocation-site suppression ----

    #[test]
    fn the_preamble_allocation_statements_are_suppressed() {
        let region = cjs_region(preamble_alloc_stmts());
        let preamble = preamble_in_region(&region);
        let suppressed: Vec<bool> = region
            .iter()
            .map(|s| preamble.stmt_allocates_only_scaffolding(s))
            .collect();
        // record `Let` + the four preamble statements; `require`, the alias
        // and the `exports` read allocate nothing and are irrelevant either
        // way, but must not be claimed.
        assert_eq!(
            suppressed.iter().filter(|b| **b).count(),
            5,
            "{suppressed:?}"
        );
        assert!(preamble.stmt_allocates_only_scaffolding(&record_stmt()));
        for stmt in preamble_alloc_stmts() {
            assert!(
                preamble.stmt_allocates_only_scaffolding(&stmt),
                "not suppressed: {stmt:?}"
            );
        }
    }

    #[test]
    fn user_allocations_are_never_suppressed() {
        let region = cjs_region(preamble_alloc_stmts());
        let preamble = preamble_in_region(&region);
        let kept = [
            user_alloc_stmt(),
            // A `defineProperty` whose target is neither scaffolding binding.
            define_property(Expr::LocalGet(77), Expr::String(EXPORTS_KEY.to_string())),
            // The right target, a key the preamble does not install.
            require_install("main", anon_shape("__AnonShape_user", Vec::new())),
            // The right key on the wrong object.
            Stmt::Expr(Expr::PutValueSet {
                target: Box::new(Expr::LocalGet(77)),
                key: Box::new(Expr::String("cache".to_string())),
                value: Box::new(anon_shape("__AnonShape_user", Vec::new())),
                receiver: Box::new(Expr::LocalGet(77)),
                strict: true,
            }),
            // A user `let` of an object literal — a real rule-1 candidate.
            let_const(55, "row", anon_shape("__AnonShape_row", Vec::new())),
        ];
        for stmt in kept {
            assert!(
                !preamble.stmt_allocates_only_scaffolding(&stmt),
                "wrongly suppressed: {stmt:?}"
            );
        }
    }

    /// Nothing is suppressed in a region whose record was not recognised —
    /// the whole exemption is gated on R1-R4, one instance at a time.
    #[test]
    fn an_unrecognised_region_suppresses_nothing() {
        let region: Vec<Stmt> = cjs_region(preamble_alloc_stmts())
            .into_iter()
            .filter(|s| !matches!(s, Stmt::Let { id, .. } if *id == MODULE_ID))
            .collect();
        let preamble = preamble_in_region(&region);
        for stmt in &region {
            assert!(
                !preamble.stmt_allocates_only_scaffolding(stmt),
                "suppressed without a recognised record: {stmt:?}"
            );
        }
    }

    /// End to end through the report walk that consumes this: the scaffolding
    /// allocations disappear from `unbound_new_sites` and the user's does not.
    #[test]
    fn the_report_walk_drops_the_scaffolding_allocations_only() {
        let region = cjs_region({
            let mut body = preamble_alloc_stmts();
            body.push(user_alloc_stmt());
            body
        });
        let base =
            super::super::ptr_shape_report::unbound_new_sites(&region, &CjsPreamble::default());
        let contexts: Vec<&str> = base.iter().map(|s| s.context).collect();
        // #7170 §5.1 / R0 renamed this bucket, and the rename is measured on
        // exactly this statement: `const __cjs_module = { exports: {} }` is an
        // anonymous-shape allocation whose constructor arguments ARE its
        // property values, so the inner `{}` is a *component of a literal*, not
        // an argument of a `new C(...)`. It was 188 of the 194 rows the old
        // `constructor argument` label carried (PR #7171 §1) — which is why
        // that label could not be read as "constructor arguments".
        assert!(
            contexts.contains(&"object literal property value"),
            "the `{{ exports: {{}} }}` inner literal is no longer reported \
             unsuppressed; this test's premise is gone: {contexts:?}"
        );
        assert!(base.len() >= 6, "{contexts:?}");

        let fixed = super::super::ptr_shape_report::unbound_new_sites(
            &region,
            &preamble_in_region(&region),
        );
        assert_eq!(
            fixed.len(),
            1,
            "{:?}",
            fixed.iter().map(|s| s.context).collect::<Vec<_>>()
        );
        assert_eq!(fixed[0].context, "statement");
    }

    /// The suppression itself, at the one place it is applied: the record is
    /// not a rule-1 candidate, a user record in the same region still is, and
    /// with the recogniser unarmed the record comes straight back.
    #[test]
    fn the_record_is_not_a_ptr_shape_candidate() {
        let region = cjs_region(vec![let_const(
            55,
            "row",
            anon_shape("__AnonShape_row", Vec::new()),
        )]);
        let seeds = |p: &CjsPreamble| {
            super::super::ptr_shape_report::candidate_seeds(
                &region,
                &HashSet::new(),
                &HashMap::new(),
                p,
            )
        };
        let fixed = seeds(&preamble_in_region(&region));
        assert!(!fixed.contains_key(&CJS_MODULE_ID));
        assert!(
            fixed.contains_key(&55),
            "a user object literal in the same region must still be a candidate"
        );
        assert!(seeds(&CjsPreamble::default()).contains_key(&CJS_MODULE_ID));
    }

    /// The census the `perry` crate's template canary reads.
    #[test]
    fn the_census_counts_one_preamble_per_wrapped_module() {
        let c = census(&cjs_module(preamble_alloc_stmts()));
        assert_eq!(c.module_records, 1);
        assert_eq!(c.preamble_alloc_stmts, 5);
        // An ordinary module has none.
        let mut plain = perry_hir::Module::new("m.ts");
        plain.init.push(let_const(
            55,
            "row",
            anon_shape("__AnonShape_row", Vec::new()),
        ));
        assert_eq!(census(&plain), CjsPreambleCensus::default());
    }
}
