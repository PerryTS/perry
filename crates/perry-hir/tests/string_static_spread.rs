use perry_hir::{lower_module, CallArg, Expr, Stmt};
use perry_parser::parse_typescript;

#[test]
fn string_static_spreads_keep_the_constructor_and_argument_boundaries() {
    for method in ["fromCodePoint", "fromCharCode", "raw"] {
        for property in [format!(".{method}"), format!("['{method}']")] {
            for (args, spread) in [
                ("...[65, 66]", vec![true]),
                ("...[]", vec![true]),
                (
                    "64, ...[65, 66], 67, ...[68]",
                    vec![false, true, false, true],
                ),
            ] {
                let source = format!("const result = String{property}({args});");
                let parsed = parse_typescript(&source, "string_spread.js").unwrap();
                let module = lower_module(&parsed, "test", "string_spread.js").unwrap();
                let result = module
                    .init
                    .iter()
                    .find_map(|stmt| match stmt {
                        Stmt::Let { name, init, .. } if name == "result" => init.as_ref(),
                        _ => None,
                    })
                    .expect("result initializer");
                let Expr::CallSpread { callee, args, .. } = result else {
                    panic!("{source} lost spread: {result:?}");
                };
                assert_eq!(
                    args.iter()
                        .map(|arg| matches!(arg, CallArg::Spread(_)))
                        .collect::<Vec<_>>(),
                    spread
                );
                let debug = format!("{callee:?}");
                assert!(
                    debug.contains("\"String\"") && debug.contains(method),
                    "{source} lost its String constructor receiver: {debug}"
                );
            }
        }
    }
}
