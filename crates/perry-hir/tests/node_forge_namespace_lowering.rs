//! node-forge deeply-nested namespace calls (`forge.pki.rsa.generateKeyPair`,
//! `forge.pki.createCertificate`, `forge.md.sha256.create`) must flatten to a
//! `NativeMethodCall { module: "node-forge", method: <last segment> }` so they
//! dispatch through the perry-ext-node-forge surface instead of falling to the
//! JS-runtime path an AOT binary can't execute. Guards the flattening added
//! for the Socket Firewall TLS-MITM CA port.

use perry_diagnostics::SourceCache;
use perry_hir::{lower_module, Expr, Module, Stmt};
use perry_parser::parse_typescript_with_cache;

fn lower(src: &str) -> Module {
    let src = src.to_string();
    std::thread::Builder::new()
        .stack_size(32 * 1024 * 1024)
        .spawn(move || {
            let mut cache = SourceCache::new();
            let parsed = parse_typescript_with_cache(&src, "test.ts", &mut cache)
                .expect("parse should succeed");
            lower_module(&parsed.module, "test", "test.ts").expect("lowering should succeed")
        })
        .expect("spawn lower thread")
        .join()
        .expect("lower thread panicked")
}

fn find_native_method_call<'a>(expr: &'a Expr, method: &str) -> Option<(&'a str, Option<&'a str>)> {
    match expr {
        Expr::NativeMethodCall {
            module,
            class_name,
            object,
            method: call_method,
            args,
        } => {
            if call_method == method {
                return Some((module.as_str(), class_name.as_deref()));
            }
            object
                .as_deref()
                .and_then(|object| find_native_method_call(object, method))
                .or_else(|| {
                    args.iter()
                        .find_map(|arg| find_native_method_call(arg, method))
                })
        }
        _ => None,
    }
}

fn native_call_in_inits(module: &Module, method: &str) -> Option<(String, Option<String>)> {
    module.init.iter().find_map(|stmt| match stmt {
        Stmt::Let {
            init: Some(expr), ..
        }
        | Stmt::Expr(expr) => find_native_method_call(expr, method)
            .map(|(m, c)| (m.to_string(), c.map(str::to_string))),
        _ => None,
    })
}

#[test]
fn three_level_namespace_call_flattens_to_last_segment() {
    // forge.pki.rsa.generateKeyPair(...) → method "generateKeyPair".
    let module = lower(
        r#"
        import forge from "node-forge";
        const keys = forge.pki.rsa.generateKeyPair({ bits: 2048 });
    "#,
    );
    let call = native_call_in_inits(&module, "generateKeyPair")
        .expect("generateKeyPair should lower to a NativeMethodCall");
    assert_eq!(call.0, "node-forge");
}

#[test]
fn two_level_namespace_call_flattens_before_module_class_static() {
    // forge.pki.createCertificate() is the 2-level shape `try_module_class_static`
    // would otherwise claim (reading `forge.pki` as module.Class and gating it);
    // the node-forge arm must win first.
    let module = lower(
        r#"
        import forge from "node-forge";
        const cert = forge.pki.createCertificate();
    "#,
    );
    let call = native_call_in_inits(&module, "createCertificate")
        .expect("createCertificate should lower to a NativeMethodCall");
    assert_eq!(call.0, "node-forge");
}

#[test]
fn md_sub_namespace_call_flattens() {
    // forge.md.sha256.create() → method "create".
    let module = lower(
        r#"
        import forge from "node-forge";
        const md = forge.md.sha256.create();
    "#,
    );
    let call =
        native_call_in_inits(&module, "create").expect("md.sha256.create should lower natively");
    assert_eq!(call.0, "node-forge");
}

#[test]
fn unsupported_md_namespace_does_not_flatten_to_sha256_create() {
    let module = lower(
        r#"
        import forge from "node-forge";
        const md = forge.md.md5.create();
    "#,
    );
    assert!(
        native_call_in_inits(&module, "create").is_none(),
        "forge.md.md5.create must not dispatch to the SHA-256 native marker"
    );
}
