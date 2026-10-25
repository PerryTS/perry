use super::*;

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
