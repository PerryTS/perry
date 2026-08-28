//! The inline `for (const [k, v] of map)` entry read (`lower_map_entry_at_inline`).

use crate::{compile_module, CompileOptions};
use perry_hir::types::Type;
use perry_hir::{Expr, Function, Module, Param, Stmt};

fn param(id: u32, name: &str) -> Param {
    Param {
        id,
        name: name.to_string(),
        ty: Type::Any,
        default: None,
        decorators: Vec::new(),
        is_rest: false,
        arguments_object: None,
    }
}

fn entry_read_ir(value_slot: bool) -> String {
    let read = if value_slot {
        Expr::MapEntryValueAt {
            map: Box::new(Expr::LocalGet(10)),
            idx: Box::new(Expr::LocalGet(11)),
        }
    } else {
        Expr::MapEntryKeyAt {
            map: Box::new(Expr::LocalGet(10)),
            idx: Box::new(Expr::LocalGet(11)),
        }
    };
    let f = Function {
        id: 1,
        name: "read".to_string(),
        type_params: Vec::new(),
        params: vec![param(10, "m"), param(11, "i")],
        return_type: Type::Any,
        body: vec![Stmt::Return(Some(read))],
        is_async: false,
        is_generator: false,
        is_strict: true,
        was_plain_async: false,
        was_unrolled: false,
        is_exported: false,
        captures: Vec::new(),
        decorators: Vec::new(),
    };
    let mut module = Module::new("map_entry_at_test.ts");
    module.functions = vec![f];
    let opts = CompileOptions {
        emit_ir_only: true,
        ..Default::default()
    };
    String::from_utf8(compile_module(&module, opts).expect("fixture must compile"))
        .expect("LLVM IR is UTF-8")
}

/// The admitted receiver reads the slot inline — a `GC_TYPE_MAP` header test,
/// a live-size bounds check, the `entries` buffer re-read — and the runtime
/// helper is kept as the fallback for every miss.
#[test]
fn map_entry_reads_are_inline_with_the_helper_as_the_fallback() {
    let key_ir = entry_read_ir(false);
    assert!(
        key_ir.contains("map_entry_key.head")
            && key_ir.contains("map_entry_key.fast")
            && key_ir.contains("icmp eq i8")
            && key_ir.contains("load double")
            && key_ir.contains("call double @js_map_entry_key_at("),
        "the key read should be inline with the helper as fallback:\n{key_ir}"
    );
    let value_ir = entry_read_ir(true);
    assert!(
        value_ir.contains("map_entry_value.fast")
            && value_ir.contains("call double @js_map_entry_value_at("),
        "the value read should be inline with the helper as fallback:\n{value_ir}"
    );
    // The value slot is the second word of the 16-byte entry.
    assert!(
        value_ir.contains("add i64 %") && value_ir.contains(", 8\n") || value_ir.contains(", 8 "),
        "the value read should offset by one word within the entry:\n{value_ir}"
    );
}
