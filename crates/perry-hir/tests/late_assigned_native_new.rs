//! #11322: a binding declared WITHOUT an initializer and assigned a `new C(...)`
//! later must be classified exactly like `const x = new C(...)`. The assignment
//! path used to carry its own, looser classification that tagged the heap-object
//! `url` URL as a `url` native instance, so `u.hostname` lowered to a
//! receiver-bound `NativeMethodCall` returning `undefined` (mongodb 7.0.0's
//! `HostAddress`: `let url; try { url = new URL(s) } catch {}`).

use perry_diagnostics::SourceCache;
use perry_hir::lower_module;
use perry_parser::parse_typescript_with_cache;

fn lowered_debug(src: &str) -> String {
    let src = src.to_string();
    std::thread::Builder::new()
        .stack_size(32 * 1024 * 1024)
        .spawn(move || {
            let mut cache = SourceCache::new();
            let parsed = parse_typescript_with_cache(&src, "late_assign.ts", &mut cache)
                .expect("parse should succeed");
            let module = lower_module(&parsed.module, "test", "late_assign.ts").expect("lowers");
            format!("{:?}", module.functions)
        })
        .expect("spawn lower thread")
        .join()
        .expect("lower thread panicked")
}

#[test]
fn late_assigned_url_reads_like_an_initialized_one() {
    for decl in [
        "let u;",
        "var u;",
        "let u: any;",
        "let u = new URL(\"http://first/\");",
    ] {
        let hir = lowered_debug(&format!(
            r#"
            import {{ URL, URLSearchParams }} from "url";
            import {{ TextEncoder }} from "util";
            export function f(s: string) {{
                {decl}
                try {{ u = new URL(s); }} catch (e) {{ throw e; }}
                let p; p = new URLSearchParams("a=1");
                let t; t = new TextEncoder();
                return [u.hostname, u.port, p.size, t.encoding];
            }}
            "#
        ));
        assert!(
            !hir.contains("NativeMethodCall { module: \"url\""),
            "{decl}: a late-assigned url.URL/URLSearchParams must not be a `url` native instance:\n{hir}"
        );
        assert!(
            !hir.contains("NativeMethodCall { module: \"util\""),
            "{decl}: a late-assigned util.TextEncoder must not be a `util` native instance:\n{hir}"
        );
        assert!(
            hir.contains("property: \"hostname\""),
            "{decl}: `u.hostname` should be a plain property read:\n{hir}"
        );
    }
}

#[test]
fn late_assigned_genuine_native_class_is_still_tagged() {
    // Control: a real native class (imported under an alias) must keep
    // registering through the assignment path.
    let hir = lowered_debug(
        r#"
        import { BlockList as B } from "net";
        export function f() {
            let q;
            q = new B();
            q.addAddress("1.2.3.4");
            return q.check("1.2.3.4");
        }
        "#,
    );
    assert!(
        hir.contains("module: \"net\""),
        "a late-assigned net.BlockList must still dispatch natively:\n{hir}"
    );
}
