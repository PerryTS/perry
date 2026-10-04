//! The bounded-index element read (`bidx.*`, `lower_bounded_array_index_get_checked`),
//! read from the emitted IR.
//!
//! ```ts
//! function f(bs: any[]) {
//!   let s: any = 0;
//!   for (let i = 0; i < bs.length; i++) { s = bs[i]; }
//!   return s;
//! }
//! ```
//!
//! The counter loop proves `i < bs.length`, so `bs[i]` takes this tier. Its
//! receiver is only DECLARED an array, and the declaration is a hint.
//!
//! * #11875: a Proxy bound to `bs` is a POINTER-tagged id in the handle band.
//!   The tier read the receiver's GC header before any receiver test, i.e.
//!   from unmapped memory (SIGSEGV). The header block must be reached only
//!   through the fused tag + handle-band compare.

#![cfg(test)]

use perry_hir::types::Type;
use perry_hir::{CompareOp, Expr, Function, Module, Param, Stmt, UpdateOp};

use crate::{compile_module, CompileOptions};

const BS: u32 = 1;
const S: u32 = 2;
const I: u32 = 3;

fn module_ir() -> String {
    let mut module = Module::new("bounded_array_index");
    module.init.push(Stmt::Expr(Expr::Call {
        callee: Box::new(Expr::FuncRef(1)),
        args: vec![Expr::Array(vec![Expr::Integer(1), Expr::Integer(2)])],
        type_args: Vec::new(),
        byte_offset: 0,
    }));
    let counter = Stmt::Let {
        id: I,
        name: "i".to_string(),
        ty: Type::Number,
        mutable: true,
        init: Some(Expr::Integer(0)),
    };
    let condition = Expr::Compare {
        op: CompareOp::Lt,
        left: Box::new(Expr::LocalGet(I)),
        right: Box::new(Expr::PropertyGet {
            object: Box::new(Expr::LocalGet(BS)),
            property: "length".to_string(),
            byte_offset: 0,
        }),
    };
    let update = Expr::Update {
        id: I,
        op: UpdateOp::Increment,
        prefix: false,
    };
    let body = vec![Stmt::Expr(Expr::LocalSet(
        S,
        Box::new(Expr::IndexGet {
            object: Box::new(Expr::LocalGet(BS)),
            index: Box::new(Expr::LocalGet(I)),
        }),
    ))];
    module.functions.push(Function {
        id: 1,
        name: "f".to_string(),
        type_params: Vec::new(),
        params: vec![Param {
            id: BS,
            name: "bs".to_string(),
            ty: Type::Array(Box::new(Type::Any)),
            default: None,
            decorators: Vec::new(),
            is_rest: false,
            arguments_object: None,
        }],
        return_type: Type::Any,
        body: vec![
            Stmt::Let {
                id: S,
                name: "s".to_string(),
                ty: Type::Any,
                mutable: true,
                init: Some(Expr::Integer(0)),
            },
            Stmt::For {
                init: Some(Box::new(counter)),
                condition: Some(condition),
                update: Some(update),
                body,
            },
            Stmt::Return(Some(Expr::LocalGet(S))),
        ],
        is_async: false,
        is_generator: false,
        is_strict: true,
        is_exported: true,
        captures: Vec::new(),
        decorators: Vec::new(),
        was_plain_async: false,
        was_unrolled: false,
    });
    let opts = CompileOptions {
        emit_ir_only: true,
        output_type: "executable".to_string(),
        ..Default::default()
    };
    String::from_utf8(compile_module(&module, opts).expect("module compiles"))
        .expect("LLVM IR is UTF-8")
}

/// The body of the block whose label starts with `prefix` (up to the next
/// blank line).
fn block<'a>(ir: &'a str, prefix: &str) -> &'a str {
    let start = ir
        .find(&format!("\n{prefix}"))
        .unwrap_or_else(|| panic!("premise: no block `{prefix}*` in\n{ir}"));
    let rest = &ir[start + 1..];
    &rest[..rest.find("\n\n").unwrap_or(rest.len())]
}

/// The instructions that end with the branch into the block named `label`.
fn predecessor_of<'a>(ir: &'a str, label: &str) -> &'a str {
    let at = ir
        .find(&format!("label %{label}"))
        .unwrap_or_else(|| panic!("premise: no branch to `{label}` in\n{ir}"));
    let start = ir[..at].rfind("\n\n").map_or(0, |p| p + 2);
    &ir[start..at]
}

fn label_of(block_text: &str) -> &str {
    block_text.split(':').next().expect("block label")
}

#[test]
fn the_receiver_header_is_read_only_behind_the_heap_pointer_test() {
    let ir = module_ir();
    let header = block(&ir, "bidx.header.");
    assert!(
        header.contains("@js_array_get_f64") || header.contains("load i8"),
        "premise: the header block reads the receiver's GC header\n{header}"
    );
    let entry = predecessor_of(&ir, label_of(header));
    let bias = (crate::expr::receiver_range::RECEIVER_BIAS as i64).to_string();
    let span = (crate::expr::receiver_range::RECEIVER_SPAN as i64).to_string();
    assert!(
        entry.contains("sub i64 ") && entry.contains(&bias),
        "#11875: the header block must be guarded by the fused receiver compare\n{entry}"
    );
    assert!(
        entry.contains("icmp ult i64 ") && entry.contains(&span),
        "#11875: the fused compare must bound the payload above the handle band\n{entry}"
    );
    assert!(
        !entry.contains("load i8") && !entry.contains("load i16"),
        "#11875: nothing may read the receiver's header before the receiver test\n{entry}"
    );
    // The compare must DECIDE the entry: an unconditional branch into the
    // header block, with the compare computed and ignored, reads the same
    // instructions and still dereferences a proxy id.
    let last = entry.trim_end().lines().last().unwrap_or("").trim();
    let cond = last
        .strip_prefix("br i1 ")
        .and_then(|rest| rest.split(',').next())
        .unwrap_or_else(|| {
            panic!("#11875: the header block must be entered by a conditional branch, got `{last}`\n{entry}")
        });
    assert!(
        entry.contains(&format!("{cond} = icmp ult i64 ")),
        "#11875: the branch into the header block must test the fused receiver compare ({cond})\n{entry}"
    );
}
