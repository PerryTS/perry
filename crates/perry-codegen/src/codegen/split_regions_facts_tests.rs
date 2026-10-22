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

#[test]
fn generated_locals_follow_field_only_closure_parameters() {
    let mut m = lower("class C { value = (unused: any) => 1; }");
    m.local_source_spans.clear();
    let field = &mut m.classes.iter_mut().find(|c| c.name == "C").unwrap().fields[0];
    let Expr::Closure { params, .. } = field.init.as_mut().unwrap() else {
        panic!("field closure");
    };
    params[0].id = 1_000_000;
    assert!(
        max_local_id(&m) >= 1_000_000,
        "field-only parameter must precede generated locals"
    );
}
