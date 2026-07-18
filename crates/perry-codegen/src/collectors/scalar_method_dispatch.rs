//! Dispatch-stability facts for the scalar-replacement method summary (#5872).
//!
//! [`simple_scalar_method_summary`](super::simple_scalar_method_summary) proves
//! that a method *body* is a numeric expression over `this.<field>`. Scalar
//! replacement of the receiver needs a second, independent proof: that
//! `obj.m(...)` still RESOLVES to that class method at the call site. JS lets
//! user code break the lookup after the object is constructed:
//!
//! * an own-property write shadows the prototype method — `(obj as any).m =
//!   () => 99` wins over `C.prototype.m`;
//! * a prototype mutation replaces the method — `C.prototype.m = fn`,
//!   `Object.defineProperty(C.prototype, "m", …)`, `let p = C.prototype; p.m =
//!   fn` — and it can happen in *another* function, in a constructor, or in a
//!   field initializer, i.e. nowhere near the `new`.
//!
//! Before #5466 codegen escaped every receiver that reached a method call, so
//! neither could bite: the perry-transform exact-receiver inliner was the only
//! layer that folded `obj.m()` into a field read, and it invalidates its facts
//! on exactly those mutations. #5466 taught codegen's escape check to keep a
//! summarized receiver scalar-replaced, but the summary only inspects the class
//! *declaration* — so a post-construction shadow was invisible and `obj.m()`
//! folded to the field value (#5872: `ownMethodWrite()` returned 14, not 99).
//!
//! Two fact sets restore the missing half:
//!
//! * [`ModuleDispatchFacts`] — module-scoped: which classes' prototypes are
//!   named anywhere in the module. Naming is enough, because a named prototype
//!   can be aliased and written through. A per-function walk cannot see this;
//!   the mutation typically lives in a helper the constructor calls.
//! * [`collect_candidate_property_writes`] — function-scoped: which property
//!   names are written directly on each scalar-replacement candidate.
//!
//! Both are deliberately narrow: they only ever *remove* a receiver from the
//! scalar-replaced set, and only when it is the target of a summarized method
//! call. Plain field scalar replacement is untouched.

use std::collections::{HashMap, HashSet};

use perry_hir::{Class, Expr, Module, Stmt};

/// Everything in a module that can change what `C.prototype.<m>` resolves to.
#[derive(Debug, Clone)]
pub struct ModuleDispatchFacts {
    /// Classes whose prototype object is named — read or written — anywhere in
    /// the module. Reading is enough: `let p = C.prototype; p.m = fn` mutates
    /// through an alias, and `<Class>.prototype.<m> = fn` lowers to
    /// `Expr::RegisterPrototypeMethod` only when the recogniser matches.
    prototype_touched_classes: HashSet<String>,
    /// A prototype was named through an expression that cannot be attributed to
    /// a declared class (`k.prototype`, `x.constructor.prototype`, …). Nothing
    /// in the module can then be trusted to keep a stable method table.
    opaque_prototype_mutation: bool,
}

impl Default for ModuleDispatchFacts {
    /// Fail safe: a fact set that was never populated must not license the
    /// scalar-method summary.
    fn default() -> Self {
        Self {
            prototype_touched_classes: HashSet::new(),
            opaque_prototype_mutation: true,
        }
    }
}

impl ModuleDispatchFacts {
    /// True when nothing in the module can rewrite the method table of
    /// `class_name` or of any class it inherits from.
    pub(crate) fn prototype_is_stable(
        &self,
        classes: &HashMap<String, &Class>,
        class_name: &str,
    ) -> bool {
        if self.opaque_prototype_mutation {
            return false;
        }
        let mut current = Some(class_name.to_string());
        let mut seen = HashSet::new();
        let mut depth = 0usize;
        while let Some(name) = current {
            depth += 1;
            if depth > 64 || !seen.insert(name.clone()) {
                return false;
            }
            if self.prototype_touched_classes.contains(&name) {
                return false;
            }
            let Some(class) = classes.get(&name).copied() else {
                return false;
            };
            if class.extends_expr.is_some() || class.native_extends.is_some() {
                return false;
            }
            current = class.extends_name.clone();
        }
        true
    }
}

/// Scan a whole module — top-level init, every function, and every class body
/// (constructor, field initializers, methods, accessors, computed members) —
/// for expressions that can rewrite a class's prototype.
pub fn collect_module_dispatch_facts(hir: &Module) -> ModuleDispatchFacts {
    let mut facts = ModuleDispatchFacts {
        prototype_touched_classes: HashSet::new(),
        opaque_prototype_mutation: false,
    };

    note_stmts(&hir.init, &mut facts);
    for function in &hir.functions {
        note_stmts(&function.body, &mut facts);
    }
    for class in &hir.classes {
        if let Some(ctor) = &class.constructor {
            note_stmts(&ctor.body, &mut facts);
        }
        for method in class
            .methods
            .iter()
            .chain(class.static_methods.iter())
            .chain(class.getters.iter().map(|(_, f)| f))
            .chain(class.setters.iter().map(|(_, f)| f))
            .chain(class.computed_members.iter().map(|m| &m.function))
        {
            note_stmts(&method.body, &mut facts);
        }
        for field in class.fields.iter().chain(class.static_fields.iter()) {
            if let Some(init) = &field.init {
                note_expr_tree(init, &mut facts);
            }
            if let Some(key) = &field.key_expr {
                note_expr_tree(key, &mut facts);
            }
        }
        for member in &class.computed_members {
            note_expr_tree(&member.key_expr, &mut facts);
        }
    }

    facts
}

fn note_stmts(stmts: &[Stmt], facts: &mut ModuleDispatchFacts) {
    for_each_expr_in_stmts(stmts, &mut |expr| note_prototype_effect(expr, facts));
}

fn note_expr_tree(expr: &Expr, facts: &mut ModuleDispatchFacts) {
    for_each_expr(expr, &mut |node| note_prototype_effect(node, facts));
}

/// Record what a single expression node does to some class's prototype.
///
/// Only the node itself is classified — [`for_each_expr`] supplies every node
/// in the tree, including closure bodies.
fn note_prototype_effect(expr: &Expr, facts: &mut ModuleDispatchFacts) {
    match expr {
        // `<Class>.prototype.<m> = fn` (and its aliased `let p = C.prototype`
        // shape) — issue #838's recogniser resolves the class by name.
        Expr::RegisterPrototypeMethod { class_name, .. }
        | Expr::RegisterClassParentDynamic { class_name, .. } => {
            facts.prototype_touched_classes.insert(class_name.clone());
        }
        // Function-classic prototypes are keyed by a synthetic class id derived
        // from the closure value, and `new <func>()` lowers to `NewDynamic`, so
        // these cannot rewrite a declared class's table. `SetFunctionPrototype`
        // installs a whole prototype object for such a function — same story.
        Expr::RegisterFunctionPrototypeMethod { .. } | Expr::SetFunctionPrototype { .. } => {}
        // Any expression that so much as NAMES a prototype object: the value
        // can be aliased into a local and written through later.
        Expr::PropertyGet { object, property } if is_prototype_key(property) => {
            note_prototype_holder(object, facts);
        }
        Expr::PropertySet {
            object, property, ..
        }
        | Expr::PropertyUpdate {
            object, property, ..
        } if is_prototype_key(property) => {
            note_prototype_holder(object, facts);
        }
        Expr::IndexGet { object, index } | Expr::IndexSet { object, index, .. } => {
            if matches!(index.as_ref(), Expr::String(key) if is_prototype_key(key)) {
                note_prototype_holder(object, facts);
            }
        }
        Expr::PutValueSet { target, key, .. } => {
            if matches!(key.as_ref(), Expr::String(k) if is_prototype_key(k)) {
                note_prototype_holder(target, facts);
            }
        }
        _ => {}
    }
}

/// Attribute a prototype-holding expression to the class that owns it.
fn note_prototype_holder(object: &Expr, facts: &mut ModuleDispatchFacts) {
    match object {
        Expr::ClassRef(name) => {
            facts.prototype_touched_classes.insert(name.clone());
        }
        // `function F() {}; F.prototype.m = …` — not a declared class (see the
        // `RegisterFunctionPrototypeMethod` arm above).
        Expr::FuncRef(_) => {}
        // Anything else (`k.prototype`, `x.constructor.prototype`, a local
        // holding a class value, …) cannot be pinned to a class name.
        _ => facts.opaque_prototype_mutation = true,
    }
}

fn is_prototype_key(key: &str) -> bool {
    key == "prototype" || key == "__proto__"
}

/// Property names written directly on each scalar-replacement candidate.
///
/// Writes through an alias, a computed key, or a call already escape the
/// candidate in `check_escapes_in_expr`; this only has to catch the writes that
/// the escape check deliberately treats as plain field stores.
fn collect_candidate_property_writes(
    stmts: &[Stmt],
    candidates: &HashMap<u32, String>,
) -> HashMap<u32, HashSet<String>> {
    let mut writes: HashMap<u32, HashSet<String>> = HashMap::new();
    let mut record = |object: &Expr, property: &str| {
        if let Expr::LocalGet(id) = object {
            if candidates.contains_key(id) {
                writes.entry(*id).or_default().insert(property.to_string());
            }
        }
    };
    for_each_expr_in_stmts(stmts, &mut |expr| match expr {
        Expr::PropertySet {
            object, property, ..
        }
        | Expr::PropertyUpdate {
            object, property, ..
        } => record(object, property),
        Expr::PutValueSet { target, key, .. } => {
            if let Expr::String(property) = key.as_ref() {
                record(target, property);
            }
        }
        Expr::IndexSet {
            object,
            index,
            value: _,
        } => {
            if let Expr::String(property) = index.as_ref() {
                record(object, property);
            }
        }
        _ => {}
    });
    writes
}

/// Escape every candidate whose summarized method call could dispatch to
/// something other than the class method the summary would inline.
///
/// This is the guard #5466 was missing (#5872). It runs after the main escape
/// walk and can only *add* to `escaped`, so a receiver it rejects falls back to
/// the ordinary heap-allocate + dispatch path — which observes the own-property
/// shadow / mutated prototype exactly like Node does.
pub fn mark_unstable_scalar_method_receivers(
    stmts: &[Stmt],
    candidates: &HashMap<u32, String>,
    classes: &HashMap<String, &Class>,
    module: &ModuleDispatchFacts,
    escaped: &mut HashSet<u32>,
) {
    if candidates.is_empty() {
        return;
    }
    let writes = collect_candidate_property_writes(stmts, candidates);

    for_each_expr_in_stmts(stmts, &mut |expr| {
        let Expr::Call { callee, args, .. } = expr else {
            return;
        };
        let Expr::PropertyGet { object, property } = callee.as_ref() else {
            return;
        };
        let Expr::LocalGet(id) = object.as_ref() else {
            return;
        };
        let Some(class_name) = candidates.get(id) else {
            return;
        };
        if escaped.contains(id) {
            return;
        }
        // Only summarized calls keep the receiver scalar-replaced; every other
        // method call already escapes it in `check_escapes_in_expr`.
        if super::simple_scalar_method_summary(classes, class_name, property, args.len()).is_none()
        {
            return;
        }
        let own_write_shadows_method = writes
            .get(id)
            .is_some_and(|written| written.contains(property));
        if own_write_shadows_method || !module.prototype_is_stable(classes, class_name) {
            escaped.insert(*id);
        }
    });
}

// ── Generic HIR walking ────────────────────────────────────────────────────
//
// `walk_expr_children` only yields an expression's *direct* children and does
// not descend into closure bodies (they are `Vec<Stmt>`), so both are wired up
// here.

fn for_each_expr(expr: &Expr, f: &mut dyn FnMut(&Expr)) {
    f(expr);
    perry_hir::walker::walk_expr_children(expr, &mut |child| for_each_expr(child, f));
    if let Expr::Closure { body, .. } = expr {
        for_each_expr_in_stmts(body, f);
    }
}

fn for_each_expr_in_stmts(stmts: &[Stmt], f: &mut dyn FnMut(&Expr)) {
    for stmt in stmts {
        for_each_expr_in_stmt(stmt, f);
    }
}

fn for_each_expr_in_stmt(stmt: &Stmt, f: &mut dyn FnMut(&Expr)) {
    match stmt {
        Stmt::Expr(expr) | Stmt::Throw(expr) => for_each_expr(expr, f),
        Stmt::Return(Some(expr)) => for_each_expr(expr, f),
        Stmt::Let {
            init: Some(expr), ..
        } => for_each_expr(expr, f),
        Stmt::If {
            condition,
            then_branch,
            else_branch,
        } => {
            for_each_expr(condition, f);
            for_each_expr_in_stmts(then_branch, f);
            if let Some(branch) = else_branch {
                for_each_expr_in_stmts(branch, f);
            }
        }
        Stmt::While { condition, body } | Stmt::DoWhile { condition, body } => {
            for_each_expr(condition, f);
            for_each_expr_in_stmts(body, f);
        }
        Stmt::For {
            init,
            condition,
            update,
            body,
        } => {
            if let Some(init) = init {
                for_each_expr_in_stmt(init, f);
            }
            if let Some(condition) = condition {
                for_each_expr(condition, f);
            }
            if let Some(update) = update {
                for_each_expr(update, f);
            }
            for_each_expr_in_stmts(body, f);
        }
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            for_each_expr_in_stmts(body, f);
            if let Some(catch) = catch {
                for_each_expr_in_stmts(&catch.body, f);
            }
            if let Some(finally) = finally {
                for_each_expr_in_stmts(finally, f);
            }
        }
        Stmt::Switch {
            discriminant,
            cases,
        } => {
            for_each_expr(discriminant, f);
            for case in cases {
                if let Some(test) = &case.test {
                    for_each_expr(test, f);
                }
                for_each_expr_in_stmts(&case.body, f);
            }
        }
        Stmt::Labeled { body, .. } => for_each_expr_in_stmt(body, f),
        Stmt::Return(None)
        | Stmt::Let { init: None, .. }
        | Stmt::Break
        | Stmt::Continue
        | Stmt::LabeledBreak(_)
        | Stmt::LabeledContinue(_)
        | Stmt::PreallocateBoxes(_)
        | Stmt::PreallocateTdzBoxes(_) => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use perry_hir::{ClassField, Function};
    use perry_types::Type;

    const RECEIVER: u32 = 1;

    /// `class C { value = 14; getValue(): number { return this.value; } }` —
    /// the exact shape `simple_scalar_method_summary` accepts.
    fn summarizable_class(name: &str) -> Class {
        Class {
            id: 1,
            name: name.to_string(),
            type_params: Vec::new(),
            extends: None,
            extends_name: None,
            native_extends: None,
            extends_expr: None,
            heritage_lexically_shadowed: false,
            fields: vec![ClassField {
                name: "value".to_string(),
                key_expr: None,
                ty: Type::Number,
                init: Some(Expr::Number(14.0)),
                is_private: false,
                is_readonly: false,
                decorators: Vec::new(),
            }],
            constructor: None,
            methods: vec![Function {
                id: 2,
                name: "getValue".to_string(),
                type_params: Vec::new(),
                params: Vec::new(),
                return_type: Type::Number,
                body: vec![Stmt::Return(Some(Expr::PropertyGet {
                    object: Box::new(Expr::This),
                    property: "value".to_string(),
                }))],
                is_async: false,
                is_generator: false,
                is_strict: false,
                is_exported: false,
                captures: Vec::new(),
                decorators: Vec::new(),
                was_plain_async: false,
                was_unrolled: false,
            }],
            getters: Vec::new(),
            setters: Vec::new(),
            static_accessor_names: Vec::new(),
            static_accessor_fn_ids: Vec::new(),
            static_fields: Vec::new(),
            static_methods: Vec::new(),
            computed_members: Vec::new(),
            decorators: Vec::new(),
            is_exported: false,
            is_nested: false,
            aliases: Vec::new(),
        }
    }

    fn new_receiver_stmt(class_name: &str) -> Stmt {
        Stmt::Let {
            id: RECEIVER,
            name: "obj".to_string(),
            ty: Type::Named(class_name.to_string()),
            mutable: false,
            init: Some(Expr::New {
                class_name: class_name.to_string(),
                args: Vec::new(),
                type_args: Vec::new(),
                byte_offset: 0,
                cap_args_appended: 0,
            }),
        }
    }

    fn call_method_stmt(method: &str) -> Stmt {
        Stmt::Return(Some(Expr::Call {
            callee: Box::new(Expr::PropertyGet {
                object: Box::new(Expr::LocalGet(RECEIVER)),
                property: method.to_string(),
            }),
            args: Vec::new(),
            type_args: Vec::new(),
            byte_offset: 0,
        }))
    }

    /// `(obj as any).<key> = () => 99` — lowers to `PutValueSet` since #4126.
    fn own_write_stmt(key: &str) -> Stmt {
        Stmt::Expr(Expr::PutValueSet {
            target: Box::new(Expr::LocalGet(RECEIVER)),
            key: Box::new(Expr::String(key.to_string())),
            value: Box::new(Expr::Number(99.0)),
            receiver: Box::new(Expr::LocalGet(RECEIVER)),
            strict: false,
        })
    }

    fn escaped_receivers(
        stmts: &[Stmt],
        class: &Class,
        facts: &ModuleDispatchFacts,
    ) -> HashSet<u32> {
        let classes = HashMap::from([(class.name.clone(), class)]);
        let candidates = HashMap::from([(RECEIVER, class.name.clone())]);
        let mut escaped = HashSet::new();
        mark_unstable_scalar_method_receivers(stmts, &candidates, &classes, facts, &mut escaped);
        escaped
    }

    fn stable_facts() -> ModuleDispatchFacts {
        ModuleDispatchFacts {
            prototype_touched_classes: HashSet::new(),
            opaque_prototype_mutation: false,
        }
    }

    #[test]
    fn summarized_receiver_stays_scalar_replaced_when_lookup_is_stable() {
        let class = summarizable_class("C");
        let stmts = vec![new_receiver_stmt("C"), call_method_stmt("getValue")];
        assert!(escaped_receivers(&stmts, &class, &stable_facts()).is_empty());
    }

    /// #5872: `const obj = new C(); (obj as any).getValue = () => 99;
    /// return obj.getValue();` must NOT fold to the field value.
    #[test]
    fn own_property_write_shadowing_the_method_escapes_the_receiver() {
        let class = summarizable_class("C");
        let stmts = vec![
            new_receiver_stmt("C"),
            own_write_stmt("getValue"),
            call_method_stmt("getValue"),
        ];
        assert!(escaped_receivers(&stmts, &class, &stable_facts()).contains(&RECEIVER));
    }

    /// A write to a *different* own property is a plain field store and must
    /// keep the receiver scalar-replaced.
    #[test]
    fn own_property_write_to_another_name_keeps_scalar_replacement() {
        let class = summarizable_class("C");
        let stmts = vec![
            new_receiver_stmt("C"),
            own_write_stmt("value"),
            call_method_stmt("getValue"),
        ];
        assert!(escaped_receivers(&stmts, &class, &stable_facts()).is_empty());
    }

    /// The shadowing write can be nested anywhere in the body (#5872's
    /// `loopMutationReceiverMethod`).
    #[test]
    fn own_property_write_inside_a_loop_escapes_the_receiver() {
        let class = summarizable_class("C");
        let stmts = vec![
            new_receiver_stmt("C"),
            Stmt::While {
                condition: Expr::Bool(true),
                body: vec![own_write_stmt("getValue")],
            },
            call_method_stmt("getValue"),
        ];
        assert!(escaped_receivers(&stmts, &class, &stable_facts()).contains(&RECEIVER));
    }

    /// `C.prototype.getValue = fn` in an unrelated function — invisible to a
    /// per-function walk, which is why the fact set is module-scoped.
    #[test]
    fn prototype_mutation_anywhere_in_the_module_escapes_the_receiver() {
        let class = summarizable_class("C");
        let mut module = Module::new("m.ts");
        module.functions.push(Function {
            id: 9,
            name: "mutate".to_string(),
            type_params: Vec::new(),
            params: Vec::new(),
            return_type: Type::Void,
            body: vec![Stmt::Expr(Expr::RegisterPrototypeMethod {
                class_name: "C".to_string(),
                method_name: "getValue".to_string(),
                value: Box::new(Expr::Number(114.0)),
            })],
            is_async: false,
            is_generator: false,
            is_strict: false,
            is_exported: false,
            captures: Vec::new(),
            decorators: Vec::new(),
            was_plain_async: false,
            was_unrolled: false,
        });
        module.classes.push(class.clone());

        let facts = collect_module_dispatch_facts(&module);
        let classes = HashMap::from([(class.name.clone(), &class)]);
        assert!(!facts.prototype_is_stable(&classes, "C"));

        let stmts = vec![new_receiver_stmt("C"), call_method_stmt("getValue")];
        assert!(escaped_receivers(&stmts, &class, &facts).contains(&RECEIVER));
    }

    /// Merely NAMING a prototype is enough — `Object.defineProperty(C.prototype,
    /// …)` and `let p = C.prototype; p.m = fn` both go through a `PropertyGet`.
    #[test]
    fn naming_a_class_prototype_marks_it_unstable() {
        let class = summarizable_class("C");
        let mut module = Module::new("m.ts");
        module.init.push(Stmt::Expr(Expr::ObjectDefineProperty(
            Box::new(Expr::PropertyGet {
                object: Box::new(Expr::ClassRef("C".to_string())),
                property: "prototype".to_string(),
            }),
            Box::new(Expr::String("getValue".to_string())),
            Box::new(Expr::Undefined),
        )));
        module.classes.push(class.clone());

        let facts = collect_module_dispatch_facts(&module);
        let classes = HashMap::from([(class.name.clone(), &class)]);
        assert!(!facts.prototype_is_stable(&classes, "C"));
    }

    /// A prototype named through something other than a class ref can't be
    /// attributed, so nothing in the module may be summarized.
    #[test]
    fn unattributable_prototype_access_marks_the_module_opaque() {
        let class = summarizable_class("C");
        let mut module = Module::new("m.ts");
        module.init.push(Stmt::Let {
            id: 7,
            name: "p".to_string(),
            ty: Type::Any,
            mutable: false,
            init: Some(Expr::PropertyGet {
                object: Box::new(Expr::LocalGet(6)),
                property: "prototype".to_string(),
            }),
        });
        module.classes.push(class.clone());

        let facts = collect_module_dispatch_facts(&module);
        let classes = HashMap::from([(class.name.clone(), &class)]);
        assert!(!facts.prototype_is_stable(&classes, "C"));
    }

    /// An untouched class in a module that mutates a *different* prototype
    /// keeps its fast path.
    #[test]
    fn unrelated_prototype_mutation_leaves_other_classes_stable() {
        let class = summarizable_class("C");
        let mut module = Module::new("m.ts");
        module.init.push(Stmt::Expr(Expr::RegisterPrototypeMethod {
            class_name: "Other".to_string(),
            method_name: "getValue".to_string(),
            value: Box::new(Expr::Number(1.0)),
        }));
        module.classes.push(class.clone());

        let facts = collect_module_dispatch_facts(&module);
        let classes = HashMap::from([(class.name.clone(), &class)]);
        assert!(facts.prototype_is_stable(&classes, "C"));
    }

    #[test]
    fn default_facts_are_conservative() {
        let class = summarizable_class("C");
        let classes = HashMap::from([(class.name.clone(), &class)]);
        assert!(!ModuleDispatchFacts::default().prototype_is_stable(&classes, "C"));
    }
}
