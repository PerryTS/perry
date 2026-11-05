//! #11725: namespace constructor calls must keep their explicit receiver.
use crate::{lower_module, Module};
use perry_diagnostics::SourceCache;

fn lower(source: String) -> Module {
    std::thread::Builder::new()
        .stack_size(32 * 1024 * 1024)
        .spawn(move || {
            let mut cache = SourceCache::new();
            let parsed =
                perry_parser::parse_typescript_with_cache(&source, "response.ts", &mut cache)
                    .unwrap();
            lower_module(&parsed.module, "response", "response.ts").unwrap()
        })
        .unwrap()
        .join()
        .unwrap()
}

#[test]
fn server_response_namespace_call_and_apply_keep_this() {
    for import in [
        "import * as http from 'http';",
        "import * as http from 'node:http';",
        "import http from 'node:http';",
        "const http = require('node:http');",
    ] {
        for invocation in ["call(this, req)", "apply(this, [req])", "apply(this, args)"] {
            let module = lower(format!("{import} function Response(this: any, req: any, args: any) {{ http.ServerResponse.{invocation}; }}"));
            let body = format!(
                "{:?}",
                module
                    .functions
                    .iter()
                    .find(|f| f.name == "Response")
                    .unwrap()
                    .body
            );
            let method = if invocation.starts_with("call") {
                "call"
            } else {
                "apply"
            };
            assert!(
                body.contains(&format!("property: \"{method}\"")),
                "must invoke Function.prototype.{method}: {body}"
            );
            assert!(body.contains("This"), "must preserve explicit this: {body}");
            assert!(
                !body.contains("NativeMethodCall"),
                "must not construct a discarded handle: {body}"
            );
        }
    }
}

#[test]
fn borrowed_namespace_functions_keep_reflective_dispatch() {
    let module = lower("import * as path from 'node:path'; function join() { return path.join.call(null, 'a', 'b'); } function direct() { return path.join('a', 'b'); }".into());
    let body = format!(
        "{:?}",
        module
            .functions
            .iter()
            .find(|f| f.name == "join")
            .unwrap()
            .body
    );
    assert!(
        body.contains("property: \"call\""),
        "borrowed namespace calls retain the selected function: {body}"
    );
    let direct = format!(
        "{:?}",
        module
            .functions
            .iter()
            .find(|f| f.name == "direct")
            .unwrap()
            .body
    );
    assert!(
        direct.contains("PathJoin"),
        "direct namespace calls retain intrinsic lowering: {direct}"
    );
}
