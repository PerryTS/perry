use perry_diagnostics::SourceCache;
use perry_hir::{clear_current_module_source, fix_local_native_instances, lower_module};
use perry_parser::parse_typescript_with_cache;

#[test]
fn awaited_axios_response_properties_keep_native_dispatch() {
    let mut cache = SourceCache::new();
    let parsed = parse_typescript_with_cache(
        r#"
        import axios from "axios";

        async function main() {
            const response = await axios.get("https://example.com/data");
            console.log(`status=${response.status}`);
            console.log(`ok=${response.data.ok}`);
        }
        "#,
        "/tmp/axios_response_property_lowering.ts",
        &mut cache,
    )
    .expect("parse");
    let mut module = lower_module(
        &parsed.module,
        "test",
        "/tmp/axios_response_property_lowering.ts",
    )
    .expect("lower");
    clear_current_module_source();
    fix_local_native_instances(&mut module);

    let main = module
        .functions
        .iter()
        .find(|function| function.name == "main")
        .expect("main function");
    let hir = format!("{:#?}", main.body);
    assert!(
        hir.matches("class_name: Some(").count() == 2
            && hir.matches("\"Response\"").count() == 2
            && hir.contains("method: \"status\"")
            && hir.contains("method: \"data\""),
        "awaited Axios response properties lost native dispatch:\n{hir}"
    );
}
