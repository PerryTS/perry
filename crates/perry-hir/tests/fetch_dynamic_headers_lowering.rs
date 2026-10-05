//! The complete init must reach runtime dictionary conversion, including the
//! dynamic HeadersInit cases from #4932 and shorthand headers from #11024.
use perry_diagnostics::SourceCache;
use perry_hir::{lower_module, Expr, Stmt};
use perry_parser::parse_typescript_with_cache;

fn fetch_args(src: &str) -> (perry_hir::Module, Vec<Expr>) {
    let mut cache = SourceCache::new();
    let parsed = parse_typescript_with_cache(src, "fetch.ts", &mut cache).unwrap();
    let module = lower_module(&parsed.module, "test", "fetch.ts").unwrap();
    let mut found = None;
    fn walk(expr: &Expr, found: &mut Option<Vec<Expr>>) {
        if let Expr::Call { callee, args, .. } = expr {
            if callee.is_global_fetch_callee() {
                *found = Some(args.clone());
            }
        }
        perry_hir::walker::walk_expr_children(expr, &mut |child| walk(child, found));
    }
    for stmt in &module.init {
        if let Stmt::Expr(expr) = stmt {
            walk(expr, &mut found);
        }
    }
    (
        module,
        found.expect("fetch must use the callable global with its original arguments"),
    )
}

fn fields(module: &perry_hir::Module, expr: &Expr) -> Vec<(String, Expr)> {
    match expr {
        Expr::Object(fields) => fields.clone(),
        Expr::New {
            class_name, args, ..
        } => {
            let class = module
                .classes
                .iter()
                .find(|class| &class.name == class_name)
                .expect("record class");
            assert!(class_name.starts_with("__AnonShape_"));
            class
                .fields
                .iter()
                .zip(args)
                .map(|(field, value)| (field.name.clone(), value.clone()))
                .collect()
        }
        _ => panic!("whole init object missing: {expr:?}"),
    }
}

#[test]
fn literal_headers_remain_in_the_complete_init() {
    let (module, args) = fetch_args(
        r#"fetch("http://x/", {method: "POST", headers: {Authorization: "Bearer x"}, body: "b", keepalive: true});"#,
    );
    assert_eq!(args.len(), 2);
    let init_fields = fields(&module, &args[1]);
    assert_eq!(init_fields.len(), 4);
    let headers = init_fields
        .iter()
        .find(|(key, _)| key == "headers")
        .unwrap();
    assert_eq!(fields(&module, &headers.1).len(), 1);
}

#[test]
fn dynamic_headers_remain_in_the_complete_init() {
    for src in [
        r#"const h = {}; h.Authorization = "Bearer x"; fetch("http://x/", {headers: h});"#,
        r#"const headers = new Headers({Authorization: "Bearer x"}); fetch("http://x/", {headers});"#,
        r#"const h = {}; fetch("http://x/", {headers: {...h}});"#,
        r#"const h = {}; fetch("http://x/", {headers: Object.assign({}, h)});"#,
    ] {
        let (module, args) = fetch_args(src);
        let init_fields = fields(&module, &args[1]);
        assert_eq!(init_fields.len(), 1);
        assert_eq!(init_fields[0].0, "headers");
        assert!(!matches!(init_fields[0].1, Expr::Undefined));
    }
}

#[test]
fn forwarded_init_and_extra_argument_are_retained() {
    let (_module, args) = fetch_args(
        r#"const opts = {headers: {Accept: "corgi"}}; fetch("http://x/", opts, console.log("extra"));"#,
    );
    assert_eq!(args.len(), 3);
    assert!(matches!(args[1], Expr::LocalGet(_)));
    assert!(!matches!(args[2], Expr::Undefined));
}
