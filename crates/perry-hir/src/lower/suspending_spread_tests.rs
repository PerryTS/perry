//! A spread object literal that suspends keeps the source-ordered builder:
//! `ObjectSpread` is one expression, and a `yield` or `await` hoisted out of
//! it would run before the parts written ahead of it.

use crate::{Expr, Module, Stmt};
use perry_diagnostics::SourceCache;

fn lower(src: &str) -> Module {
    let src = src.to_string();
    std::thread::Builder::new()
        .stack_size(32 * 1024 * 1024)
        .spawn(move || {
            let mut cache = SourceCache::new();
            let parsed = perry_parser::parse_typescript_with_cache(&src, "spread.ts", &mut cache)
                .expect("parses");
            crate::lower_module(&parsed.module, "test", "spread.ts").expect("lowers")
        })
        .expect("spawn")
        .join()
        .expect("lowering thread")
}

/// The returned expression of function `name`.
fn returned(m: &Module, name: &str) -> Expr {
    let f = m.functions.iter().find(|f| f.name == name).expect("function");
    f.body
        .iter()
        .find_map(|s| match s {
            Stmt::Return(Some(e), _) => Some(e.clone()),
            _ => None,
        })
        .expect("return")
}

#[test]
fn a_suspending_spread_literal_keeps_the_builder() {
    let m = lower(
        "function mark(v: string) { return v; }\n\
         function* gen(): any { return { a: mark(\"a\"), ...(yield 0), [yield 1]: 2 }; }\n\
         async function run(p: any): Promise<any> { return { a: mark(\"a\"), ...(await p) }; }\n\
         function plain(s: any): any { return { a: mark(\"a\"), ...s, [mark(\"k\")]: 2 }; }",
    );
    for name in ["gen", "run"] {
        let e = returned(&m, name);
        assert!(!matches!(e, Expr::ObjectSpread { .. }), "{name}: {e:?}");
    }
    let e = returned(&m, "plain");
    assert!(matches!(e, Expr::ObjectSpread { .. }), "plain: {e:?}");
}
