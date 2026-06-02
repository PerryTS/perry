//! Issue #1193 — `const $ = load(html); $(sel).text()` must lower to
//! cheerio NativeMethodCall dispatch instead of a generic function call
//! that the runtime can't resolve.

use perry_diagnostics::SourceCache;
use perry_hir::{clear_current_module_source, fix_local_native_instances, lower_module, Expr};
use perry_parser::parse_typescript_with_cache;

fn lower(src: &str) -> perry_hir::Module {
    let mut cache = SourceCache::new();
    let parsed =
        parse_typescript_with_cache(src, "/tmp/cheerio_test.ts", &mut cache).expect("parse failed");
    let mut hir =
        lower_module(&parsed.module, "test", "/tmp/cheerio_test.ts").expect("lower failed");
    clear_current_module_source();
    fix_local_native_instances(&mut hir);
    hir
}

#[test]
fn cheerio_call_handle_lowers_to_select() {
    let src = r#"
        import { load } from "cheerio";
        const $ = load("<p class='x'>hi</p>");
        const t = $(".x").text();
        const n = $(".x").length;
    "#;
    let module = lower(src);
    // The `const t = $(".x").text()` should hold a chained NativeMethodCall:
    // NativeMethodCall { method: "text", object: Some(NativeMethodCall { method: "select", ... }) }
    let t_init = module
        .init
        .iter()
        .find_map(|s| match s {
            perry_hir::Stmt::Let {
                name,
                init: Some(e),
                ..
            } if name == "t" => Some(e),
            _ => None,
        })
        .expect("expected a `t` Let binding");
    let (outer_module, outer_method, inner) = match t_init {
        Expr::NativeMethodCall {
            module,
            method,
            object: Some(inner),
            ..
        } => (module.as_str(), method.as_str(), inner.as_ref()),
        other => panic!(
            "expected NativeMethodCall for $('.x').text(), got: {:?}",
            other
        ),
    };
    assert_eq!(outer_module, "cheerio");
    assert_eq!(outer_method, "text");
    let (inner_module, inner_method) = match inner {
        Expr::NativeMethodCall { module, method, .. } => (module.as_str(), method.as_str()),
        other => panic!(
            "expected inner NativeMethodCall for $('.x'), got: {:?}",
            other
        ),
    };
    assert_eq!(inner_module, "cheerio");
    assert_eq!(inner_method, "select");

    // `.length` is a PropertyGet in JS but must also become a NativeMethodCall.
    let n_init = module
        .init
        .iter()
        .find_map(|s| match s {
            perry_hir::Stmt::Let {
                name,
                init: Some(e),
                ..
            } if name == "n" => Some(e),
            _ => None,
        })
        .expect("expected an `n` Let binding");
    let (n_module, n_method) = match n_init {
        Expr::NativeMethodCall { module, method, .. } => (module.as_str(), method.as_str()),
        other => panic!(
            "expected NativeMethodCall for $('.x').length, got: {:?}",
            other
        ),
    };
    assert_eq!(n_module, "cheerio");
    assert_eq!(n_method, "length");
}
