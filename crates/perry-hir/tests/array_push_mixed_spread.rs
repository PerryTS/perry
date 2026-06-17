use perry_diagnostics::SourceCache;
use perry_hir::{lower_module, Expr};
use perry_parser::parse_typescript_with_cache;

fn lower_src(src: &str) -> anyhow::Result<perry_hir::Module> {
    let mut cache = SourceCache::new();
    let parsed = parse_typescript_with_cache(src, "array_push_mixed_spread.ts", &mut cache)?;
    lower_module(&parsed.module, "test", "array_push_mixed_spread.ts")
}

#[test]
fn array_push_mixed_spread_lowers_to_ordered_push_sequence() {
    let module = lower_src(
        r#"
        const parts = [];
        const extras = [1, 2];
        const tail = 3;
        parts.push(...extras, tail);
        "#,
    )
    .expect("mixed spread push should lower");

    let debug = format!("{module:#?}");
    assert!(
        debug.contains("ArrayPushSpread"),
        "mixed spread push should include the spread push step: {debug}"
    );
    assert!(
        debug.contains("ArrayPush"),
        "mixed spread push should include the plain push step: {debug}"
    );
}

#[test]
fn array_push_plain_then_spread_lowers_to_ordered_push_sequence() {
    let module = lower_src(
        r#"
        const parts = [];
        const head = 1;
        const extras = [2, 3];
        parts.push(head, ...extras);
        "#,
    )
    .expect("plain then spread push should lower");

    let found_mixed_sequence = module.init.iter().any(|stmt| {
        let perry_hir::Stmt::Expr(Expr::Sequence(exprs)) = stmt else {
            return false;
        };
        matches!(
            exprs.as_slice(),
            [Expr::ArrayPush { .. }, Expr::ArrayPushSpread { .. }]
        )
    });

    assert!(
        found_mixed_sequence,
        "plain then spread push should preserve source order in a sequence: {module:#?}"
    );
}
