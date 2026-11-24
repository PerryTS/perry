use crate::Module;
use perry_diagnostics::SourceCache;

fn lower(source: &str) -> Module {
    let source = source.to_string();
    std::thread::Builder::new()
        .stack_size(32 * 1024 * 1024)
        .spawn(move || {
            let mut cache = SourceCache::new();
            let parsed =
                perry_parser::parse_typescript_with_cache(&source, "alias.ts", &mut cache).unwrap();
            crate::lower_module(&parsed.module, "test", "alias.ts").unwrap()
        })
        .unwrap()
        .join()
        .unwrap()
}

fn is_exported(module: &Module, name: &str) -> bool {
    module
        .functions
        .iter()
        .any(|f| f.name == name && f.is_exported)
}

#[test]
fn a_binding_alias_publishes_the_declared_function() {
    let module = lower("function add(a, b) { return a + b; }\nexport const addRef = add;\n");
    assert_eq!(
        module.function_alias_origin("addRef").map(String::as_str),
        Some("add")
    );
    // Importers form `add`'s own symbols, so its body is link-visible.
    assert!(is_exported(&module, "add"));
}

#[test]
fn a_renamed_export_and_a_plain_function_export() {
    let module = lower("function add() {}\nexport function sub() {}\nexport { add as a2 };\n");
    assert_eq!(
        module.function_alias_origin("a2").map(String::as_str),
        Some("add")
    );
    assert_eq!(module.function_alias_origin("sub"), None);
}

#[test]
fn a_binding_of_a_value_is_no_function_alias() {
    let module = lower("const box = { n: 1 };\nexport const boxRef = box;\n");
    assert_eq!(module.function_alias_origin("boxRef"), None);
}
