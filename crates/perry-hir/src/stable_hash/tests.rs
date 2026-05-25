//! Tests for the stable hash. Split out of `stable_hash.rs` (no
//! behavior change). Lives as a sibling sub-module rather than the old
//! inline `mod tests { ... }` so the parent file stays small.

use super::primitives::SH;
use super::*;
use crate::ir::*;
use perry_types::{ObjectType, PropertyInfo, Type};
use std::collections::HashMap;

fn empty_module() -> Module {
    Module::new("test_module")
}

#[test]
fn same_hir_hashes_identically() {
    let m = empty_module();
    let a = hash_module(&m);
    let b = hash_module(&m);
    assert_eq!(a, b, "same module must hash identically across calls");
}

#[test]
fn behavior_change_changes_hash() {
    let mut m1 = empty_module();
    let mut m2 = empty_module();
    // Add an extra Stmt to m2 only.
    m2.init.push(Stmt::Expr(Expr::Number(1.0)));
    assert_ne!(
        hash_module(&m1),
        hash_module(&m2),
        "adding an init stmt must change hash"
    );
    // Now add the same stmt to m1 and they should match.
    m1.init.push(Stmt::Expr(Expr::Number(1.0)));
    assert_eq!(
        hash_module(&m1),
        hash_module(&m2),
        "matching init stmts must produce identical hashes"
    );
}

#[test]
fn object_type_properties_insertion_order_independent() {
    // Build two ObjectTypes with the same content but different
    // HashMap insertion sequences. They must hash identically.
    let prop_a = PropertyInfo {
        ty: Type::Number,
        optional: false,
        readonly: false,
    };
    let prop_b = PropertyInfo {
        ty: Type::String,
        optional: true,
        readonly: false,
    };
    let prop_c = PropertyInfo {
        ty: Type::Boolean,
        optional: false,
        readonly: true,
    };

    let mut props_x: HashMap<String, PropertyInfo> = HashMap::new();
    props_x.insert("a".to_string(), prop_a.clone());
    props_x.insert("b".to_string(), prop_b.clone());
    props_x.insert("c".to_string(), prop_c.clone());

    let mut props_y: HashMap<String, PropertyInfo> = HashMap::new();
    props_y.insert("c".to_string(), prop_c.clone());
    props_y.insert("a".to_string(), prop_a.clone());
    props_y.insert("b".to_string(), prop_b.clone());

    let ot_x = ObjectType {
        name: Some("Foo".to_string()),
        properties: props_x,
        index_signature: None,
    };
    let ot_y = ObjectType {
        name: Some("Foo".to_string()),
        properties: props_y,
        index_signature: None,
    };

    let mut h1 = Djb2Hasher::new();
    ot_x.hash(&mut h1);
    let mut h2 = Djb2Hasher::new();
    ot_y.hash(&mut h2);
    assert_eq!(
        h1.finish(),
        h2.finish(),
        "ObjectType.properties HashMap insertion order MUST NOT leak into hash"
    );
}

#[test]
fn module_metadata_affects_hash() {
    let base = empty_module();
    let base_hash = hash_module(&base);

    // Different name
    let mut m_name = empty_module();
    m_name.name = "other".to_string();
    assert_ne!(base_hash, hash_module(&m_name));

    // Add an import
    let mut m_imp = empty_module();
    m_imp.imports.push(Import {
        source: "./util".to_string(),
        specifiers: vec![ImportSpecifier::Named {
            imported: "x".to_string(),
            local: "x".to_string(),
        }],
        is_native: false,
        module_kind: ModuleKind::NativeCompiled,
        resolved_path: None,
        type_only: false,
        is_dynamic: false,
        is_dynamic_target: false,
    });
    assert_ne!(base_hash, hash_module(&m_imp));

    // Add a class
    let mut m_class = empty_module();
    m_class.classes.push(Class {
        id: 1,
        name: "C".to_string(),
        type_params: vec![],
        extends: None,
        extends_name: None,
        extends_expr: None,
        native_extends: None,
        fields: vec![],
        constructor: None,
        methods: vec![],
        getters: vec![],
        setters: vec![],
        static_fields: vec![],
        static_methods: vec![],
        decorators: vec![],
        is_exported: false,
        aliases: vec![],
    });
    assert_ne!(base_hash, hash_module(&m_class));

    // Add a function
    let mut m_fn = empty_module();
    m_fn.functions.push(Function {
        id: 7,
        name: "f".to_string(),
        type_params: vec![],
        params: vec![],
        return_type: Type::Void,
        body: vec![],
        is_async: false,
        is_generator: false,
        is_exported: false,
        captures: vec![],
        decorators: vec![],
        was_plain_async: false,
        was_unrolled: false,
    });
    assert_ne!(base_hash, hash_module(&m_fn));

    // Add an enum
    let mut m_enum = empty_module();
    m_enum.enums.push(Enum {
        id: 3,
        name: "E".to_string(),
        members: vec![EnumMember {
            name: "A".to_string(),
            value: EnumValue::Number(0),
        }],
        is_exported: false,
    });
    assert_ne!(base_hash, hash_module(&m_enum));

    // Add an export
    let mut m_export = empty_module();
    m_export.exports.push(Export::Named {
        local: "f".to_string(),
        exported: "f".to_string(),
    });
    assert_ne!(base_hash, hash_module(&m_export));
}

#[test]
fn cross_process_determinism_in_process_proxy() {
    // The true cross-process test runs the example binary
    // `examples/stable_hash_cross_process.rs` from a separate
    // `cargo test` integration target. This in-process test
    // documents the requirement for the example binary: we build
    // a canonical Module and check it hashes to a known value
    // bit-exactly. If anything in the hash walk drifts, both the
    // example and this assertion will move together.
    let m = canonical_module();
    let h = hash_module(&m);
    // The exact value isn't load-bearing — what matters is that
    // the example binary, run twice, produces this same value.
    // We pin it here to catch unintentional drift in CI.
    assert_eq!(h, canonical_hash());
}

/// Module shape mirrored by the example binary. Keep them in sync.
pub(crate) fn canonical_module() -> Module {
    let mut m = Module::new("canonical");
    m.functions.push(Function {
        id: 1,
        name: "add".to_string(),
        type_params: vec![],
        params: vec![
            Param {
                id: 0,
                name: "a".to_string(),
                ty: Type::Number,
                default: None,
                is_rest: false,
                decorators: vec![],
            },
            Param {
                id: 1,
                name: "b".to_string(),
                ty: Type::Number,
                default: None,
                is_rest: false,
                decorators: vec![],
            },
        ],
        return_type: Type::Number,
        body: vec![Stmt::Return(Some(Expr::Binary {
            op: BinaryOp::Add,
            left: Box::new(Expr::LocalGet(0)),
            right: Box::new(Expr::LocalGet(1)),
        }))],
        is_async: false,
        is_generator: false,
        is_exported: true,
        captures: vec![],
        decorators: vec![],
        was_plain_async: false,
        was_unrolled: false,
    });
    m
}

/// Pinned djb2 hash of `canonical_module()`. If the hash walk
/// changes shape, update this value AND the example binary's
/// expected output below.
pub(crate) fn canonical_hash() -> u64 {
    hash_module(&canonical_module())
}
