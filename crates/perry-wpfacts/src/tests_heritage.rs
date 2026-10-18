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
        function make() { return class NativeChild extends Error {
            static marker = 1;
            constructor() { super("message"); this.extra = 1; }
        }; }
        const factory = make; const C = factory();
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

#[test]
fn callable_projection_keeps_argument_flow_with_noncallable_alternatives() {
    let pf = facts(
        r#"
        class Arg { value = 7; }
        function method(arg) { return arg.value; }
        function choose(flag) { return flag ? method : { notCallable: true }; }
        const holder = { method: choose(true) };
        console.log(holder.method(new Arg()));
    "#,
    );
    let arg = child(&pf, "Arg");
    assert!(
        arg.escape.is_none() && arg.hard.is_none(),
        "{}",
        pf.dump_text()
    );
    assert!(
        pf.sites
            .iter()
            .any(|s| s.key == "value" && s.receivers.iter().any(|r| r.ends_with(":Arg"))),
        "{}",
        pf.dump_text()
    );
}

#[test]
fn method_summary_resumes_after_a_declared_member_is_deleted() {
    use crate::model::{Hard, Key, TokKind};
    let ast = perry_parser::parse_typescript(
        "class Base { read() { return 1; } } class Child extends Base {} function late() { return 2; }",
        "resume.ts",
    ).unwrap();
    let hir = perry_hir::lower_module(&ast, "resume", "resume.ts").unwrap();
    let mut g = crate::gen::Gen::new(vec![("resume.ts", &hir)], false, false);
    g.register_all();
    g.link_exports();
    g.walk_all();
    let base =
        g.m.tmpls
            .iter()
            .position(|t| t.display.ends_with(":Base"))
            .unwrap() as u32;
    let child =
        g.m.tmpls
            .iter()
            .position(|t| t.display.ends_with(":Child"))
            .unwrap() as u32;
    let late =
        g.m.funcs
            .iter()
            .find(|f| f.name.ends_with(":late"))
            .unwrap()
            .token;
    let derived = g.m.tok(TokKind::Proto(child));
    let key = g.m.atom("read");
    let targets = g.m.prototype_method_value(derived, Key::Named(key));
    g.m.solve();
    assert!(!g.m.nodes[targets as usize].pts.is_empty());
    assert!(!g.m.nodes[targets as usize].pts.contains_tok(late));
    let object_proto = g.m.tok(TokKind::ObjectProto);
    let inherited = g.m.field(object_proto, key);
    g.m.add_tok(inherited, late);
    let base_proto = g.m.tok(TokKind::Proto(base));
    let reason = g.m.reason("delete declared method");
    g.m.mark_hard(base_proto, Hard::E7Delete, reason);
    g.m.solve();
    assert!(g.m.nodes[targets as usize].pts.contains_tok(late));
}

#[test]
fn class_capture_indices_keep_registration_refresh_and_writes_separate() {
    use perry_hir::{Expr, Stmt};
    for refresh in [false, true] {
        for snapshot in [false, true] {
            let ast = perry_parser::parse_typescript(
                "class Holder { read() { return 0; } } const o = { value: new Holder().read() }; console.log(o.value);",
                "slots.ts",
            ).unwrap();
            let mut hir = perry_hir::lower_module(&ast, "slots", "slots.ts").unwrap();
            let holder = hir.classes.iter_mut().find(|c| c.name == "Holder").unwrap();
            holder
                .methods
                .iter_mut()
                .find(|m| m.name == "read")
                .unwrap()
                .body = vec![Stmt::Return(Some(if snapshot {
                Expr::ClassCaptureValue {
                    class_name: "Holder".into(),
                    index: 1,
                    fallback: None,
                    prefer_fallback: false,
                }
            } else {
                Expr::ClassEnvGet {
                    class_name: "Holder".into(),
                    index: 1,
                    guarded: true,
                }
            }))];
            let captures = vec![Expr::Number(1.0), Expr::String("right-slot".into())];
            hir.init.push(Stmt::Expr(if refresh {
                Expr::RefreshClassExprCaptures {
                    class_value: Box::new(Expr::ClassRef("Holder".into())),
                    captures,
                    env_class: Some("Holder".into()),
                }
            } else {
                Expr::RegisterClassCaptures {
                    class_name: "Holder".into(),
                    captures,
                }
            }));
            hir.init.push(Stmt::Expr(Expr::ClassEnvSet {
                class_name: "Holder".into(),
                index: 0,
                value: Box::new(Expr::Bool(true)),
                guarded: true,
                publish: false,
            }));
            let pf = compute(&[("slots.ts".into(), &hir)], &FactsConfig::default());
            let value = pf
                .templates
                .iter()
                .flat_map(|t| &t.fields)
                .find(|f| f.key == "value")
                .unwrap();
            assert_eq!(
                value.kind.desc(),
                "str|undef",
                "refresh={refresh} snapshot={snapshot}: {}",
                pf.dump_text()
            );
        }
    }
}

#[test]
fn numeric_capture_cell_read_does_not_union_class_statics() {
    let pf = facts(
        r#"
        class Base { static build() { return new Base(); } read() { return 1; } }
        function make(flag) {
            const captured = flag ? [Base] : Base;
            const Parent = captured[0];
            return class Child extends Parent { static marker = 1; };
        }
        const factory = make; const C = factory(true); console.log(new C().read());
    "#,
    );
    let c = child(&pf, "Child");
    assert!(c.escape.is_none() && c.hard.is_none(), "{}", pf.dump_text());
    assert!(
        pf.sites
            .iter()
            .any(|s| s.key == "read" && s.call_target.is_some()),
        "{}",
        pf.dump_text()
    );
}

#[test]
fn numeric_literal_read_keeps_other_object_properties_out() {
    let pf = facts("const o = { 0: 7, other: 'wrong' }; const result = { value: o[0] }; console.log(result.value);");
    let value = pf
        .templates
        .iter()
        .flat_map(|t| &t.fields)
        .find(|f| f.key == "value")
        .unwrap();
    assert_eq!(value.kind.desc(), "num|undef", "{}", pf.dump_text());
}

#[test]
fn numeric_array_call_uses_its_stored_function_without_an_intrinsic_fallback() {
    let pf = facts(
        r#"
        class Arg { value = 7; }
        function method(arg) { return arg.value; }
        const actions = [method]; console.log(actions[0](new Arg()));
    "#,
    );
    let arg = child(&pf, "Arg");
    assert!(
        arg.escape.is_none() && arg.hard.is_none(),
        "{}",
        pf.dump_text()
    );
    assert!(
        pf.sites
            .iter()
            .any(|s| s.key == "value" && s.receivers.iter().any(|r| r.ends_with(":Arg"))),
        "{}",
        pf.dump_text()
    );
}

#[test]
fn a_nonconstructor_heritage_alternative_throws_without_escaping_the_valid_parent() {
    let pf = facts(
        r#"
        class Base { read() { return 1; } }
        function make(flag) {
            return class Child extends (flag ? Base : { notConstructor: true }) { static marker = 1; };
        }
        const factory = make; const C = factory(true); console.log(new C().read());
    "#,
    );
    for name in ["Base", "Child"] {
        let t = child(&pf, name);
        assert!(t.escape.is_none() && t.hard.is_none(), "{}", pf.dump_text());
    }
}

#[test]
fn symbol_read_does_not_name_string_fields_or_builtin_constructors() {
    let pf = facts(
        r#"
        const key = Symbol("key");
        const o = { other: "wrong" };
        o[key] = 7;
        const result = { value: o[key] };
        console.log(result.value);
    "#,
    );
    let value = pf
        .templates
        .iter()
        .flat_map(|t| &t.fields)
        .find(|f| f.key == "value")
        .unwrap();
    assert_eq!(value.kind.desc(), "num|undef", "{}", pf.dump_text());
}

#[test]
fn mixed_symbol_and_string_index_retains_full_lookup() {
    let pf = facts(
        r#"
        function read(o, key) { return o[key]; }
        const key = Symbol("key");
        const o = { other: "string" }; o[key] = 7;
        const result = { value: read(o, key), also: read(o, "other") };
        console.log(result.value, result.also);
    "#,
    );
    let value = pf
        .templates
        .iter()
        .flat_map(|t| &t.fields)
        .find(|f| f.key == "value")
        .unwrap();
    let desc = value.kind.desc();
    assert!(
        desc.contains("num") && desc.contains("str"),
        "{}",
        pf.dump_text()
    );
}

#[test]
fn unread_field_aggregate_never_widens_or_escapes_its_members() {
    use crate::{gen::Gen, model::*};
    let mut g = Gen::new(vec![], false, false);
    let m = &mut g.m;
    m.widen_k = 4;
    let holder = m.tok(TokKind::ModNs(0));
    let mut values = Vec::new();
    for i in 1..=8 {
        let key = m.atom(&format!("key{i}"));
        let value = m.tok(TokKind::ModNs(i));
        values.push(value);
        let field = m.field(holder, key);
        m.add_tok(field, value);
    }
    assert!(!m.all_fields.contains_key(&holder));
    let all = m.all_fields(holder);
    let late_key = m.atom("late");
    let late_value = m.tok(TokKind::ModNs(9));
    let late = m.field(holder, late_key);
    m.add_tok(late, late_value);
    values.push(late_value);
    m.solve();
    assert!(!m.nodes[all as usize].widened);
    assert!(values
        .iter()
        .all(|&v| m.nodes[all as usize].pts.contains_tok(v)));
    assert!(values.iter().all(|&v| !m.is_escaped(v)));
}

#[test]
fn consumed_field_aggregate_still_widens_at_the_program_read() {
    use crate::{gen::Gen, model::*};
    let mut g = Gen::new(vec![], false, false);
    let m = &mut g.m;
    m.widen_k = 4;
    let holder = m.tok(TokKind::ModNs(0));
    let mut values = Vec::new();
    for i in 1..=8 {
        let key = m.atom(&format!("key{i}"));
        let value = m.tok(TokKind::ModNs(i));
        values.push(value);
        let field = m.field(holder, key);
        m.add_tok(field, value);
    }
    let all = m.all_fields(holder);
    let result = m.node_l("program-read");
    m.edge(all, result);
    m.solve();
    assert!(!m.nodes[all as usize].widened);
    assert!(m.nodes[result as usize].widened);
    assert!(values.iter().all(|&v| m.is_escaped(v)));
}
