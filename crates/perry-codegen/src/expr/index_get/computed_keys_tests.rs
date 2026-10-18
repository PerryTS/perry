use crate::codegen::{ElisionFacts, FactsClass, FactsField, FactsRecv, ProgramFactsView};
use crate::{compile_module, CompileOptions};
use perry_hir::types::Type;
use perry_hir::{Class, ClassField, ClassFieldOrigin, Expr, Function, Module, Param, Stmt};
use std::sync::Arc;

fn ir(key_count: usize, receiver_proven: bool, nullable: bool) -> String {
    let names: Vec<_> = (0..key_count).map(|i| format!("key_{i}")).collect();
    let mut module = Module::new("computed_keys");
    module.classes.push(Class {
        id: 3,
        name: "Record".into(),
        type_params: vec![],
        extends: None,
        extends_name: None,
        native_extends: None,
        extends_expr: None,
        heritage_lexically_shadowed: false,
        fields: names
            .iter()
            .map(|name| ClassField {
                origin: ClassFieldOrigin::Definition,
                name: name.clone(),
                key_expr: None,
                ty: Type::Number,
                init: Some(Expr::Number(1.0)),
                is_private: false,
                is_readonly: false,
                decorators: vec![],
            })
            .collect(),
        constructor: None,
        methods: vec![],
        getters: vec![],
        setters: vec![],
        static_accessor_names: vec![],
        static_accessor_fn_ids: vec![],
        computed_members: vec![],
        static_fields: vec![],
        static_methods: vec![],
        decorators: vec![],
        is_exported: false,
        aliases: vec![],
        is_nested: false,
        alloc_width_hint: 0,
        specialized_from: None,
    });
    let mut index = Expr::String(names.last().unwrap().clone());
    for name in names.iter().rev().skip(1) {
        index = Expr::Conditional {
            condition: Box::new(Expr::LocalGet(2)),
            then_expr: Box::new(Expr::String(name.clone())),
            else_expr: Box::new(index),
        };
    }
    module.functions.push(Function {
        id: 1,
        name: "read".into(),
        type_params: vec![],
        params: vec![
            Param {
                id: 1,
                name: "record".into(),
                ty: Type::Named("Record".into()),
                default: None,
                decorators: vec![],
                is_rest: false,
                arguments_object: None,
            },
            Param {
                id: 2,
                name: "condition".into(),
                ty: Type::Boolean,
                default: None,
                decorators: vec![],
                is_rest: false,
                arguments_object: None,
            },
        ],
        return_type: Type::Any,
        body: vec![Stmt::Return(Some(Expr::IndexGet {
            object: Box::new(Expr::LocalGet(1)),
            index: Box::new(index),
        }))],
        is_async: false,
        is_generator: false,
        is_strict: true,
        is_exported: true,
        captures: vec![],
        decorators: vec![],
        was_plain_async: false,
        was_unrolled: false,
    });
    let mut facts = ElisionFacts::default();
    if receiver_proven {
        facts.locals.insert(
            1,
            FactsRecv {
                class_ids: vec![3],
                nullable,
                checked: false,
            },
        );
        facts.classes.insert(
            3,
            FactsClass {
                display: "Record".into(),
                fixed_layout: true,
                fields: names
                    .iter()
                    .enumerate()
                    .map(|(slot, key)| FactsField {
                        key: key.clone(),
                        slot: slot as u32,
                        readonly: false,
                        kind_mask: 1,
                        ta_stable: false,
                        ta_kind: None,
                    })
                    .collect(),
            },
        );
    }
    let opts = CompileOptions {
        emit_ir_only: true,
        program_facts: ProgramFactsView {
            elision: Arc::new(facts),
            ..Default::default()
        },
        ..Default::default()
    };
    String::from_utf8(compile_module(&module, opts).unwrap()).unwrap()
}

#[test]
fn proven_finite_keys_select_fixed_slots_and_keep_the_exact_miss() {
    for count in [2, 6] {
        let ll = ir(count, true, false);
        assert!(ll.contains("finitekey.slot"), "no finite slot branch: {ll}");
        assert_eq!(
            ll.matches("call i32 @js_jsvalue_equals(").count(),
            count,
            "one comparison per proven key"
        );
        assert!(
            ll.contains("call double @js_dyn_index_get("),
            "the key miss keeps full semantics"
        );
        #[cfg(feature = "llvm-inprocess")]
        crate::testing::verify_ir(&ll, "finite_keys").unwrap();
    }
}

#[test]
fn annotations_nullable_receivers_and_large_key_sets_keep_the_checked_route() {
    for (count, proven, nullable) in [(2, false, false), (2, true, true), (7, true, false)] {
        assert!(!ir(count, proven, nullable).contains("finitekey.slot"));
    }
}
