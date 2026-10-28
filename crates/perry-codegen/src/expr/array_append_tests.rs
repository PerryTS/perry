//! The compact append store must stay free of metadata calls and growth code.
use perry_hir::types::Type;
use perry_hir::{Expr, Function, Module as HirModule, Stmt};

fn append_ir(ty: Type, value: Expr) -> String {
    let mut hir = HirModule::new("append_size_test");
    hir.functions.push(Function {
        id: 0,
        name: "pushes".to_string(),
        type_params: Vec::new(),
        params: Vec::new(),
        return_type: Type::Any,
        body: vec![
            Stmt::Let {
                id: 0,
                name: "a".to_string(),
                ty,
                mutable: true,
                init: Some(Expr::Array(Vec::new(), None)),
            },
            Stmt::Expr(Expr::IndexSet {
                object: Box::new(Expr::LocalGet(0)),
                index: Box::new(Expr::PropertyGet {
                    byte_end: 0,
                    object: Box::new(Expr::LocalGet(0)),
                    property: "length".into(),
                    byte_offset: 0,
                }),
                value: Box::new(value),
            }),
            Stmt::Return(Some(Expr::LocalGet(0))),
        ],
        is_async: false,
        is_generator: false,
        is_strict: true,
        is_exported: false,
        captures: Vec::new(),
        decorators: Vec::new(),
        was_plain_async: false,
        was_unrolled: false,
    });
    let opts = crate::CompileOptions {
        emit_ir_only: true,
        ..Default::default()
    };
    let bytes = crate::compile_module(&hir, opts).expect("test module compiles");
    String::from_utf8(bytes).expect("LLVM IR is UTF-8")
}

#[test]
fn numeric_append_changes_scan_declaration_without_metadata_calls() {
    for ty in [Type::Any, Type::Array(Box::new(Type::Any))] {
        let ir = append_ir(ty, Expr::Number(1.25));
        let label = ir
            .lines()
            .find(|line| line.starts_with("append.fast.") && line.contains(':'))
            .expect("numeric construction must exercise compact raw arm");
        let block = ir
            .split_once(label)
            .unwrap()
            .1
            .split("br label")
            .next()
            .unwrap();
        assert!(block.contains("store double"));
        assert!(block.find("store i16").unwrap() < block.find("store double").unwrap());
        assert!(
            ir.contains("407339263"),
            "numeric admission must allow all-pointer to scalar transitions"
        );
        assert!(block.contains("store i32"));
        assert!(
            !block.contains("call "),
            "raw append must not contain metadata/barrier/grow calls: {block}"
        );
        assert!(ir.contains("call double @js_array_append_set("));
        let llvm = inkwell::context::Context::create();
        let parsed = crate::inprocess::parse_ir_text(&llvm, &ir, "compact append").unwrap();
        parsed.verify().unwrap();
    }
}

#[test]
fn values_append_inline_with_their_barrier_and_share_the_runtime_continuation() {
    let ir = append_ir(Type::Any, Expr::String("heap string append".into()));
    // Growth, raw-f64/element-shape work and every unproven receiver
    // keep the one runtime continuation.
    assert!(ir.contains("call double @js_array_append_set("));
    // The note-free append (`note_noop`) is inline: one store, the slot
    // barrier, the length.
    let label = ir
        .lines()
        .find(|line| line.starts_with("append.fast.") && line.contains(':'))
        .expect("a value append has the inline layout-transition arm");
    let block = ir.split_once(label).unwrap().1;
    assert!(block.contains("store double"));
    assert!(block.find("store i16").unwrap() < block.find("store double").unwrap());
    let string_arm = ir.split("append.string.").last().unwrap();
    assert!(
        string_arm.contains("store i32 0"),
        "store sharing rather than rejecting unique strings"
    );
    assert!(
        ir.contains("415727871"),
        "the plain guard excludes raw and element-shape layouts"
    );
    assert!(ir.contains("@js_write_barrier_slot_validated_parent("));
    assert!(!ir.contains("call i64 @js_array_append_grow("));
    assert!(!ir.contains("call double @js_array_note_numeric_write_value("));
    assert!(!ir.contains("call i32 @perry_array_element_kind("));
    assert!(!ir.contains("call void @js_string_addref_if_heap_string("));
    let llvm = inkwell::context::Context::create();
    let parsed = crate::inprocess::parse_ir_text(&llvm, &ir, "shared string append").unwrap();
    parsed.verify().unwrap();
}
