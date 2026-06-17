use perry_diagnostics::SourceCache;
use perry_hir::{lower_module, Export, Module};
use perry_parser::parse_typescript_with_cache;

fn lower_result(src: &str) -> Result<Module, String> {
    let src = src.to_string();
    std::thread::Builder::new()
        .stack_size(32 * 1024 * 1024)
        .spawn(move || {
            let mut cache = SourceCache::new();
            let parsed = parse_typescript_with_cache(&src, "exported_destructuring.ts", &mut cache)
                .expect("parse should succeed");
            lower_module(&parsed.module, "test", "exported_destructuring.ts")
                .map_err(|e| e.to_string())
        })
        .expect("spawn lower thread")
        .join()
        .expect("lower thread panicked")
}

fn named_exports(module: &Module) -> Vec<(String, String)> {
    module
        .exports
        .iter()
        .filter_map(|export| match export {
            Export::Named { local, exported } => Some((local.clone(), exported.clone())),
            _ => None,
        })
        .collect()
}

#[test]
fn exported_object_binding_pattern_with_rename_and_shorthand_lowers() {
    let module = lower_result(
        r#"
        const Data: any = {
            taggedEnum() {
                return {
                    $match: "match",
                    EntityRegistered: "entity",
                    SingletonRegistered: "singleton"
                };
            }
        };

        export const {
            $match: match,
            EntityRegistered,
            SingletonRegistered
        } = Data.taggedEnum();
        "#,
    )
    .expect("exported object destructuring should lower");

    let exports = named_exports(&module);
    for name in ["match", "EntityRegistered", "SingletonRegistered"] {
        assert!(
            exports.contains(&(name.to_string(), name.to_string())),
            "destructured binding {name} should be exported: {exports:?}"
        );
        assert!(
            module.exported_objects.contains(&name.to_string()),
            "destructured binding {name} should get an export global: {:?}",
            module.exported_objects
        );
    }

    let debug = format!("{module:#?}");
    assert!(
        debug.contains("name: \"match\"") && debug.contains("property: \"$match\""),
        "renamed $match binding should lower through a property read: {debug}"
    );
}
