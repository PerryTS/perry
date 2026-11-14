//! A packet's lexical ports and direct seeds precede child admission.
use crate::gen::Gen;
use crate::model::{Act, Cons, Key, Model, TokKind};

struct Probe {
    bytes: String,
    cells: Vec<crate::summary::packet::Cell>,
}

// The packet emitter mirrors legacy publication; red.patch retains the base adapter.
fn probe(src: &str, delay_children: bool) -> Probe {
    let ast = perry_parser::parse_typescript(src, "t.ts").unwrap();
    let hir = perry_hir::lower_module(&ast, "t", "t.ts").unwrap();
    let id = hir.functions.iter().find(|f| f.name == "make").unwrap().id;
    let mut g = Gen::new(vec![("t.ts", &hir)], false, false);
    g.m.packets_enabled = true;
    g.register_all();
    let f = g.mods[0].funcs[&id];
    if delay_children {
        g.m.act = Some(Act::default());
        g.m.activate(f, "call");
    }
    g.walk_all();
    let packet = &g.m.funcs[f as usize].body_packet;
    assert!(packet.ready(), "creator emitted");
    Probe {
        bytes: packet.bytes(),
        cells: packet.cells.clone(),
    }
}

#[test]
fn late_child_keeps_creator_transfer_immutable() {
    let src = "function make(x: any) { return () => x; }";
    let early = probe(src, false);
    let late = probe(src, true);
    assert_eq!(
        early.bytes, late.bytes,
        "child admission changed the creator's immutable transfer"
    );
}

#[test]
fn late_capture_uses_cells_before_child_demand() {
    let src = r#"function make(x: any) {
        let cell = x;
        const read = () => cell;
        const write = (value: any) => { cell = value; };
        write(x);
        return {read, write};
    }"#;
    let early = probe(src, false);
    let late = probe(src, true);
    assert!(!early.cells.is_empty(), "capture control must execute");
    assert_eq!(
        early.cells, late.cells,
        "lexical cell identity depends on the child's demanded ports"
    );
}

#[test]
fn shared_rest_child_keeps_creator_transfer_immutable() {
    let src = r#"function make(x: any) {
        let cell = x;
        return (...values: any[]) => { cell = values[0]; return cell; };
    }"#;
    let early = probe(src, false);
    let late = probe(src, true);
    assert_eq!(
        early.bytes, late.bytes,
        "a shared rest child reclassified the sealed creator transfer"
    );
}

// Inert mode intercepts generation before the legacy node publisher runs.
fn begin(m: &mut Model, f: u32) {
    m.packets_enabled = true;
    m.packets_inert = true;
    assert!(m.packet_body(f).is_none());
}

#[test]
fn direct_seeds_and_store_stay_inert_until_seal() {
    let mut m = super::marker_model();
    let param = m.node_l("incoming binding");
    let constant = m.node_l("direct constant");
    let result = m.funcs[0].ret;
    let environment = m.node_l("lexical environment");
    let escaped = m.node_l("live return effect");
    let object = m.tok(TokKind::ModNs(110));
    let direct = m.tok(TokKind::ModNs(111));
    let foreign = m.tok(TokKind::ModNs(112));
    let key = m.atom("stored");
    let field = m.field(object, key);
    m.add_tok(param, foreign);
    m.add_tok(environment, object);
    begin(&mut m, 0);
    m.add_tok(constant, direct);
    m.edge(param, result);
    m.edge(constant, result);
    m.edge(result, escaped);
    m.cons(
        environment,
        Cons::Store {
            key: Key::Named(key),
            src: constant,
            site: 0,
        },
    );
    assert!(
        m.nodes[escaped as usize].pts.is_empty(),
        "body return escaped before packet seal (foreign incoming seed)"
    );
    assert!(
        m.nodes[field as usize].pts.is_empty(),
        "body store executed before packet seal"
    );
    m.packet_end(None, true);
    let packet = m.funcs[0].body_packet.clone();
    assert!(
        !packet.emissions.iter().any(|op| matches!(op,
        crate::summary::packet::Emission::Seed(_, ts, _) if ts.contains(&foreign))),
        "incoming values cannot become direct body seeds"
    );
    packet.publish(&mut m);
    m.solve();
    assert_eq!(m.nodes[field as usize].pts.to_sorted(), vec![direct]);
    assert!(m.nodes[escaped as usize].pts.contains_tok(foreign));
    assert!(m.nodes[escaped as usize].pts.contains_tok(direct));
}

#[test]
fn forward_capture_has_the_declaration_owner_before_child_admission() {
    let src = "function make(x: any) { const read = () => cell; let cell = x; return read; }";
    let early = probe(src, false);
    let late = probe(src, true);
    assert_eq!(
        early.cells, late.cells,
        "first reader cannot choose lexical owner"
    );
    assert_eq!(
        early.bytes, late.bytes,
        "forward capture changed creator transfer"
    );
}

#[test]
fn child_births_and_keys_do_not_renumber_creator_ports() {
    let src = r#"function make(x: any) {
        const child = () => { const another = () => x.foreign; return {alien: another}; };
        return {own: x, child};
    }"#;
    assert_eq!(probe(src, false).bytes, probe(src, true).bytes);
}

#[test]
fn direct_escape_and_activation_wait_for_publication() {
    let mut m = super::marker_model();
    m.act = Some(Act::default());
    let token = m.funcs[0].token;
    let reason = m.reason("packet escape sentinel");
    begin(&mut m, 0);
    m.activate(0, "call");
    m.escape(token, reason);
    assert!(m.act.as_ref().unwrap().on.is_empty());
    assert!(!m.is_escaped(token));
    m.packet_end(None, true);
    m.funcs[0].body_packet.clone().publish(&mut m);
    assert!(!m.act.as_ref().unwrap().on.is_empty());
    assert!(m.is_escaped(token));
}

fn inspect(src: &str, check: impl FnOnce(&Gen<'_>)) {
    let ast = perry_parser::parse_typescript(src, "t.ts").unwrap();
    let hir = perry_hir::lower_module(&ast, "t", "t.ts").unwrap();
    let mut g = Gen::new(vec![("t.ts", &hir)], false, false);
    g.m.packets_enabled = true;
    g.register_all();
    g.walk_all();
    check(&g);
}

#[test]
fn arguments_and_rest_have_explicit_shared_entry_ports() {
    use crate::summary::packet::SharedEntry;
    inspect(
        "function make(x:any, ...rest:any[]) {return [arguments[0], rest[0], x];}",
        |g| {
            let packet =
                &g.m.funcs
                    .iter()
                    .find(|f| f.name.ends_with(":make"))
                    .unwrap()
                    .body_packet;
            assert!(packet.ready());
            assert!(packet
                .header
                .shared
                .iter()
                .any(|p| matches!(p, SharedEntry::Rest { start: 1, .. })));
            assert!(packet
                .header
                .shared
                .iter()
                .any(|p| matches!(p, SharedEntry::Arguments { .. })));
            assert!(!packet.formals.is_empty());
        },
    );
}

#[test]
fn computed_accessors_and_synthetic_bodies_have_owned_interfaces() {
    use crate::summary::packet::SharedEntry;
    inspect("const k = (globalThis as any).key; class C { x=1; static s=2; get [k]() { return this.x; } } function make(x:any) { return new C(); }",|g| {
        assert!(g.m.funcs.iter().any(|f| f.body_packet.header.shared.iter()
            .any(|p| matches!(p,SharedEntry::ComputedAccessor {..}))));
        for f in &g.m.funcs {
            if f.func_id != u32::MAX {
                assert!(f.body_packet.ready(),"unsealed source body {}",f.name);
            }
        }
        for &f in g.field_init_fn.values().chain(g.static_init_fn.values()).chain(g.init_fn_of.values()) {
            assert!(g.m.funcs[f as usize].body_packet.ready());
        }
    });
}

#[test]
fn fields_address_object_shapes_in_the_owned_packet() {
    use crate::summary::packet::{FieldPort, Operation, Term};
    let mut m = super::marker_model();
    let object = m.tok(TokKind::ModNs(150));
    let value = m.tok(TokKind::ModNs(151));
    let key = m.atom("own");
    let field = m.field(object, key);
    begin(&mut m, 0);
    m.add_tok(field, value);
    m.packet_end(None, true);
    let packet = &m.funcs[0].body_packet;
    assert!(matches!(&packet.operations[0],Operation::Seed(
        Term::Field(value,FieldPort::Named(key)),_,_)
        if key=="own" && value.identity=="ModNs(150)"));
    assert!(m.nodes[field as usize].pts.is_empty());
    packet.clone().publish(&mut m);
    assert_eq!(m.nodes[field as usize].pts.to_sorted(), vec![value]);
}

#[test]
fn another_body_materializing_an_unknown_port_does_not_change_transfer() {
    fn bytes(warm: bool) -> String {
        let mut m = super::marker_model();
        m.explain = true;
        if warm {
            m.top_val("unknown external binding");
        }
        begin(&mut m, 0);
        let value = m.top_val("unknown external binding");
        m.edge(value, m.funcs[0].ret);
        m.packet_end(None, true);
        m.funcs[0].body_packet.bytes()
    }
    assert_eq!(
        bytes(false),
        bytes(true),
        "another body's materialization changed the transfer"
    );
}

#[test]
fn another_body_materializing_a_field_port_does_not_change_transfer() {
    fn bytes(warm: bool) -> String {
        let mut m = super::marker_model();
        let object = m.tok(TokKind::ModNs(160));
        let value = m.tok(TokKind::ModNs(161));
        let key = m.atom("own");
        if warm {
            m.field(object, key);
        }
        begin(&mut m, 0);
        let field = m.field(object, key);
        m.add_tok(field, value);
        m.packet_end(None, true);
        m.funcs[0].body_packet.bytes()
    }
    assert_eq!(
        bytes(false),
        bytes(true),
        "another body's field materialization changed the transfer"
    );
}

#[test]
fn observations_belong_to_the_executable_body_not_its_children() {
    inspect(
        "function make(x:any) { const child=()=>x.foreign; return [x.own,child]; }",
        |g| {
            let packet =
                &g.m.funcs
                    .iter()
                    .find(|f| f.name.ends_with(":make"))
                    .unwrap()
                    .body_packet;
            assert!(packet.observations.iter().any(|o| o.key == "own"));
            assert!(!packet.observations.iter().any(|o| o.key == "foreign"));
        },
    );
}

#[test]
fn inert_creator_registers_a_dormant_child_without_executable_descent() {
    let ast =
        perry_parser::parse_typescript("function make(x:any) { return ()=>x.unreached; }", "t.ts")
            .unwrap();
    let hir = perry_hir::lower_module(&ast, "t", "t.ts").unwrap();
    let id = hir.functions.iter().find(|f| f.name == "make").unwrap().id;
    let mut g = Gen::new(vec![("t.ts", &hir)], false, false);
    g.m.packets_enabled = true;
    g.m.packets_inert = true;
    g.register_all();
    let f = g.mods[0].funcs[&id];
    g.m.act = Some(Act::default());
    g.m.activate(f, "call");
    g.walk_all();
    g.walk_activated();
    assert!(g.m.funcs[f as usize].body_packet.ready());
    let child =
        g.m.funcs
            .iter()
            .enumerate()
            .find(|(_, child)| child.parent == f)
            .unwrap();
    assert!(!child.1.body_packet.ready());
    assert!(g.pending_walk.contains_key(&(child.0 as u32)));
    assert!(!g
        .read_sites
        .iter()
        .any(|site| g.m.atoms[site.key as usize] == "unreached"));
}

#[test]
fn function_tokens_keep_their_defining_module_with_colliding_hir_ids() {
    let a =
        perry_parser::parse_typescript("export function same(x:any){return x;}", "a.ts").unwrap();
    let b =
        perry_parser::parse_typescript("export function same(x:any){return x;}", "b.ts").unwrap();
    let ah = perry_hir::lower_module(&a, "a", "a.ts").unwrap();
    let bh = perry_hir::lower_module(&b, "b", "b.ts").unwrap();
    let mut g = Gen::new(vec![("a.ts", &ah), ("b.ts", &bh)], false, false);
    g.m.packets_enabled = true;
    g.m.packets_inert = true;
    g.register_all();
    let aid = ah.functions.iter().find(|f| f.name == "same").unwrap().id;
    let bid = bh.functions.iter().find(|f| f.name == "same").unwrap().id;
    assert_eq!(aid, bid, "control: HIR ids must collide across modules");
    let af = g.mods[0].funcs[&aid];
    let bf = g.mods[1].funcs[&bid];
    let prior = g.m.packet_body(af);
    let out = g.m.node_l("function operand");
    g.m.add_tok(out, g.m.funcs[bf as usize].token);
    g.m.packet_end(prior, true);
    let bytes = g.m.funcs[af as usize].body_packet.bytes();
    assert!(
        bytes.contains("b.ts"),
        "function operand lost its defining module: {bytes}"
    );
}

#[test]
fn another_body_interning_key_shapes_does_not_change_transfer() {
    fn bytes(warm: bool) -> String {
        let mut m = super::marker_model();
        if warm {
            m.key_shape(crate::model::KeyShape::of(&perry_hir::Expr::String(
                "unrelated".into(),
            )));
        }
        let shape = m.key_shape(crate::model::KeyShape::of(&perry_hir::Expr::String(
            "own".into(),
        )));
        begin(&mut m, 0);
        let idx = m.node_l("index");
        let obj = m.node_l("object");
        let dst = m.funcs[0].ret;
        m.cons(
            obj,
            Cons::IdxLoad {
                idx,
                dst,
                shape,
                kept: false,
            },
        );
        m.packet_end(None, true);
        m.funcs[0].body_packet.bytes()
    }
    assert_eq!(
        bytes(false),
        bytes(true),
        "global shape interning changed body bytes"
    );
}

#[test]
fn incoming_import_values_cannot_choose_a_body_transfer() {
    fn bytes(seed: bool) -> String {
        let ast = perry_parser::parse_typescript("class C {} function make() {}", "t.ts").unwrap();
        let hir = perry_hir::lower_module(&ast, "t", "t.ts").unwrap();
        let id = hir.functions.iter().find(|f| f.name == "make").unwrap().id;
        let mut g = Gen::new(vec![("t.ts", &hir)], false, false);
        g.m.packets_enabled = true;
        g.register_all();
        let f = g.mods[0].funcs[&id];
        let import = g.m.node_l("foreign class binding");
        g.mods[0].imports.insert("Foreign".into(), import);
        if seed {
            let tm = g.mods[0].classes["C"];
            let t = g.m.tok(TokKind::ClassObj(tm));
            g.m.add_tok(import, t);
        }
        let fi = &g.m.funcs[f as usize];
        let fx = crate::gen::Fx {
            module: 0,
            f,
            this: fi.this,
            ret: fi.ret,
            class: None,
            ctor_of: None,
            new_target: fi.ret,
            fn_ctor: true,
            guard: None,
        };
        let prior = g.m.packet_body(f);
        g.expr(
            &fx,
            &perry_hir::Expr::RegisterPrototypeMethod {
                class_name: "Foreign".into(),
                method_name: "method".into(),
                value: Box::new(perry_hir::Expr::Undefined),
            },
        );
        g.m.packet_end(prior, true);
        g.m.funcs[f as usize].body_packet.bytes()
    }
    assert_eq!(
        bytes(false),
        bytes(true),
        "incoming solved class values changed body operations"
    );
}

#[test]
fn token_receiver_remains_an_explicit_call_input() {
    fn bytes(id: u32) -> String {
        let mut m = super::marker_model();
        let value = m.tok(TokKind::ModNs(id));
        let callee = m.node_l("callee");
        let ret = m.funcs[0].ret;
        m.calls.push(crate::model::CallSite {
            this: crate::model::ThisArg::Tok(value),
            args: vec![],
            spread: vec![],
            ret,
            site: 0,
            region: m.thrown,
        });
        begin(&mut m, 0);
        m.cons(callee, Cons::Call { call: 0 });
        m.packet_end(None, true);
        m.funcs[0].body_packet.bytes()
    }
    assert_ne!(
        bytes(180),
        bytes(181),
        "token receiver was erased from the transfer"
    );
}

fn class_action_bytes(expr: &perry_hir::Expr, seed: bool, inert: bool) -> String {
    let ast = perry_parser::parse_typescript("class C {} function make() {}", "t.ts").unwrap();
    let hir = perry_hir::lower_module(&ast, "t", "t.ts").unwrap();
    let id = hir.functions.iter().find(|f| f.name == "make").unwrap().id;
    let mut g = Gen::new(vec![("t.ts", &hir)], false, false);
    g.m.packets_enabled = true;
    g.register_all();
    let f = g.mods[0].funcs[&id];
    let import = g.m.node_l("foreign class binding");
    g.mods[0].imports.insert("Foreign".into(), import);
    let tm = g.mods[0].classes["C"];
    if seed {
        let t = g.m.tok(TokKind::ClassObj(tm));
        g.m.add_tok(import, t);
    }
    g.m.packets_inert = inert;
    let fi = &g.m.funcs[f as usize];
    let fx = crate::gen::Fx {
        module: 0,
        f,
        this: fi.this,
        ret: fi.ret,
        class: None,
        ctor_of: None,
        new_target: fi.ret,
        fn_ctor: true,
        guard: None,
    };
    let prior = g.m.packet_body(f);
    g.expr(&fx, expr);
    g.m.packet_end(prior, true);
    if inert {
        assert!(
            !g.m.tmpls[tm as usize].per_evaluation,
            "class effect escaped staging"
        );
        assert!(
            !g.m.tmpls[tm as usize].has_computed_members,
            "accessor effect escaped staging"
        );
    }
    g.m.funcs[f as usize].body_packet.bytes()
}

fn assert_class_action(expr: perry_hir::Expr, action: &str) {
    let empty = class_action_bytes(&expr, false, false);
    let seeded = class_action_bytes(&expr, true, false);
    assert_eq!(
        empty, seeded,
        "incoming class values selected {action} operations"
    );
    assert_eq!(
        seeded,
        class_action_bytes(&expr, true, true),
        "inert {action} differs"
    );
    assert!(
        empty.contains(action),
        "missing symbolic {action} action: {empty}"
    );
}

#[test]
fn dynamic_parent_is_a_symbolic_class_binding_action() {
    assert_class_action(
        perry_hir::Expr::RegisterClassParentDynamic {
            class_name: "Foreign".into(),
            parent_expr: Box::new(perry_hir::Expr::Undefined),
        },
        "DynamicParent",
    );
}

#[test]
fn computed_method_is_a_symbolic_class_binding_action() {
    assert_class_action(
        perry_hir::Expr::RegisterClassComputedMethod {
            class_name: "Foreign".into(),
            key_expr: Box::new(perry_hir::Expr::Undefined),
            method_name: "method".into(),
            is_static: false,
            param_count: 0,
            has_rest: false,
            definition_order: 0,
        },
        "ComputedMethod",
    );
}

#[test]
fn computed_accessor_is_a_symbolic_class_binding_action() {
    assert_class_action(
        perry_hir::Expr::RegisterClassComputedAccessor {
            class_name: "Foreign".into(),
            key_expr: Box::new(perry_hir::Expr::Undefined),
            getter_name: None,
            setter_name: None,
            is_static: false,
            definition_order: 0,
        },
        "ComputedAccessor",
    );
}

fn fresh_class(parent: bool) -> perry_hir::Expr {
    perry_hir::Expr::ClassExprFresh {
        template: "Foreign".into(),
        evaluation_owner: None,
        named_statics: vec![],
        computed_keys: vec![],
        computed_statics: vec![],
        static_init_order: vec![],
        captured_args: vec![],
        shared_first_evaluation: None,
        evaluated_parent: parent.then(|| Box::new(perry_hir::Expr::Undefined)),
        definition_steps: vec![],
    }
}

#[test]
fn per_evaluation_class_is_a_symbolic_class_binding_action() {
    assert_class_action(fresh_class(false), "PerEvaluation");
}

#[test]
fn evaluated_parent_is_a_symbolic_class_binding_action() {
    assert_class_action(fresh_class(true), "EvaluatedParent");
}

fn generator_link_bytes(seed: bool, inert: bool, is_async: bool) -> String {
    let ast =
        perry_parser::parse_typescript("function make() {} function step() {}", "t.ts").unwrap();
    let hir = perry_hir::lower_module(&ast, "t", "t.ts").unwrap();
    let id = |name: &str| hir.functions.iter().find(|f| f.name == name).unwrap().id;
    let mut g = Gen::new(vec![("t.ts", &hir)], false, false);
    g.m.packets_enabled = true;
    g.register_all();
    let f = g.mods[0].funcs[&id("make")];
    let step = g.mods[0].funcs[&id("step")];
    let t = g.m.funcs[step as usize].token;
    g.m.proto_node(t);
    let input = g.m.node_l("step binding");
    g.mods[0].imports.insert("incomingStep".into(), input);
    if seed {
        g.m.add_tok(input, t);
    }
    g.m.packets_inert = inert;
    let fi = &g.m.funcs[f as usize];
    let fx = crate::gen::Fx {
        module: 0,
        f,
        this: fi.this,
        ret: fi.ret,
        class: None,
        ctor_of: None,
        new_target: fi.ret,
        fn_ctor: true,
        guard: None,
    };
    let prior = g.m.packet_body(f);
    g.expr(
        &fx,
        &perry_hir::Expr::LinkGeneratorPrototype {
            obj: Box::new(perry_hir::Expr::ExternFuncRef {
                name: "incomingStep".into(),
                param_types: vec![],
                return_type: perry_hir::types::Type::Any,
            }),
            is_async,
            fixed_state: false,
            i32_controls: vec![],
            i1_controls: vec![],
        },
    );
    g.m.packet_end(prior, true);
    if inert {
        assert!(
            g.m.funcs[step as usize].gen_proto.is_none(),
            "prototype escaped staging"
        );
    }
    g.m.funcs[f as usize].body_packet.bytes()
}

#[test]
fn incoming_step_values_cannot_choose_generator_prototype_operations() {
    for is_async in [false, true] {
        let bytes = generator_link_bytes(false, false, is_async);
        assert_eq!(
            bytes,
            generator_link_bytes(true, false, is_async),
            "incoming step values selected generator operations"
        );
        assert_eq!(
            bytes,
            generator_link_bytes(true, true, is_async),
            "inert generator link differs"
        );
        assert!(bytes.contains("generator prototype"));
    }
}

#[test]
fn fixed_resume_aliases_and_controls_remain_ordinary_packet_operations() {
    use crate::summary::packet::{Operation, Term};
    let ast = perry_parser::parse_typescript(
        "function* make(){const value=yield 1;return value;}",
        "t.ts",
    )
    .unwrap();
    let mut hir = perry_hir::lower_module(&ast, "t", "t.ts").unwrap();
    perry_transform::transform_generators(&mut hir);
    let mut expected = None;
    perry_hir::tdz_check::for_each_module_expr(&hir, &mut |e| {
        if let perry_hir::Expr::LinkGeneratorPrototype {
            obj,
            fixed_state: true,
            i32_controls,
            i1_controls,
            ..
        } = e
        {
            if let perry_hir::Expr::Closure {
                params, captures, ..
            } = obj.as_ref()
            {
                let value = if params.len() == 1 {
                    params.first()
                } else {
                    params.get(1)
                };
                expected = Some((
                    value.unwrap().id,
                    captures[2],
                    i32_controls
                        .iter()
                        .chain(i1_controls)
                        .copied()
                        .collect::<Vec<_>>(),
                ));
            }
        }
    });
    let (value, sent, controls) = expected.expect("control must use fixed resume lowering");
    assert!(!controls.is_empty(), "control must exercise scalar seeds");
    let mut g = Gen::new(vec![("t.ts", &hir)], false, false);
    g.m.packets_enabled = true;
    g.register_all();
    g.walk_all();
    fn local(t: &Term) -> Option<u32> {
        match t {
            Term::Cell(c) | Term::Environment(c) => Some(c.local),
            _ => None,
        }
    }
    let packet =
        g.m.funcs
            .iter()
            .map(|f| &f.body_packet)
            .find(|p| {
                p.operations
                    .iter()
                    .any(|op| matches!(op, Operation::GeneratorPrototype { .. }))
            })
            .unwrap();
    assert!(
        packet.operations.iter().any(
            |op| matches!(op,Operation::Edge(a,b) if local(a)==Some(value)&&local(b)==Some(sent))
        ),
        "fixed resume alias disappeared"
    );
    for id in controls {
        assert!(packet.operations.iter().any(|op|matches!(op,Operation::Seed(n,_,p) if local(n)==Some(id)&&*p==crate::model::prim::NUM|crate::model::prim::BOOL)),"control seed {id} disappeared");
    }
}
