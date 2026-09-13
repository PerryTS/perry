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
                assert!(part
                    .pre
                    .contains("@m_.str.67.bytes = linkonce_odr unnamed_addr constant [8 x i8]"));
            }
        } else {
            assert!(parts[0]
                .pre
                .contains("@m_.str.67.bytes = external constant [8 x i8]"));
            assert!(parts[1]
                .pre
                .contains("@m_.str.67.bytes = unnamed_addr constant [8 x i8]"));
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
    assert!(units[0].contains("@anchor = constant ptr @middle"));
    assert!(units[0].contains("@middle = external constant ptr"));
    assert!(units[0].contains("@payload = external constant ptr"));
    assert!(units[1].contains("@middle = constant ptr @payload"));
    assert!(units[1].contains("@payload = constant ptr null"));
    assert!(!units[1].contains("@anchor ="));
}
