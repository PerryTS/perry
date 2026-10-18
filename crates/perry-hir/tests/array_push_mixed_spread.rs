use perry_diagnostics::SourceCache;
use perry_hir::{lower_module, CallArg, Expr, Stmt};
use perry_parser::parse_typescript_with_cache;

fn lower_src(src: &str) -> anyhow::Result<perry_hir::Module> {
    let mut cache = SourceCache::new();
    let parsed = parse_typescript_with_cache(src, "array_push_mixed_spread.ts", &mut cache)?;
    lower_module(&parsed.module, "test", "array_push_mixed_spread.ts")
}

fn assert_push_order(module: &perry_hir::Module, expected: [(&str, bool); 2]) {
    let local = |name: &str| {
        module
            .init
            .iter()
            .find_map(|stmt| match stmt {
                Stmt::Let {
                    id, name: found, ..
                } if found == name => Some(*id),
                _ => None,
            })
            .expect("fixture declares the local")
    };
    let calls: Vec<_> = module
        .init
        .iter()
        .filter_map(|stmt| match stmt {
            Stmt::Expr(Expr::CallSpread { callee, args, .. }) => Some((callee.as_ref(), args)),
            _ => None,
        })
        .collect();
    assert_eq!(
        calls.len(),
        1,
        "mixed push retains one method lookup: {module:#?}"
    );
    let (callee, args) = calls[0];
    assert!(
        matches!(callee,
            Expr::PropertyGet { object, property, .. }
                if matches!(object.as_ref(), Expr::LocalGet(id) if *id == local("parts"))
                    && property == "push"
        ),
        "the call must read parts.push: {callee:#?}"
    );
    let observed: Vec<_> = args
        .iter()
        .map(|arg| match arg {
            CallArg::Expr(Expr::LocalGet(id)) => (*id, false),
            CallArg::Spread(Expr::LocalGet(id)) => (*id, true),
            _ => panic!("fixture arguments retain their local values: {arg:#?}"),
        })
        .collect();
    assert_eq!(
        observed,
        expected.map(|(name, spread)| (local(name), spread))
    );
}

#[test]
fn array_push_mixed_spread_preserves_one_method_lookup_and_argument_order() {
    let module = lower_src(
        r#"
        const parts = [];
        const extras = [1, 2];
        const tail = 3;
        parts.push(...extras, tail);
    "#,
    )
    .expect("mixed spread push should lower");
    // Expanding this into separate intrinsic pushes loses the one callee
    // read before spread iteration. Keep that read and the argument order.
    assert_push_order(&module, [("extras", true), ("tail", false)]);
}

#[test]
fn array_push_plain_then_spread_preserves_one_method_lookup_and_argument_order() {
    let module = lower_src(
        r#"
        const parts = [];
        const head = 1;
        const extras = [2, 3];
        parts.push(head, ...extras);
    "#,
    )
    .expect("plain then spread push should lower");
    assert_push_order(&module, [("head", false), ("extras", true)]);
}
