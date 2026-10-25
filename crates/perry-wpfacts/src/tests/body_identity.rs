#[test]
fn specialized_frames_keep_their_own_exact_receiver_layout() {
    let ast = perry_parser::parse_typescript(
        r#"
        class A { service = "a"; layer = 123; deps = [1]; }
        class B { service = "b"; deps = [2]; layer = "b"; }
        function read<T>(input: T) {
            if (input === null) throw new Error("missing");
            return (input as any).layer;
        }
        console.log(read<A>(new A()), read<A>(new A()));
        console.log(read<B>(new B()), read<B>(new B()));
    "#,
        "identity.ts",
    )
    .unwrap();
    let mut hir = perry_hir::lower_module(&ast, "identity", "identity.ts").unwrap();
    perry_hir::monomorphize_module(&mut hir);
    let pf = crate::compute_units(&[("identity.ts".to_string(), &hir)], &Default::default());
    let mb = pf.bindings_for("identity.ts").unwrap();
    let mut seen = 0;
    for f in hir.functions.iter().filter(|f| f.name.starts_with("read$")) {
        let class_name = if f.name.ends_with("$A") {
            "A"
        } else if f.name.ends_with("$B") {
            "B"
        } else {
            continue;
        };
        let class = hir.classes.iter().find(|c| c.name == class_name).unwrap();
        let b = &mb
            .locals
            .iter()
            .find(|(id, _)| *id == f.params[0].id)
            .expect("each live body has its own parameter facts")
            .1;
        assert_eq!(b.class_ids, [class.id], "{}: {:?}", f.name, b);
        assert!(
            !b.other && b.prims == 0,
            "each body alone proves its exact receiver"
        );
        let t = pf.stable_classes()[&class.id];
        let field = t.fields.iter().find(|f| f.key == "layer").unwrap();
        assert!(field.definite && field.data_field);
        assert_eq!(field.slot, Some(if class_name == "A" { 1 } else { 2 }));
        seen += 1;
    }
    assert_eq!(
        seen, 2,
        "both independently provable bodies must stay exact"
    );
}

fn inlined(src: &str) -> perry_hir::Module {
    let ast = perry_parser::parse_typescript(src, "copies.ts").unwrap();
    let mut module = perry_hir::lower_module(&ast, "copies", "copies.ts").unwrap();
    perry_transform::inline_functions(
        &mut module,
        &Default::default(),
        &Default::default(),
        &Default::default(),
        &Default::default(),
    );
    perry_hir::monomorphize_module(&mut module);
    module
}

#[test]
fn inlined_creation_copies_keep_one_emitted_body_and_one_fact_owner() {
    let module = inlined(
        r#"
        function factory() { return (input: any) => input.layer; }
        function indirect() { return factory(); }
        const first = factory();
        const second = indirect();
        console.log(first({ layer: "first", deps: [] }), second({ deps: [], layer: "second" }));
    "#,
    );
    let bodies = perry_hir::emitted_closures(&module);
    assert_eq!(
        bodies.len(),
        1,
        "copied creations must not add machine-code bodies"
    );
    let id = bodies[0].0;
    let mut copies = 0;
    perry_hir::tdz_check::for_each_module_expr(&module, &mut |e| {
        if matches!(e, perry_hir::Expr::Closure { func_id, .. } if *func_id == id) {
            copies += 1;
        }
    });
    assert!(
        copies >= 3,
        "original, direct and indirect creations must be live"
    );
    let facts = crate::compute_units(&[("copies.ts".into(), &module)], &Default::default());
    assert!(facts.broken.is_empty(), "{:?}", facts.broken);
    let owned = facts.bindings_for("copies.ts").unwrap();
    assert_eq!(owned.this_of.iter().filter(|(f, _)| *f == id).count(), 1);
}

#[test]
fn brand_style_nominal_and_all_share_their_emitted_closures() {
    let module = inlined(
        r#"
        function nominal() {
            return Object.assign((input: any) => input, {
                option: (input: any) => ({ value: input }),
                result: (input: any) => ({ success: input }),
                is: (_: any) => true
            });
        }
        function all() { return nominal(); }
        const a = nominal();
        const b = all();
        console.log(a(1), b("b"), a.is(1), b.option("b").value);
    "#,
    );
    assert_eq!(perry_hir::emitted_closures(&module).len(), 4);
    let facts = crate::compute_units(&[("copies.ts".into(), &module)], &Default::default());
    assert!(facts.broken.is_empty(), "{:?}", facts.broken);
}

#[test]
fn every_creation_feeds_the_canonical_bodys_captured_shape() {
    let module = inlined(
        r#"
        class A { layer = "first"; deps = []; }
        class B { deps = []; layer = "second"; }
        function factory(input: any) { return () => input.layer; }
        function indirect(input: any) { return factory(input); }
        const first = factory(new A());
        const second = indirect(new B());
        console.log(first(), second());
    "#,
    );
    let bodies = perry_hir::emitted_closures(&module);
    assert_eq!(bodies.len(), 1);
    let perry_hir::Expr::Closure { captures, .. } = bodies[0].1 else {
        unreachable!()
    };
    assert_eq!(captures.len(), 1);
    let facts = crate::compute_units(&[("copies.ts".into(), &module)], &Default::default());
    let bindings = facts.bindings_for("copies.ts").unwrap();
    let captured = &bindings
        .locals
        .iter()
        .find(|(id, _)| *id == captures[0])
        .unwrap()
        .1;
    let mut expected: Vec<_> = module
        .classes
        .iter()
        .filter(|c| c.name == "A" || c.name == "B")
        .map(|c| c.id)
        .collect();
    expected.sort_unstable();
    assert_eq!(
        captured.class_ids, expected,
        "one emitted body must cover both capture sources"
    );
    assert!(!captured.other && captured.prims == 0);
}
