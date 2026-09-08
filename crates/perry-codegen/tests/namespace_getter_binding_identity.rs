use perry_codegen::{compile_module, CompileOptions};
use perry_hir::Module;

fn empty_opts() -> CompileOptions {
    CompileOptions {
        emit_ir_only: true,
        output_type: "executable".into(),
        ..Default::default()
    }
}

#[test]
fn namespace_variable_getters_preserve_raw_local_binding_names() {
    let module = Module::new("namespace_collision.js");
    let mut opts = empty_opts();
    opts.namespace_entries.push(perry_codegen::NamespaceEntry {
        name: "Tool".into(),
        kind: perry_codegen::NamespaceEntryKind::ForeignVar {
            source_prefix: "provider_js".into(),
            source_local: "$item".into(),
        },
    });
    let ir = String::from_utf8(compile_module(&module, opts).unwrap()).unwrap();
    let getter = ir
        .split("define double @__perry_ns_get_namespace_collision_js__0(")
        .nth(1)
        .and_then(|rest| rest.split("\n}").next())
        .expect("the namespace live getter must be emitted");
    assert!(
        getter.contains("call double @perry_fn_provider_js__$item()"),
        "namespace getter must read the raw dollar binding:\n{getter}"
    );
    assert!(!getter.contains("call double @perry_fn_provider_js___item()"));
}
