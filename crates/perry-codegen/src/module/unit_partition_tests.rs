//! Regression coverage for globals retained without function-body references.

use super::LlModule;
use crate::types::{PTR, VOID};

#[test]
fn owner_only_dispatch_resolves_bytes_in_another_unit() {
    for target in [
        "x86_64-unknown-linux-gnu",
        "x86_64-pc-windows-msvc",
        "arm64-apple-macosx15.0.0",
    ] {
        let mut module = LlModule::new(target);
        module.add_named_string_constant("m_.str.67.bytes", 8, "c\"segment\\00\"");
        module.add_raw_global(
            "@m_.str.67.dispatch = private unnamed_addr constant { i32, i32, i64, ptr } \
             { i32 7, i32 0, i64 0, ptr @m_.str.67.bytes }"
                .to_string(),
        );
        module.declare_function("consume", VOID, &[PTR]);
        // The largest function takes unit 0; only the smaller function uses
        // the bytes. The unreferenced dispatch descriptor falls back to unit 0.
        let block = module
            .define_function("big", VOID, vec![])
            .create_block("entry");
        for _ in 0..20 {
            block.call_void("consume", &[(PTR, "null")]);
        }
        block.ret_void();
        module
            .define_function("bytes_user", PTR, vec![])
            .create_block("entry")
            .ret(PTR, "@m_.str.67.bytes");

        let parts = module.codegen_unit_parts(2);
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[0].funcs[0].name, "big");
        assert_eq!(parts[1].funcs[0].name, "bytes_user");
        assert!(parts[0].pre.contains("@m_.str.67.dispatch ="));
        assert!(!parts[1].pre.contains("@m_.str.67.dispatch ="));

        // Parse the skeleton itself: this is where native construction failed
        // in #10152, before there were any function bodies to dump.
        #[cfg(feature = "llvm-inprocess")]
        for part in &parts {
            let context = inkwell::context::Context::create();
            let skeleton = format!("{}{}", part.pre, part.post);
            crate::inprocess::parse_ir_text(&context, &skeleton, "dispatch_skeleton")
                .expect("retained dispatch initializer must resolve its bytes")
                .verify()
                .expect("valid unit skeleton");
        }

        if target.contains("apple") {
            for part in &parts {
                assert!(part.pre.contains(
                    "@m_.str.67.bytes = linkonce_odr hidden unnamed_addr constant [8 x i8]"
                ));
            }
        } else {
            assert!(parts[0]
                .pre
                .contains("@m_.str.67.bytes = external hidden constant [8 x i8]"));
            assert!(parts[1]
                .pre
                .contains("@m_.str.67.bytes = hidden unnamed_addr constant [8 x i8]"));
        }
    }
}

#[test]
fn owner_only_global_dependencies_are_transitive() {
    let mut module = LlModule::new("x86_64-unknown-linux-gnu");
    module.add_internal_constant("anchor", PTR, "@middle");
    module.add_internal_constant("middle", PTR, "@payload");
    module.add_internal_constant("payload", PTR, "null");
    module.declare_function("consume", VOID, &[PTR]);
    let block = module
        .define_function("big", VOID, vec![])
        .create_block("entry");
    for _ in 0..20 {
        block.call_void("consume", &[(PTR, "null")]);
    }
    block.ret_void();
    module
        .define_function("middle_user", PTR, vec![])
        .create_block("entry")
        .ret(PTR, "@middle");

    let units = module.render_codegen_units(2);
    assert!(units[0].contains("@anchor = hidden constant ptr @middle"));
    assert!(units[0].contains("@middle = external hidden constant ptr"));
    assert!(units[0].contains("@payload = external hidden constant ptr"));
    assert!(units[1].contains("@middle = hidden constant ptr @payload"));
    assert!(units[1].contains("@payload = hidden constant ptr null"));
    assert!(!units[1].contains("@anchor ="));
}

#[test]
fn cross_unit_globals_keep_local_binding_and_tls_model() {
    use crate::types::I64;
    for target in [
        "x86_64-unknown-linux-gnu",
        "x86_64-pc-windows-msvc",
        "arm64-apple-macosx15.0.0",
    ] {
        let mut module = LlModule::new(target);
        module.add_internal_global("module_slot", I64, "7");
        module
            .add_raw_global("@module_tls = internal thread_local(initialexec) global i64 9".into());
        module.declare_function("consume", VOID, &[PTR]);
        for name in ["first", "second"] {
            let block = module
                .define_function(name, VOID, vec![])
                .create_block("entry");
            block.call_void("consume", &[(PTR, "@module_slot")]);
            block.call_void("consume", &[(PTR, "@module_tls")]);
            block.ret_void();
        }
        let units = module.render_codegen_units(2);
        assert_eq!(units.len(), 2);
        for (name, prefix, init) in [
            ("module_slot", "", "7"),
            ("module_tls", "thread_local(initialexec) ", "9"),
        ] {
            if target.contains("apple") {
                for unit in &units {
                    assert!(
                        unit.contains(&format!(
                            "@{name} = linkonce_odr hidden {prefix}global i64 {init}"
                        )),
                        "{unit}"
                    );
                }
            } else {
                assert_eq!(
                    units
                        .iter()
                        .filter(|unit| unit
                            .contains(&format!("@{name} = hidden {prefix}global i64 {init}")))
                        .count(),
                    1,
                    "one hidden owning definition: {units:?}"
                );
                assert_eq!(
                    units
                        .iter()
                        .filter(|unit| unit
                            .contains(&format!("@{name} = external hidden {prefix}global i64")))
                        .count(),
                    1,
                    "one hidden cross-unit declaration: {units:?}"
                );
            }
        }
        #[cfg(feature = "llvm-inprocess")]
        for unit in &units {
            let context = inkwell::context::Context::create();
            crate::inprocess::parse_ir_text(&context, unit, "hidden_unit")
                .unwrap()
                .verify()
                .unwrap();
        }
    }
}

#[test]
fn executable_definitions_are_hidden_but_imports_and_library_exports_stay_visible() {
    use crate::types::I64;
    let mut module = LlModule::new("x86_64-unknown-linux-gnu");
    module.add_global("module_public", I64, "7");
    module.add_external_global("embedder_data", I64);
    for name in ["first", "second"] {
        module
            .define_function(name, PTR, vec![])
            .create_block("entry")
            .ret(PTR, "@module_public");
    }
    let library = module.render_codegen_units(2);
    assert!(library
        .iter()
        .any(|unit| unit.contains("@module_public = global i64 7")));
    assert!(library
        .iter()
        .any(|unit| unit.contains("@module_public = external global i64")));
    module.bind_generated_globals_to_image();
    let executable = module.render_codegen_units(2);
    assert!(executable
        .iter()
        .any(|unit| unit.contains("@module_public = hidden global i64 7")));
    assert!(executable
        .iter()
        .any(|unit| unit.contains("@module_public = external hidden global i64")));
    assert!(executable
        .iter()
        .any(|unit| unit.contains("@embedder_data = external global i64")));
    assert!(!executable
        .iter()
        .any(|unit| unit.contains("@embedder_data = external hidden")));
}

#[cfg(all(
    feature = "llvm-inprocess",
    target_os = "linux",
    target_arch = "x86_64"
))]
#[test]
fn split_global_declarations_match_native_and_text_streams() {
    use crate::types::I64;
    fn fixture() -> LlModule {
        let mut module = LlModule::new("x86_64-unknown-linux-gnu");
        module.add_internal_global("module_slot", I64, "7");
        // Lowering can import a symbol before defining it. The definition is
        // authoritative; native freeze must not append this stale declaration.
        module.declarations.push((
            "module_slot".into(),
            "@module_slot = external global i64".into(),
        ));
        for name in ["first", "second"] {
            let block = module
                .define_function(name, I64, vec![])
                .create_block("entry");
            let value = block.load(I64, "@module_slot");
            block.ret(I64, &value);
        }
        module
    }
    let _native = crate::codegen::helpers::NativeRootsPin::native();
    let text =
        crate::linker::compile_streamed_units_to_object(fixture().into_codegen_unit_texts(2), None)
            .unwrap();
    let native = crate::native_emit::compile_module_units_native(
        &mut fixture(),
        2,
        None,
        "split_global_declarations",
    )
    .unwrap();
    assert_eq!(
        native, text,
        "global visibility must be identical in both streams"
    );
}

#[test]
fn hidden_image_literals_keep_cross_module_coalescing() {
    use super::{hidden_global_definition, make_unique_owner_global};
    let definition = hidden_global_definition("@image = weak_odr constant [2 x i8] c\"a\\00\"");
    assert_eq!(
        definition,
        "@image = weak_odr hidden constant [2 x i8] c\"a\\00\""
    );
    assert_eq!(make_unique_owner_global(&definition), definition);
}

#[test]
fn mach_o_offset_alias_has_one_hidden_owner() {
    fn fixture() -> LlModule {
        let mut module = LlModule::new("arm64-apple-macosx15.0.0");
        module.add_raw_global("@payload = weak_odr constant [4 x i8] c\"abc\\00\"".into());
        module.add_raw_global(
            "@fnname = private alias i8, getelementptr (i8, ptr @payload, i64 1)".into(),
        );
        for name in ["first", "second"] {
            module
                .define_function(name, PTR, vec![])
                .create_block("entry")
                .ret(PTR, "@fnname");
        }
        module.bind_generated_globals_to_image();
        module
    }
    let units = fixture().render_codegen_units(2);
    assert_eq!(
        units
            .iter()
            .filter(|unit| unit.contains("@fnname = hidden alias i8,"))
            .count(),
        1
    );
    assert_eq!(
        units
            .iter()
            .filter(|unit| unit.contains("@fnname = external hidden global i8"))
            .count(),
        1
    );
    assert!(units
        .iter()
        .all(|unit| !unit.contains("@fnname = linkonce_odr")));
    #[cfg(all(feature = "llvm-inprocess", target_os = "macos"))]
    {
        let _native = crate::codegen::helpers::NativeRootsPin::native();
        let object = crate::native_emit::compile_module_units_native(
            &mut fixture(),
            2,
            None,
            "hidden_offset_alias",
        )
        .expect("hidden Mach-O offset aliases must survive the partial link");
        assert!(!object.is_empty());
    }
}
