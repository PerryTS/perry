use perry_codegen::{assign_static_shape_ids, compile_module, module_birth_shapes, CompileOptions};
use perry_diagnostics::SourceCache;
use perry_hir::lower_module;
use perry_parser::parse_typescript_with_cache;

/// A computed method of a class DECLARATION that reads its home (`super`)
/// shares its direct body: the string pool defines no `__eclo` entry for it,
/// so its registration must not name one (minipass, glob: undefined
/// `..._computed_method_named_..__eclo$info` at link). Every entry info the
/// module names is one it defines.
#[test]
fn computed_method_registration_names_only_defined_entries() {
    let source = r#"
class Base { greet(): string { return "base"; } }
const key = "hello";
class Derived extends Base {
  [key](): string { return super.greet() + "!"; }
}
const d: any = new Derived();
console.log(d[key]());
"#;
    let mut cache = SourceCache::new();
    let parsed = parse_typescript_with_cache(source, "computed_eclo.ts", &mut cache).unwrap();
    let module = lower_module(&parsed.module, "computed_eclo", "computed_eclo.ts").unwrap();
    let mut options = CompileOptions {
        emit_ir_only: true,
        output_type: "executable".into(),
        disable_constfn_shapes: false,
        ..Default::default()
    };
    let births = module_birth_shapes(&module, options.clone()).unwrap();
    options.static_shape_ids = assign_static_shape_ids(births.iter().map(|b| &b.shape))
        .into_iter()
        .collect();
    let ir = String::from_utf8(compile_module(&module, options).unwrap()).unwrap();
    assert!(ir.contains("js_register_class_computed_method"), "the member is computed");
    for line in ir.lines() {
        let Some(decl) = line.strip_prefix("@") else { continue };
        let Some((name, rhs)) = decl.split_once(" = ") else { continue };
        assert!(
            !(name.ends_with("__eclo$info") && rhs.starts_with("external ")),
            "@{name} is named but this module defines no such entry"
        );
    }
}
