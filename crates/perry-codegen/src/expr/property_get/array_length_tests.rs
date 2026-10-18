//! Array-length guards use live layout facts; no per-site cache remains.
use super::*;

#[test]
fn a_length_read_uses_the_live_array_header_in_small_and_outlined_modules() {
    for (ir, header_prefix, cold) in [
        (
            emit_read("length"),
            "pget.recv_real.",
            "js_object_get_field_slow",
        ),
        (
            emit_outlined_read("length"),
            "pget.outline_length_object.",
            "js_object_get_field_value",
        ),
    ] {
        let header = super::super::super::class_field_barrier_tests::block_body(&ir, header_prefix)
            .unwrap_or_else(|| panic!("missing array header guard {header_prefix}: {ir}"));
        assert!(
            header.contains("load i8")
                && header.contains("icmp eq i8")
                && header.contains(", 1")
                && header.contains("and i8")
                && header.contains(", -128"),
            "kind and forwarding flags guard length: {header}"
        );
        let fast =
            super::super::super::class_field_barrier_tests::block_body(&ir, "pget.array_length.")
                .expect("the live header load must remain");
        assert!(
            fast.contains("load i32") && fast.contains("uitofp i32") && !fast.contains("call "),
            "the proved array reads its live length: {fast}"
        );
        assert!(
            ir.contains(&format!("call double @{cold}(")),
            "declined receivers retain Get: {ir}"
        );
        assert!(!ir.contains("@perry_ic_") && !ir.contains("pic.token"));
    }
}

#[test]
fn only_length_grows_the_array_arm() {
    for key in ["foo", "size", "charCodeAt"] {
        for ir in [emit_read(key), emit_outlined_read(key)] {
            assert!(
                !ir.contains("pget.array_length.") && !ir.contains("pget.array_stub."),
                "{key}: {ir}"
            );
        }
    }
}

fn full_outline_module_reading(property: &str) -> Module {
    let mut m = Module::new("read_outlined.ts");
    m.functions = (0..4000u32)
        .map(|i| perry_hir::Function {
            id: 10_000 + i,
            name: format!("pad{i}"),
            type_params: Vec::new(),
            params: Vec::new(),
            return_type: perry_hir::types::Type::Void,
            body: Vec::new(),
            is_async: false,
            is_generator: false,
            is_strict: true,
            is_exported: false,
            captures: Vec::new(),
            decorators: Vec::new(),
            was_plain_async: false,
            was_unrolled: false,
        })
        .collect();
    m.init = vec![
        Stmt::Let {
            id: 1,
            name: "o".to_string(),
            ty: perry_hir::types::Type::Any,
            mutable: false,
            init: Some(Expr::Undefined),
        },
        Stmt::Expr(Expr::PropertyGet {
            object: Box::new(Expr::LocalGet(1)),
            property: property.to_string(),
            byte_offset: 0,
        }),
    ];
    m.init_kind = ModuleInitKind::Eager;
    m
}

fn emit_outlined_read(property: &str) -> String {
    let module = full_outline_module_reading(property);
    assert!(
        crate::codegen::decide_full_outline_ic(crate::codegen::module_callable_count(&module)),
        "test premise: the fixture must cross the full-outline threshold"
    );
    String::from_utf8(compile_module(&module, ir_opts(false, None)).unwrap())
        .expect("LLVM IR should be UTF-8")
}
