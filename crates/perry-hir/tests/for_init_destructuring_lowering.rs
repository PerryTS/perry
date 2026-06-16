//! Regression test for #5248: a destructuring declarator inside a C-style
//! `for` init clause (`for (var {a} = o; …)`) used to bail with
//! `error[U006]: Unsupported binding pattern` because the for-init lowering
//! called `get_binding_name` (idents only) on every declarator. The fix routes
//! `Pat::Object` / `Pat::Array` declarators through the shared
//! `lower_pattern_binding` helper. These cases lower at module top level and
//! inside a function body (two separate for-init lowering paths).

use perry_diagnostics::SourceCache;
use perry_hir::{lower_module, Module};

fn lower_result(src: &str) -> Result<Module, String> {
    let src = src.to_string();
    std::thread::Builder::new()
        .stack_size(32 * 1024 * 1024)
        .spawn(move || {
            let mut cache = SourceCache::new();
            let parsed = perry_parser::parse_typescript_with_cache(
                &src,
                "for_init_destructuring.ts",
                &mut cache,
            )
            .expect("parse should succeed");
            lower_module(&parsed.module, "test", "for_init_destructuring.ts")
                .map_err(|e| e.to_string())
        })
        .expect("spawn lower thread")
        .join()
        .expect("lower thread panicked")
}

/// Every form listed in the issue's "Scope" table, both at module scope and
/// nested in a function body, must lower without the U006 bail.
#[test]
fn for_init_destructuring_declarators_lower() {
    let forms = [
        // (object, multi-declarator)
        "for (var { a, b } = { a: 1, b: 2 }, i = 0; i < 1; i++) console.log(a, b, i);",
        // (object, single declarator)
        "for (var { a, b } = { a: 1, b: 2 }; a < 5; a++) console.log(a, b);",
        // let
        "for (let { a } = { a: 1 }, i = 0; i < 1; i++) console.log(a, i);",
        // const
        "for (const { a } = { a: 1 }; ;) { console.log(a); break; }",
        // array pattern
        "for (var [a, b] = [1, 2], i = 0; i < 1; i++) console.log(a, b, i);",
        // renamed object keys (the bundler-emitted shape from the issue)
        "for (var { a: x, b: y } = { a: 1, b: 2 }, i = 0; i < 1; i++) console.log(x, y, i);",
    ];

    for form in forms {
        // Module top level (lower/stmt.rs path).
        lower_result(form).unwrap_or_else(|e| {
            panic!("module-scope for-init destructuring should lower: {form}\n{e}")
        });

        // Inside a function body (lower_decl/body_stmt.rs path).
        let in_fn = format!("function f() {{ {form} }}\nf();");
        lower_result(&in_fn).unwrap_or_else(|e| {
            panic!("function-body for-init destructuring should lower: {form}\n{e}")
        });
    }
}
