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

#[test]
fn property_push_mixed_spread_lowers_to_single_spread_source() {
    let module = lower_src(
        r#"
        class LazyPath {
          _cachedPath = [];
          _path = [];
          _key = 1;
          get path() {
            this._cachedPath.push(...this._path, this._key);
            return this._cachedPath;
          }
        }
        "#,
    )
    .expect("property receiver mixed spread push should lower");

    let debug = format!("{module:#?}");
    assert!(
        debug.contains("method: \"push_spread\""),
        "property receiver mixed spread push should use push_spread: {debug}"
    );
    assert!(
        debug.contains("ArraySpread"),
        "mixed spread args should be packed into one spread source array: {debug}"
    );
    assert!(
        !debug.contains("method: \"push_spread\",\n")
            || !debug.contains("args: [\n                                        PropertyGet"),
        "push_spread should not keep two direct args for codegen: {debug}"
    );
}
