//! Value-flow heritage, including late-arriving and unknown parents.

use crate::{compute, FactsConfig, ProgramFacts};

fn facts(src: &str) -> ProgramFacts {
    let ast = perry_parser::parse_typescript(src, "heritage.ts").unwrap();
    let hir = perry_hir::lower_module(&ast, "heritage", "heritage.ts").unwrap();
    compute(&[("heritage.ts".into(), &hir)], &FactsConfig::default())
}

fn child<'a>(pf: &'a ProgramFacts, name: &str) -> &'a crate::TemplateFacts {
    pf.templates
        .iter()
        .find(|t| {
            t.display.ends_with(&format!(":{name}"))
                || t.display.contains(&format!(":{name}__class_expr_"))
        })
        .unwrap_or_else(|| panic!("missing {name}: {}", pf.dump_text()))
}

#[test]
fn fresh_parent_runs_constructor_and_inherited_method_without_escape() {
    let pf = facts(
        r#"
        class Base { constructor(v) { this.value = v; } read() { return this.value; } }
        function make(Parent) { return class Child extends Parent { static marker = 1; }; }
        const factory = make; const C = factory(Base); const c = new C(7); console.log(c.read());
    "#,
    );
    let c = child(&pf, "Child");
    assert!(c.escape.is_none(), "{}", pf.dump_text());
    assert!(c.hard.is_none(), "{}", pf.dump_text());
    let value = c.fields.iter().find(|f| f.key == "value").unwrap();
    assert!(value.kind.desc().contains("num"), "{}", pf.dump_text());
    assert!(
        pf.sites
            .iter()
            .any(|s| s.key == "read" && s.call_target.is_some()),
        "{}",
        pf.dump_text()
    );
    assert!(
        !c.fixed_layout && c.fields.iter().all(|f| f.slot.is_none()),
        "{}",
        pf.dump_text()
    );
}

#[test]
fn conditional_parents_union_constructor_fields_and_methods() {
    let pf = facts(
        r#"
        class A { constructor() { this.a = 1; } read() { return this.a; } }
        class B { constructor() { this.b = "b"; } read() { return this.b; } }
        function make(cond) { return class Child extends (cond ? A : B) { static marker = 1; }; }
        const factory = make; const C = factory(true); console.log(new C().read());
    "#,
    );
    let c = child(&pf, "Child");
    assert!(c.escape.is_none() && c.hard.is_none(), "{}", pf.dump_text());
    assert!(c.fields.iter().any(|f| f.key == "a"), "{}", pf.dump_text());
    assert!(c.fields.iter().any(|f| f.key == "b"), "{}", pf.dump_text());
    assert!(
        c.fields.iter().all(|f| f.slot.is_none()),
        "{}",
        pf.dump_text()
    );
    assert!(
        pf.sites
            .iter()
            .filter(|s| s.key == "read")
            .all(|s| s.call_target.is_none()),
        "{}",
        pf.dump_text()
    );
}

#[test]
fn mixin_return_value_keeps_the_known_grandparent() {
    let pf = facts(
        r#"
        class Base { value = 1; read() { return this.value; } }
        function mixin(Parent) { return class Mixed extends Parent { static x = 1; }; }
        function make() { return class Child extends mixin(Base) { static y = 1; }; }
        const factory = make; const C = factory(); console.log(new C().read());
    "#,
    );
    let c = child(&pf, "Child");
    assert!(c.escape.is_none() && c.hard.is_none(), "{}", pf.dump_text());
    assert!(
        c.fields.iter().any(|f| f.key == "value"),
        "{}",
        pf.dump_text()
    );
}

#[test]
fn unknown_parent_keeps_e11_and_escapes_constructor_arguments() {
    let pf = facts(
        r#"
        class Arg { value = 1; }
        function make() { return class Child extends unknownFactory() { static x = 1; }; }
        const factory = make; const C = factory(); new C(new Arg());
    "#,
    );
    let c = child(&pf, "Child");
    assert!(
        c.escape.as_ref().is_some_and(|s| s.contains("E11")),
        "{}",
        pf.dump_text()
    );
    assert!(
        c.hard.as_ref().is_some_and(|s| s.starts_with("E11")),
        "{}",
        pf.dump_text()
    );
    assert!(child(&pf, "Arg").escape.is_some(), "{}", pf.dump_text());
}

#[test]
fn heritage_side_effect_is_preserved_without_an_extra_metadata_evaluation() {
    let ast = perry_parser::parse_typescript(
        r#"
        class Base { read() { return 1; } }
        function parent() { console.log("parent"); return Base; }
        function make() { return class Child extends parent() { static x = 1; }; }
        const factory = make; const C = factory(); new C().read();
    "#,
        "heritage.ts",
    )
    .unwrap();
    let hir = perry_hir::lower_module(&ast, "heritage", "heritage.ts").unwrap();
    let mut g = crate::gen::Gen::new(vec![("heritage.ts", &hir)], false, false);
    g.register_all();
    g.link_exports();
    g.walk_all();
    g.m.solve();
    let parent =
        g.m.funcs
            .iter()
            .position(|f| f.name.ends_with(":parent"))
            .unwrap();
    let token = g.m.funcs[parent].token;
    let parent_calls =
        g.m.nodes
            .iter()
            .flat_map(|n| &n.cons)
            .filter(|c| matches!(c, crate::model::Cons::Call { .. }))
            .count();
    assert!(parent_calls > 0);
    // Exactly one source expression node names the heritage call's function.
    let calls =
        g.m.nodes
            .iter()
            .filter(|n| n.pts.contains_tok(token))
            .flat_map(|n| &n.cons)
            .filter(|c| matches!(c, crate::model::Cons::Call { .. }))
            .count();
    assert_eq!(calls, 1, "heritage evaluated once in the constraint graph");
}

#[test]
fn null_parent_does_not_escape_or_gain_object_prototype() {
    let pf = facts(
        r#"
        function make() { return class Child extends null { static x = 1; constructor() { return {value: 1}; } }; }
        const factory = make; const C = factory(); console.log(new C().value);
    "#,
    );
    let c = child(&pf, "Child");
    assert!(c.escape.is_none() && c.hard.is_none(), "{}", pf.dump_text());
}

#[test]
fn evaluated_super_method_binds_the_child_receiver() {
    let pf = facts(
        r#"
        class Base { read() { return this.value; } }
        function make(Parent) { return class Child extends Parent {
            static x = 1; value = 7; read() { return super.read(); }
        }; }
        const factory = make; const C = factory(Base); console.log(new C().read());
    "#,
    );
    let c = child(&pf, "Child");
    assert!(c.escape.is_none() && c.hard.is_none(), "{}", pf.dump_text());
    assert!(
        pf.sites.iter().any(|s| s.key == "value"
            && s.receivers
                .iter()
                .any(|r| r.contains(":Child__class_expr_"))),
        "{}",
        pf.dump_text()
    );
}

#[test]
fn known_parent_prototype_mutation_still_refuses_proofs() {
    let pf = facts(
        r#"
        class Base { read() { return 1; } }
        function make(Parent) { return class Child extends Parent { static x = 1; }; }
        const factory = make; const C = factory(Base);
        Object.setPrototypeOf(Base.prototype, unknownPrototype());
        console.log(new C().read());
    "#,
    );
    assert!(!child(&pf, "Child").prefix_stable, "{}", pf.dump_text());
}

#[test]
fn imported_and_reexported_parent_uses_existing_value_graph() {
    fn module(path: &str, source: &str) -> perry_hir::Module {
        let ast = perry_parser::parse_typescript(source, path).unwrap();
        let mut hir = perry_hir::lower_module(&ast, path, path).unwrap();
        for import in &mut hir.imports {
            import.resolved_path = Some(import.source.clone());
        }
        hir
    }
    let base = module(
        "/base.ts",
        "export class Base { value = 1; read() { return this.value; } }",
    );
    let bridge = module("/bridge.ts", "export {Base as Parent} from '/base.ts';");
    let leaf = module("/leaf.ts", "import {Parent} from '/bridge.ts'; function make() { return class Child extends Parent { static x = 1; }; } const factory = make; const C = factory(); console.log(new C().read());");
    // Re-exports have no HIR Import row. Reuse the canonical edge supplied
    // to layout-family parent resolution by the driver (including aliases).
    let cfg = FactsConfig {
        resolved_reexports: [(("/bridge.ts".into(), "/base.ts".into()), "/base.ts".into())].into(),
        ..Default::default()
    };
    let pf = compute(
        &[
            ("/base.ts".into(), &base),
            ("/bridge.ts".into(), &bridge),
            ("/leaf.ts".into(), &leaf),
        ],
        &cfg,
    );
    let c = child(&pf, "Child");
    assert!(c.escape.is_none() && c.hard.is_none(), "{}", pf.dump_text());
    assert!(
        c.fields.iter().any(|f| f.key == "value"),
        "{}",
        pf.dump_text()
    );
}

#[test]
fn a_late_unknown_parent_revokes_the_known_parent_result() {
    use crate::model::{TokKind, TOP};
    let ast = perry_parser::parse_typescript(
        r#"
        class Base { read() { return 1; } }
        function make(Parent) { return class Child extends Parent { static x = 1; }; }
        const factory = make; const C = factory(Base); new C().read();
    "#,
        "heritage.ts",
    )
    .unwrap();
    let hir = perry_hir::lower_module(&ast, "heritage", "heritage.ts").unwrap();
    let mut g = crate::gen::Gen::new(vec![("heritage.ts", &hir)], false, false);
    g.register_all();
    g.link_exports();
    g.walk_all();
    g.m.solve();
    let tm =
        g.m.tmpls
            .iter()
            .position(|t| t.display.contains(":Child__class_expr_"))
            .unwrap() as u32;
    let inst = g.m.tok(TokKind::Obj(tm));
    let parent = g.m.tmpls[tm as usize].evaluated_parent.unwrap();
    assert!(!g.m.is_escaped(inst));
    g.m.add_tok(parent, TOP);
    g.m.solve();
    assert!(g.m.is_escaped(inst));
    assert!(g.m.hard.contains_key(&inst));
}

#[test]
fn repeated_mixin_evaluations_have_a_finite_constructor_fixpoint() {
    let pf = facts(
        r#"
        class Base { value = 1; read() { return this.value; } }
        function wrap(Parent) { return class Wrapped extends Parent { static x = 1; }; }
        let C = Base; C = wrap(C); C = wrap(C); console.log(new C().read());
    "#,
    );
    assert!(!pf.stats.aborted);
    assert!(child(&pf, "Wrapped").escape.is_none(), "{}", pf.dump_text());
}

#[test]
fn specialized_class_site_keeps_heritage_after_metadata_resolution() {
    let ast = perry_parser::parse_typescript(
        r#"
        class Base { value = 1; read() { return this.value; } }
        function make(Parent) { return class Child extends Parent { static x = 1; }; }
        const factory = make; const C = factory(Base); console.log(new C().read());
    "#,
        "heritage.ts",
    )
    .unwrap();
    let mut hir = perry_hir::lower_module(&ast, "heritage", "heritage.ts").unwrap();
    let c = hir
        .classes
        .iter_mut()
        .find(|c| c.name.starts_with("Child__class_expr_"))
        .unwrap();
    c.extends_expr = None;
    c.extends_name = Some("Base".into());
    let pf = compute(&[("heritage.ts".into(), &hir)], &FactsConfig::default());
    let c = child(&pf, "Child");
    assert!(c.escape.is_none() && c.hard.is_none(), "{}", pf.dump_text());
    assert!(
        c.fields.iter().any(|f| f.key == "value"),
        "{}",
        pf.dump_text()
    );
    assert!(
        c.fields.iter().all(|f| f.slot.is_none()),
        "{}",
        pf.dump_text()
    );
}
#[test]
fn class_prototype_reference_stays_fixed_when_the_class_escapes_or_is_written() {
    use crate::model::{Key, TokKind};
    let ast = perry_parser::parse_typescript(
        r#"
        class Base { read() { return 1; } }
        Reflect.set(Base, "prototype", { wrong: true });
        function make(Parent) { return class Child extends Parent { static x = 1; }; }
        const factory = make; const C = factory(Base); new C().read();
    "#,
        "fixed.ts",
    )
    .unwrap();
    let hir = perry_hir::lower_module(&ast, "fixed", "fixed.ts").unwrap();
    let mut g = crate::gen::Gen::new(vec![("fixed.ts", &hir)], false, false);
    g.register_all();
    g.link_exports();
    g.walk_all();
    g.m.solve();
    let base =
        g.m.tmpls
            .iter()
            .position(|t| t.display.ends_with(":Base"))
            .unwrap() as u32;
    let child =
        g.m.tmpls
            .iter()
            .position(|t| t.display.contains(":Child__class_expr_"))
            .unwrap() as u32;
    let class = g.m.tok(TokKind::ClassObj(base));
    let proto = g.m.tok(TokKind::Proto(base));
    let key = g.m.atom("prototype");
    let field = g.m.field(class, key);
    assert!(g.m.fixed_class_prototype(class, Key::Named(key)));
    assert_eq!(g.m.nodes[field as usize].pts.to_sorted(), vec![proto]);
    let reason = g.m.reason("class passed to unknown code");
    g.m.escape(class, reason);
    g.m.solve();
    assert_eq!(g.m.nodes[field as usize].pts.to_sorted(), vec![proto]);
    let child_proto = g.m.tok(TokKind::Proto(child));
    let parents = g.m.proto_node(child_proto);
    assert_eq!(g.m.nodes[parents as usize].pts.to_sorted(), vec![proto]);
}
#[test]
fn evaluated_builtin_parent_keeps_the_existing_native_summary() {
    let pf = facts(
        r#"
        const C = class NativeChild extends Error {
            static marker = 1;
            constructor() { super("message"); this.extra = 1; }
        };
        console.log(new C().message);
    "#,
    );
    let c = child(&pf, "NativeChild");
    assert!(c.escape.is_none() && c.hard.is_none(), "{}", pf.dump_text());
    assert!(
        c.fields.iter().any(|f| f.key == "message"),
        "{}",
        pf.dump_text()
    );
}

#[test]
fn shadowed_native_parent_keeps_the_unknown_boundary() {
    let pf = facts(
        r#"
        function make(Error) { return class ShadowedChild extends Error { static marker = 1; }; }
        const factory = make; const C = factory(unknownParent()); new C();
    "#,
    );
    assert!(
        child(&pf, "ShadowedChild").hard.is_some(),
        "{}",
        pf.dump_text()
    );
}
