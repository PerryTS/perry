//! JST slice 3 (C): the pointer-push gate admits a heap string.
//!
//! An append of a heap string onto an all-pointers array at its append
//! position makes the layout note, the numeric note and (for an already shared
//! string) the demote provable no-ops. The test is one header test beside the
//! value-tag test; anything that fails it takes today's full sequence.

use perry_hir::types::Type;
use perry_hir::{Expr, Function, Module as HirModule, Stmt};

fn push_ir(value: Expr) -> String {
    let mut hir = HirModule::new("apush_string_gate_test");
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
                ty: Type::Any,
                mutable: true,
                init: Some(Expr::Array(Vec::new(), None)),
            },
            Stmt::Expr(Expr::ArrayPush {
                array_id: 0,
                value: Box::new(value),
                field_writeback: None,
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
    let bytes = crate::compile_module(&hir, opts).expect("compile");
    String::from_utf8(bytes).expect("ir is text")
}

/// Body of every block whose label starts with `prefix` (labels carry a
/// uniquing suffix, so walk labels rather than substrings).
fn blocks_named(ir: &str, prefix: &str) -> Vec<String> {
    let mut blocks = Vec::new();
    let mut current: Option<Vec<&str>> = None;
    for line in ir.lines() {
        if let Some(label) = line.strip_suffix(':') {
            if !label.starts_with(char::is_whitespace) {
                if let Some(done) = current.take() {
                    blocks.push(done.join("\n"));
                }
                if label.starts_with(prefix) {
                    current = Some(Vec::new());
                }
                continue;
            }
        }
        if let Some(body) = current.as_mut() {
            body.push(line);
        }
    }
    if let Some(done) = current.take() {
        blocks.push(done.join("\n"));
    }
    blocks
}

fn block(ir: &str, prefix: &str) -> String {
    let mut found = blocks_named(ir, prefix);
    assert_eq!(found.len(), 1, "expected one `{prefix}*` block:\n{ir}");
    found.remove(0)
}

const NOTE_CALLS: [&str; 3] = [
    "@js_string_addref_if_heap_string(",
    "@js_gc_note_slot_layout(",
    "@js_array_note_numeric_write(",
];

#[test]
fn a_string_push_has_one_header_test_and_no_note_calls_on_the_admitted_arm() {
    let ir = push_ir(Expr::String("x".to_string()));
    // The header proof (mask 0xF880 == 63616, expected 0xA000 == 40960) is
    // tested once, beside the value-tag test.
    assert_eq!(
        ir.matches(", 63616").count(),
        1,
        "exactly one header-layout mask test:\n{ir}"
    );
    assert_eq!(ir.matches(", 40960").count(), 1, "{ir}");
    // The admitted arm: no notes and no demote call before the refcount test.
    let fast = block(&ir, "apush.pointer_layout.fast");
    for call in NOTE_CALLS {
        assert!(
            !fast.contains(call),
            "admitted arm must not call {call}:\n{fast}"
        );
    }
    let tag_arm = block(&ir, "apush.pointer_layout.str_addref");
    for call in NOTE_CALLS {
        assert!(
            !tag_arm.contains(call),
            "refcount arm must not call {call}:\n{tag_arm}"
        );
    }
    assert!(
        tag_arm.contains(", 12") && tag_arm.contains("load i32"),
        "the string arm reads StringHeader.refcount (offset 12):\n{tag_arm}"
    );
    // The demote runs only for a not-yet-shared string.
    let demote = block(&ir, "apush.pointer_layout.str_demote");
    assert!(
        demote.contains("@js_string_addref_if_heap_string("),
        "{demote}"
    );
    assert!(!demote.contains("@js_gc_note_slot_layout("), "{demote}");
    // The slow arm is today's exact sequence.
    let slow = block(&ir, "apush.pointer_layout.bookkeeping");
    for call in NOTE_CALLS {
        assert!(slow.contains(call), "slow arm must keep {call}:\n{slow}");
    }
    // The barrier is untouched: still gated on the parent's header after the
    // bookkeeping blocks.
    assert!(
        ir.contains("apush.barrier."),
        "write barrier arm missing:\n{ir}"
    );
}

#[test]
fn a_value_that_cannot_be_a_string_owes_no_string_arm() {
    let ir = push_ir(Expr::Object(vec![("v".to_string(), Expr::Number(1.0))]));
    assert!(
        blocks_named(&ir, "apush.pointer_layout.str_addref").is_empty()
            && blocks_named(&ir, "apush.pointer_layout.str_demote").is_empty()
            && blocks_named(&ir, "apush.pointer_layout.fast").is_empty(),
        "an object literal never owes the string demote, so no string arm:\n{ir}"
    );
    for slow in blocks_named(&ir, "apush.pointer_layout.bookkeeping") {
        assert!(
            !slow.contains("@js_string_addref_if_heap_string("),
            "{slow}"
        );
    }
}

fn def_of<'a>(ir: &'a str, reg: &str) -> &'a str {
    ir.lines()
        .find(|line| line.trim_start().starts_with(&format!("{reg} = ")))
        .unwrap_or_else(|| panic!("no definition of {reg} in:\n{ir}"))
}

/// Sabotage guard for the gate itself: the branch into the admitted arm must
/// be an `and` whose operand is the header-proof compare (`icmp eq i16 ...,
/// 40960`). Dropping the header test (admitting on the value's tag alone)
/// leaves a branch on the tag test only and turns this red.
#[test]
fn the_admission_branch_is_conditioned_on_the_header_proof() {
    let ir = push_ir(Expr::String("x".to_string()));
    let branch = ir
        .lines()
        .find(|line| line.contains("br i1") && line.contains("label %apush.pointer_layout.fast"))
        .unwrap_or_else(|| panic!("no branch into the admitted arm:\n{ir}"));
    let condition = branch
        .split_whitespace()
        .nth(2)
        .unwrap()
        .trim_end_matches(',')
        .to_string();
    let def = def_of(&ir, &condition);
    assert!(
        def.contains("and i1"),
        "admission must conjoin its tests: {def}"
    );
    let operands: Vec<String> = def
        .split("and i1")
        .nth(1)
        .unwrap()
        .split(',')
        .map(|s| s.trim().to_string())
        .collect();
    assert!(
        operands.iter().any(|reg| {
            let d = def_of(&ir, reg);
            d.contains("icmp eq i16") && d.trim_end().ends_with(", 40960")
        }),
        "the header proof must be one operand of the admission test: {def}\n{ir}"
    );
}
