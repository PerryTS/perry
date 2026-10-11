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
fn computed_accessors_take_the_named_guarded_arms() {
    let emit = |names: ClassAccessorNames| {
        let mut module = module_reading("price");
        module.init.push(Stmt::Expr(Expr::PropertySet {
            object: Box::new(Expr::LocalGet(1)),
            property: "price".into(),
            value: Box::new(Expr::Number(1.0)),
        }));
        let mut opts = ir_opts(false, None);
        opts.program_class_accessor_names = Some(std::sync::Arc::new(names));
        String::from_utf8(compile_module(&module, opts).unwrap()).unwrap()
    };
    let getter = emit(names(Expr::LocalGet(1), ClassComputedMemberKind::Getter));
    assert!(getter.contains("pic.acc.empty"));
    assert!(!getter.contains("put.pic.acc"));
    let setter = emit(names(Expr::LocalGet(1), ClassComputedMemberKind::Setter));
    assert!(!setter.contains("pic.acc.empty"));
    assert!(setter.contains("put.pic.acc"));
}

#[test]
fn presence_uses_shared_layout_only_on_lp64() {
    let mut module = module_reading("price");
    module.init.push(Stmt::Expr(Expr::In {
        property: Box::new(Expr::String("price".into())),
        object: Box::new(Expr::LocalGet(1)),
    }));
    let emit = |target| {
        let mut opts = ir_opts(false, None);
        opts.target = target;
        compile_module(&module, opts)
    };
    let native = String::from_utf8(emit(None).unwrap()).unwrap();
    let blocks = super::tower_blocks(&native);
    let (_, body) = super::tower_block(&blocks, "in.pic.guard");
    let guard = body.join("\n");
    assert_eq!(guard.matches("icmp eq i64").count(), 1);
    assert!(
        !guard.contains("load i8"),
        "no kind/flag/presence guards at the site"
    );
    assert!(!guard.contains("load i16"));
    assert!(native.contains("call double @js_in_operator_presence_ic"));
    // The backend currently rejects ILP32 before expression lowering. Keep
    // that public gate explicit instead of pretending a full wasm module can
    // already be compiled to exercise this operation's outlined path.
    let wasm = emit(Some("wasm32-wasip1".into())).unwrap_err();
    assert!(wasm.to_string().contains("32-bit pointers"));
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
        callee: Box::new(Expr::ExternFuncRef {
            name: "js_object_define_accessor".into(),
            param_types: vec![],
            return_type: Type::Void,
        }),
        args: vec![
            Expr::LocalGet(2),
            Expr::PropertyGet {
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
