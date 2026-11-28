use crate::{lower_module, Module};
use perry_diagnostics::SourceCache;

fn lower(source: &'static str) -> Module {
    std::thread::Builder::new()
        .stack_size(32 * 1024 * 1024)
        .spawn(move || {
            let mut cache = SourceCache::new();
            let parsed =
                perry_parser::parse_typescript_with_cache(source, "client.ts", &mut cache).unwrap();
            lower_module(&parsed.module, "client", "client.ts").unwrap()
        })
        .unwrap()
        .join()
        .unwrap()
}

/// A binding assigned `http.request(...)` after its declaration is a
/// ClientRequest, as a declaration initializer is: `client.end()` then calls
/// the request's end instead of reading the bound method.
#[test]
fn late_assigned_request_is_a_client_request() {
    for module_name in ["http", "https"] {
        let source: &'static str = Box::leak(
            format!(
                "import * as {module_name} from 'node:{module_name}';
                 let client: any;
                 function start() {{ client = {module_name}.request('{module_name}://127.0.0.1/'); client.end(); }}"
            )
            .into_boxed_str(),
        );
        let module = lower(source);
        let body = format!(
            "{:?}",
            module
                .functions
                .iter()
                .find(|f| f.name == "start")
                .unwrap()
                .body
        );
        assert!(
            body.contains(
                r#"class_name: Some("ClientRequest"), object: Some(LocalGet(0)), method: "end""#
            ),
            "{module_name}: {body}"
        );
    }
}
