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

    /// The `cjs_wrap` preamble, verbatim in HIR shape: a `require` closure, the
    /// `__cjs_module` record, and `exports` read out of it. `extra` is appended
    /// to the CommonJS body inside the IIFE.
    fn cjs_module(extra: Vec<Stmt>) -> perry_hir::Module {
        let mut body = vec![
            let_stmt(REQUIRE_ID, "require", closure(7, Vec::new())),
            let_stmt(
                CJS_MODULE_ID,
                "__cjs_module",
                anon_shape(
                    "__AnonShape_module",
                    vec![anon_shape("__AnonShape_exports", Vec::new())],
                ),
            ),
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
        let mut module = perry_hir::Module::new("node_modules/dep/index.js");
        module.init.push(let_stmt(
            0,
            "_cjs",
            Expr::Call {
                callee: Box::new(closure(2, body)),
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
}
