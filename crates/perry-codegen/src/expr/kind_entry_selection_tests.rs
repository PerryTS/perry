//! Kind-typed entry selection (`collection_receiver::select_collection_receiver`).
//!
//! A receiver the compiler itself constructed (`const m = new Map()`) is a
//! static kind proof: the site calls the kind-typed entry with no receiver
//! check. Everything else (a reassigned binding, a declared type, any other
//! value) keeps the checked receiver and the generic entry.
use crate::temp_root_coverage::main_ir_for as ir_for;
use perry_hir::types::Type;
use perry_hir::{Expr, Stmt};

const M: u32 = 1;
const R: u32 = 2;

fn local(id: u32, mutable: bool, init: Expr) -> Stmt {
    Stmt::Let {
        id,
        name: format!("v{id}"),
        ty: Type::Any,
        mutable,
        init: Some(init),
    }
}

fn size_of(id: u32) -> Expr {
    Expr::PropertyGet {
        byte_end: 0,
        object: Box::new(Expr::LocalGet(id)),
        property: "size".to_string(),
        byte_offset: 0,
    }
}

#[test]
fn a_constructed_map_selects_the_typed_size_and_get_entries() {
    let ir = ir_for(
        "kind_entry_constructed_map",
        vec![
            local(M, false, Expr::MapNew),
            local(R, false, size_of(M)),
            Stmt::Expr(Expr::MapGet {
                map: Box::new(Expr::LocalGet(M)),
                key: Box::new(Expr::Number(1.0)),
            }),
        ],
    );
    assert!(
        ir.contains("@js_map_size__Map("),
        "typed Map size not selected:\n{ir}"
    );
    assert!(
        ir.contains("@js_map_get__Map("),
        "typed Map get not selected:\n{ir}"
    );
    assert!(
        !ir.contains("@js_map_size("),
        "generic Map size remained:\n{ir}"
    );
    assert!(
        !ir.contains("collection_recv.throw"),
        "a constructed receiver still emitted the receiver check:\n{ir}"
    );
}

#[test]
fn a_constructed_set_selects_the_typed_size_entry() {
    let ir = ir_for(
        "kind_entry_constructed_set",
        vec![local(M, false, Expr::SetNew), local(R, false, size_of(M))],
    );
    assert!(
        ir.contains("@js_set_size__Set("),
        "typed Set size not selected:\n{ir}"
    );
    assert!(
        !ir.contains("@js_set_size("),
        "generic Set size remained:\n{ir}"
    );
}

#[test]
fn a_reassigned_map_binding_keeps_the_generic_entry() {
    let ir = ir_for(
        "kind_entry_reassigned_map",
        vec![
            local(M, true, Expr::MapNew),
            Stmt::Expr(Expr::LocalSet(M, Box::new(Expr::MapNew))),
            local(R, false, size_of(M)),
        ],
    );
    assert!(
        !ir.contains("__Map("),
        "a reassigned binding is no construction proof:\n{ir}"
    );
}

/// A receiver the compiler did not construct tests its brand before the
/// `size` entry; a miss reads `size` the ordinary way (wa4).
#[test]
fn an_unconstructed_map_size_tests_its_brand_and_reads_size_on_a_miss() {
    let ir = ir_for(
        "kind_entry_unconstructed_size",
        vec![
            local(M, true, Expr::MapNew),
            Stmt::Expr(Expr::LocalSet(M, Box::new(Expr::MapNew))),
            local(R, false, size_of(M)),
            Stmt::Expr(Expr::MapClear(Box::new(Expr::LocalGet(M)))),
        ],
    );
    assert!(
        ir.contains("collguard.miss"),
        "no brand test in front of size / clear:\n{ir}"
    );
    assert!(
        ir.contains("@js_collection_size_get("),
        "the size miss is not the ordinary Get:\n{ir}"
    );
    assert!(
        ir.contains("@js_builtin_proto_generic_call0("),
        "the clear() miss is not the ordinary Get + Call:\n{ir}"
    );
}

/// A constructed receiver keeps its exact code: no brand test.
#[test]
fn a_constructed_map_size_and_clear_keep_their_exact_code() {
    let ir = ir_for(
        "kind_entry_constructed_size_clear",
        vec![
            local(M, false, Expr::MapNew),
            local(R, false, size_of(M)),
            Stmt::Expr(Expr::MapClear(Box::new(Expr::LocalGet(M)))),
        ],
    );
    assert!(
        !ir.contains("collguard"),
        "a constructed receiver was brand-tested:\n{ir}"
    );
}
