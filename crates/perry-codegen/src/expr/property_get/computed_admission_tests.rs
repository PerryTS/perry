use super::*;
use crate::ClassAccessorNames;
use perry_hir::{types::Type, Class, ClassComputedMember, ClassComputedMemberKind, Function};

fn local_class(id: u32, name: &str) -> Class {
    Class {
        id,
        name: name.to_string(),
        type_params: Vec::new(),
        extends: None,
        extends_name: None,
        native_extends: None,
        extends_expr: None,
        heritage_lexically_shadowed: false,
        fields: Vec::new(),
        constructor: None,
        methods: Vec::new(),
        getters: Vec::new(),
        setters: Vec::new(),
        static_accessor_names: Vec::new(),
        static_accessor_fn_ids: Vec::new(),
        static_fields: Vec::new(),
        static_methods: Vec::new(),
        computed_members: Vec::new(),
        decorators: Vec::new(),
        is_exported: false,
        aliases: Vec::new(),
        is_nested: false,
        alloc_width_hint: 0,
        specialized_from: None,
    }
}

fn names(key: Expr, kind: ClassComputedMemberKind) -> ClassAccessorNames {
    let mut class = local_class(1, "Computed");
    class.computed_members.push(ClassComputedMember {
        key_expr: key,
        kind,
        is_static: false,
        source_order: 0,
        function: Function {
            id: 1,
            name: "computed".into(),
            type_params: vec![],
            params: vec![],
            return_type: Type::Any,
            body: vec![],
            is_async: false,
            is_generator: false,
            is_strict: true,
            is_exported: false,
            captures: vec![],
            decorators: vec![],
            was_plain_async: false,
            was_unrolled: false,
        },
    });
    let mut module = Module::new("computed.ts");
    module.classes.push(class);
    module.init.push(Stmt::Let {
        id: 1,
        name: "key".into(),
        ty: Type::String,
        mutable: false,
        init: Some(Expr::String("price".into())),
    });
    ClassAccessorNames::collect([&module])
}

#[test]
fn computed_accessor_admission_preserves_kind_and_cache_identity() {
    let empty = ClassAccessorNames::default();
    let literal = names(
        Expr::String("price".into()),
        ClassComputedMemberKind::Getter,
    );
    assert!(literal.may_get("price"));
    assert!(!literal.may_get("other"));
    assert!(!literal.may_set("price"));
    let getter = names(Expr::LocalGet(1), ClassComputedMemberKind::Getter);
    assert!(getter.may_get("price"));
    assert!(!getter.may_get("unrelated"));
    assert!(!getter.may_set("price"));
    let setter = names(Expr::LocalGet(1), ClassComputedMemberKind::Setter);
    assert!(!setter.may_get("price"));
    assert!(setter.may_set("price"));
    let opaque = names(Expr::LocalGet(2), ClassComputedMemberKind::Getter);
    assert_eq!(opaque.cache_key(), empty.cache_key());
    let symbol = names(
        Expr::SymbolFor(Box::new(Expr::String("price".into()))),
        ClassComputedMemberKind::Getter,
    );
    assert_eq!(symbol.cache_key(), empty.cache_key());
    let method = names(Expr::LocalGet(1), ClassComputedMemberKind::Method);
    assert_eq!(method.cache_key(), empty.cache_key());
    assert_ne!(getter.cache_key(), empty.cache_key());
    assert_ne!(setter.cache_key(), getter.cache_key());
}

#[test]
fn object_computed_accessors_admit_literal_property_choices_without_symbol_names() {
    let mut module = Module::new("object-computed.ts");
    module.init.push(Stmt::Let {
        id: 1,
        name: "options".into(),
        ty: Type::Any,
        mutable: false,
        init: Some(Expr::Object(vec![(
            "prop".into(),
            Expr::String("answer".into()),
        )])),
    });
    module.init.push(Stmt::Expr(Expr::Call {
        byte_end: 0,
        callee: Box::new(Expr::ExternFuncRef {
            name: "js_object_define_accessor".into(),
            param_types: vec![],
            return_type: Type::Void,
        }),
        args: vec![
            Expr::LocalGet(2),
            Expr::PropertyGet {
                byte_end: 0,
                object: Box::new(Expr::LocalGet(1)),
                property: "prop".into(),
                byte_offset: 0,
            },
            Expr::FuncRef(1),
            Expr::Undefined,
        ],
        type_args: vec![],
        byte_offset: 0,
    }));
    let names = ClassAccessorNames::collect([&module]);
    assert!(names.may_get("answer"));
    assert!(!names.may_get("prop"));
    assert!(!names.may_get("other"));
    assert!(!names.may_set("answer"));
    // Ordinary object getter declarations already have a runtime route and
    // must not broaden the program's computed-name admission.
    if let Stmt::Expr(Expr::Call { args, .. }) = &mut module.init[1] {
        args[1] = Expr::String("ordinary".into());
    }
    assert!(!ClassAccessorNames::collect([&module]).may_get("ordinary"));
}
