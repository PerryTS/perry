use super::*;
use perry_hir::{Class, ClassField};

fn property(object: Expr, name: &str) -> Expr {
    Expr::PropertyGet {
        object: Box::new(object),
        property: name.into(),
        byte_offset: 0,
    }
}

fn store(object: Expr, name: &str, value: Expr) -> Stmt {
    Stmt::Expr(Expr::PropertySet {
        object: Box::new(object),
        property: name.into(),
        value: Box::new(value),
    })
}

fn scalar_loop(poison: Option<Expr>) -> String {
    let name = "__AnonShape_10769";
    let params: Vec<_> = ["x", "y"]
        .iter()
        .enumerate()
        .map(|(index, field)| Param {
            id: 100 + index as u32,
            name: field.to_string(),
            ty: Type::Any,
            default: None,
            decorators: Vec::new(),
            is_rest: false,
            arguments_object: None,
        })
        .collect();
    let mut ctor = probe_module(
        "ctor",
        params,
        vec![
            store(Expr::This, "x", Expr::LocalGet(100)),
            store(Expr::This, "y", Expr::LocalGet(101)),
        ],
    )
    .functions
    .remove(0);
    ctor.id = 2;
    ctor.is_strict = true;
    ctor.return_type = Type::Void;
    let class = Class {
        id: 1,
        name: name.into(),
        type_params: Vec::new(),
        extends: None,
        extends_name: None,
        native_extends: None,
        extends_expr: None,
        heritage_lexically_shadowed: false,
        fields: ["x", "y"]
            .iter()
            .map(|field| ClassField {
                name: field.to_string(),
                key_expr: None,
                ty: Type::Number,
                init: None,
                is_private: false,
                is_readonly: false,
                decorators: Vec::new(),
            })
            .collect(),
        constructor: Some(ctor),
        methods: Vec::new(),
        getters: Vec::new(),
        setters: Vec::new(),
        static_fields: Vec::new(),
        static_methods: Vec::new(),
        computed_members: Vec::new(),
        decorators: Vec::new(),
        is_exported: false,
        aliases: Vec::new(),
        is_nested: false,
        alloc_width_hint: 0,
        specialized_from: None,
        static_accessor_names: Vec::new(),
        static_accessor_fn_ids: Vec::new(),
    };
    assert!(class.is_literal_shape());
    let mut body = vec![
        Stmt::Let {
            id: 1,
            name: "o".into(),
            ty: Type::Named(name.into()),
            mutable: false,
            init: Some(Expr::New {
                class_name: name.into(),
                args: vec![Expr::Integer(1), Expr::Integer(2)],
                type_args: Vec::new(),
                byte_offset: 0,
                cap_args_appended: 0,
            }),
        },
        number_let(2, "sum", true, Expr::Integer(0)),
    ];
    if let Some(value) = poison {
        body.push(Stmt::If {
            condition: Expr::LocalGet(99),
            then_branch: vec![store(Expr::LocalGet(1), "x", value)],
            else_branch: None,
        });
    }
    body.push(Stmt::For {
        init: Some(Box::new(number_let(3, "i", true, Expr::Integer(0)))),
        condition: Some(Expr::Compare {
            op: CompareOp::Lt,
            left: Box::new(Expr::LocalGet(3)),
            right: Box::new(Expr::Integer(64)),
        }),
        update: Some(Expr::Update {
            id: 3,
            op: UpdateOp::Increment,
            prefix: false,
        }),
        body: vec![
            store(Expr::LocalGet(1), "y", Expr::LocalGet(3)),
            Stmt::Expr(Expr::LocalSet(
                2,
                Box::new(Expr::Binary {
                    op: BinaryOp::Add,
                    left: Box::new(Expr::LocalGet(2)),
                    right: Box::new(property(Expr::LocalGet(1), "x")),
                }),
            )),
        ],
    });
    body.push(Stmt::Return(Some(Expr::Binary {
        op: BinaryOp::Add,
        left: Box::new(Expr::LocalGet(2)),
        right: Box::new(property(Expr::LocalGet(1), "y")),
    })));
    let mut module = probe_module(
        "scalar_own_read.ts",
        vec![Param {
            id: 99,
            name: "branch".into(),
            ty: Type::Boolean,
            default: None,
            decorators: Vec::new(),
            is_rest: false,
            arguments_object: None,
        }],
        body,
    );
    module.classes.push(class);
    let ir = emitted_ir(module);
    // Generic bodies have internal linkage; the public definition may only
    // dispatch to one. Match the definition's name independently of linkage.
    let start = [
        "@perry_fn_scalar_own_read_ts__probe$generic(",
        "@perry_fn_scalar_own_read_ts__probe(",
    ]
    .iter()
    .find_map(|name| {
        ir.match_indices("define ")
            .find(|(start, _)| ir[*start..].lines().next().unwrap().contains(*name))
            .map(|(start, _)| start)
    })
    .expect("probe body definition");
    let end = start + ir[start..].find("\n}").unwrap();
    ir[start..end].to_string()
}

#[test]
fn scalar_own_read_preserves_whole_write_numeric_proof() {
    let body = scalar_loop(None);
    assert!(
        body.contains("fadd"),
        "the assertion must inspect the loop body:\n{body}"
    );
    for helper in [
        "@js_number_coerce(",
        "@js_dynamic_string_or_number_add(",
        "@js_string_addref_if_heap_string(",
        "@js_gc_loop_safepoint(",
    ] {
        assert!(
            !body.contains(helper),
            "numeric scalar loop retains {helper}:\n{body}"
        );
    }
}

#[test]
fn scalar_own_read_declines_nonnumber_branch_writes() {
    for value in [
        Expr::String("s".into()),
        Expr::Undefined,
        Expr::Null,
        Expr::Object(vec![("n".into(), Expr::Integer(7))]),
    ] {
        let body = scalar_loop(Some(value));
        assert!(
            body.contains("@js_dynamic_string_or_number_add("),
            "a nonnumber write must poison the field despite its annotation:\n{body}"
        );
        assert!(
            body.contains("@js_gc_loop_safepoint("),
            "potentially collecting loop lost its poll:\n{body}"
        );
    }
}
