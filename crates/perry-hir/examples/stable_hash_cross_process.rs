//! Cross-process determinism check for `perry_hir::stable_hash`.
//!
//! Constructs a canonical `Module` whose `ObjectType.properties`
//! (the only `HashMap` reachable from `Module`) is populated with
//! several keys. Rust's `HashMap` uses `RandomState` by default —
//! iteration order changes BETWEEN processes — so any forgotten
//! "sort by key" inside the hash walk would surface as a different
//! printed hash on a second run.
//!
//! Run twice, compare stdout. The integration test
//! `tests/cross_process.rs` does exactly this. Output is a single
//! 16-hex-digit u64 followed by `\n`.

use perry_hir::ir::*;
use perry_hir::stable_hash::hash_module;
use perry_hir::types::{ObjectType, PropertyInfo, Type};
use std::collections::HashMap;

fn build_canonical() -> Module {
    let mut m = Module::new("cross_process_canonical");

    // Build an ObjectType whose HashMap has multiple entries — this is
    // the only place HashMap iteration order could leak into the hash.
    let mut props: HashMap<String, PropertyInfo> = HashMap::new();
    for (k, ty) in [
        ("alpha", Type::Number),
        ("beta", Type::String),
        ("gamma", Type::Boolean),
        ("delta", Type::Int32),
        ("epsilon", Type::Any),
        ("zeta", Type::Symbol),
        ("eta", Type::BigInt),
        ("theta", Type::Null),
    ] {
        props.insert(
            k.to_string(),
            PropertyInfo {
                ty,
                optional: false,
                readonly: false,
            },
        );
    }
    let obj = ObjectType {
        name: Some("Canon".to_string()),
        properties: props,
        property_order: None,
        index_signature: None,
    };

    // Embed the ObjectType in a TypeAlias so it sits inside Module's
    // reachable-from-codegen graph.
    m.type_aliases.push(TypeAlias {
        id: 1,
        name: "Canon".to_string(),
        type_params: vec![],
        ty: Type::Object(obj),
        is_exported: true,
    });

    // A small function so we exercise more of the walk too.
    m.functions.push(Function {
        id: 1,
        name: "twice".to_string(),
        type_params: vec![],
        params: vec![Param {
            id: 0,
            name: "n".to_string(),
            ty: Type::Number,
            default: None,
            decorators: Vec::new(),
            is_rest: false,
            arguments_object: None,
        }],
        return_type: Type::Number,
        body: vec![Stmt::Return(Some(Expr::Binary {
            op: BinaryOp::Mul,
            left: Box::new(Expr::LocalGet(0)),
            right: Box::new(Expr::Number(2.0)),
        }))],
        is_async: false,
        is_generator: false,
        is_strict: false,
        is_exported: true,
        captures: vec![],
        decorators: vec![],
        was_plain_async: false,
        was_unrolled: false,
    });

    m
}

fn main() {
    let m = build_canonical();
    println!("{:016x}", hash_module(&m));
}
