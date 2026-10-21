use super::*;

fn lower(src: &str) -> HirModule {
    let src = src.to_owned();
    std::thread::Builder::new()
        .stack_size(32 * 1024 * 1024)
        .spawn(move || {
            let mut cache = perry_diagnostics::SourceCache::new();
            let parsed =
                perry_parser::parse_typescript_with_cache(&src, "test.ts", &mut cache).unwrap();
            perry_hir::lower_module(&parsed.module, "test", "test.ts").unwrap()
        })
        .unwrap()
        .join()
        .unwrap()
}

#[test]
fn field_initializer_writes_are_module_facts() {
    for field in [
        "value = (count = 1)",
        "static value = (count = 2)",
        "value = () => ++count",
        "static value = () => ++count",
    ] {
        let m = lower(&format!("let count = 0; class C {{ {field}; }}"));
        let mut expected = HashSet::new();
        for c in &m.classes {
            for f in c.fields.iter().chain(&c.static_fields) {
                if let Some(init) = &f.init {
                    visit_expr(init, true, &mut |_| {}, &mut |e| {
                        if let Expr::LocalSet(id, _) | Expr::Update { id, .. } = e {
                            expected.insert(*id);
                        }
                    });
                }
            }
        }
        assert!(
            !expected.is_empty(),
            "fixture must contain a field write: {field}"
        );
        let facts = module_facts(&m, Limits::from_env());
        assert!(
            expected.is_subset(&facts.writes),
            "field write omitted: {field}, expected {expected:?}, got {:?}",
            facts.writes
        );
    }
}
