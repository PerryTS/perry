//! The declared-kind typed-array element lowering (`typed_element`) and the
//! class-guard run (`class_guard_memo`), checked on the emitted IR.
//!
//! What must hold, each asserted on the subject being live:
//! 1. every kind's read and store take the inline arm, whose type-byte test
//!    names exactly that kind's owner brand, and leave through the one shared
//!    runtime exit; BigInt kinds emit only the exit;
//! 2. no generic any-receiver machinery (width dispatch, array-like arms) and
//!    no write barrier remain on those sites;
//! 3. a relational compare of an element read decides the ordered case with
//!    one `fcmp` and gives the unordered case to the runtime helper;
//! 4. a straight-line run reuses one receiver guard, every call inside the run
//!    clears the proof first, and a run broken by a call reuses nothing.
use crate::compile_module;
use perry_hir::types::Type;
use perry_hir::{
    BinaryOp, Class, ClassField, CompareOp, Expr, Function, Module, ModuleInitKind, Param, Stmt,
};

const KINDS: [(&str, u8); 11] = [
    ("Int8Array", 0),
    ("Uint8Array", 1),
    ("Int16Array", 2),
    ("Uint16Array", 3),
    ("Int32Array", 4),
    ("Uint32Array", 5),
    ("Float32Array", 6),
    ("Float64Array", 7),
    ("Uint8ClampedArray", 8),
    ("BigInt64Array", 9),
    ("BigUint64Array", 10),
];

fn field(name: &str, ty: Type) -> ClassField {
    ClassField {
        origin: perry_hir::ClassFieldOrigin::Definition,
        name: name.to_string(),
        key_expr: None,
        ty,
        init: None,
        is_private: false,
        is_readonly: false,
        decorators: Vec::new(),
    }
}

fn holder(fields: Vec<ClassField>) -> Class {
    Class {
        id: 101,
        name: "Holder".to_string(),
        type_params: Vec::new(),
        extends: None,
        extends_name: None,
        native_extends: None,
        extends_expr: None,
        heritage_lexically_shadowed: false,
        fields,
        constructor: None,
        methods: Vec::new(),
        getters: Vec::new(),
        setters: Vec::new(),
        static_accessor_names: Vec::new(),
        static_accessor_fn_ids: Vec::new(),
        computed_members: Vec::new(),
        static_fields: Vec::new(),
        static_methods: Vec::new(),
        decorators: Vec::new(),
        is_exported: false,
        aliases: Vec::new(),
        is_nested: false,
        alloc_width_hint: 0,
        specialized_from: None,
    }
}

fn param(id: u32, name: &str, ty: Type) -> Param {
    Param {
        id,
        name: name.to_string(),
        ty,
        default: None,
        decorators: Vec::new(),
        is_rest: false,
        arguments_object: None,
    }
}

fn function(id: u32, name: &str, params: Vec<Param>, body: Vec<Stmt>) -> Function {
    Function {
        id,
        name: name.to_string(),
        type_params: Vec::new(),
        params,
        return_type: Type::Any,
        body,
        is_async: false,
        is_generator: false,
        is_strict: true,
        is_exported: true,
        captures: Vec::new(),
        decorators: Vec::new(),
        was_plain_async: false,
        was_unrolled: false,
    }
}

fn get(object: u32, property: &str) -> Expr {
    Expr::PropertyGet {
        byte_end: 0,
        byte_offset: 0,
        object: Box::new(Expr::LocalGet(object)),
        property: property.to_string(),
    }
}

fn element(object: u32, property: &str, index: u32) -> Expr {
    Expr::IndexGet {
        byte_offset: 0,
        byte_end: 0,
        object: Box::new(get(object, property)),
        index: Box::new(Expr::LocalGet(index)),
    }
}

fn store(object: u32, property: &str, index: u32, value: Expr) -> Stmt {
    Stmt::Expr(Expr::PutValueSet {
        target: Box::new(get(object, property)),
        key: Box::new(Expr::LocalGet(index)),
        value: Box::new(value),
        receiver: Box::new(get(object, property)),
        strict: true,
        byte_offset: 0,
        byte_end: 0,
    })
}

fn holder_param() -> Param {
    param(1, "h", Type::Named("Holder".to_string()))
}

fn ir(module: &Module) -> String {
    String::from_utf8(
        compile_module(module, super::class_field_barrier_tests::ir_opts())
            .expect("module compiles"),
    )
    .expect("LLVM IR should be UTF-8")
}

/// The body of the compiled function `name`: of its definitions (dispatch
/// stub, `$generic` and specialized clones; never the ABI wrapper), the
/// largest, which is the one carrying the lowered statements.
fn body<'a>(ir: &'a str, name: &str) -> &'a str {
    ir.split("\ndefine")
        .filter(|f| {
            f.lines().next().is_some_and(|head| {
                head.contains("@perry_fn_")
                    && (head.contains(&format!("__{name}("))
                        || head.contains(&format!("__{name}$")))
            })
        })
        .max_by_key(|f| f.len())
        .unwrap_or_else(|| panic!("no function {name} in IR:\n{ir}"))
}

/// `get(h, i) { return h.f[i] }` and `put(h, i, v) { h.f[i] = v }` on a
/// `Holder` whose `f` is declared as `kind`.
fn kind_module(kind: &str) -> Module {
    let mut m = Module::new("typed_element_kind.ts");
    m.classes = vec![holder(vec![field("f", Type::Named(kind.to_string()))])];
    m.functions = vec![
        function(
            10,
            "get",
            vec![holder_param(), param(2, "i", Type::Number)],
            vec![Stmt::Return(Some(element(1, "f", 2)))],
        ),
        function(
            11,
            "put",
            vec![
                holder_param(),
                param(2, "i", Type::Number),
                param(3, "v", Type::Number),
            ],
            vec![store(1, "f", 2, Expr::LocalGet(3))],
        ),
    ];
    m.init_kind = ModuleInitKind::Eager;
    m
}

fn assert_no_generic_element_arms(f: &str, what: &str) {
    for generic in ["tav.width", "arrlike.", "dynarr.", "bytes.view"] {
        assert!(
            !f.contains(generic),
            "{what}: generic element arm `{generic}` is still emitted"
        );
    }
    assert!(
        !f.contains("call void @js_write_barrier"),
        "{what}: typed elements are raw bytes and take no write barrier"
    );
}

#[test]
fn every_kind_selects_its_own_owner_brand_and_one_exit() {
    for (name, kind) in KINDS {
        let ir = ir(&kind_module(name));
        let read = body(&ir, "get");
        let write = body(&ir, "put");
        assert!(
            read.contains("call double @js_dyn_index_get("),
            "{name}: the read lost its shared exit:\n{read}"
        );
        assert!(
            write.contains("call double @js_dyn_index_set_strict("),
            "{name}: the store lost its shared exit"
        );
        assert_no_generic_element_arms(read, name);
        assert_no_generic_element_arms(write, name);
        if kind >= 9 {
            assert!(
                !read.contains("tael.get.load") && !write.contains("tael.set.store"),
                "{name}: BigInt kinds take only the exit"
            );
            continue;
        }
        assert!(read.contains("tael.get.load"), "{name}: no inline read");
        assert!(write.contains("tael.set.store"), "{name}: no inline store");
        let brands: Vec<u8> = if kind == 1 {
            super::u8_buffer_read::U8_BRANDS.to_vec()
        } else {
            vec![super::byte_cell::brand_for_kind(kind)]
        };
        for f in [read, write] {
            for brand in &brands {
                let compare = format!(", {}\n", *brand as i8);
                assert!(
                    f.lines()
                        .any(|l| l.contains("icmp eq i8") && format!("{l}\n").contains(&compare)),
                    "{name}: no type-byte compare against owner brand {brand:#x}:\n{f}"
                );
            }
        }
    }
}

/// Negative control: a proof admitting another kind's brand must turn the
/// brand assertion above red.
#[test]
fn a_wrong_brand_turns_the_kind_test_red() {
    let child = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "expr::typed_element_tests::every_kind_selects_its_own_owner_brand_and_one_exit",
            "--nocapture",
        ])
        .env("PERRY_TYPED_ELEMENT_SABOTAGE", "brand")
        .output()
        .unwrap();
    assert!(String::from_utf8_lossy(&child.stdout).contains("running 1 test"));
    assert!(
        !child.status.success(),
        "a wrong owner brand must fail the kind test"
    );
}

/// `make(h, l, r) { const i = h.size++; h.left[i] = l; h.right[i] = r; return i }`
/// and `check(h, i) { const l = h.left[i]; if (l < 0) return 1; return l }`.
/// With `call_between`, a call statement between the two stores splits make's
/// run.
fn arena_module(call_between: bool) -> Module {
    let mut m = Module::new("typed_element_arena.ts");
    m.classes = vec![holder(vec![
        field("left", Type::Named("Int32Array".to_string())),
        field("right", Type::Named("Int32Array".to_string())),
        field("size", Type::Number),
    ])];
    let mut make = vec![Stmt::Let {
        id: 4,
        name: "i".to_string(),
        ty: Type::Number,
        mutable: false,
        init: Some(Expr::PropertyUpdate {
            object: Box::new(Expr::LocalGet(1)),
            property: "size".to_string(),
            op: BinaryOp::Add,
            prefix: false,
            strict: true,
            byte_offset: 0,
            byte_end: 0,
        }),
    }];
    make.push(store(1, "left", 4, Expr::LocalGet(2)));
    if call_between {
        make.push(Stmt::Expr(Expr::Call {
            byte_end: 0,
            callee: Box::new(Expr::FuncRef(21)),
            args: Vec::new(),
            type_args: Vec::new(),
            byte_offset: 0,
        }));
    }
    make.push(store(1, "right", 4, Expr::LocalGet(3)));
    make.push(Stmt::Return(Some(Expr::LocalGet(4))));
    m.functions = vec![
        function(
            20,
            "make",
            vec![
                holder_param(),
                param(2, "l", Type::Number),
                param(3, "r", Type::Number),
            ],
            make,
        ),
        function(21, "effect", Vec::new(), vec![Stmt::Return(None)]),
        function(
            22,
            "check",
            vec![holder_param(), param(2, "i", Type::Number)],
            vec![
                Stmt::Let {
                    id: 5,
                    name: "l".to_string(),
                    ty: Type::Any,
                    mutable: false,
                    init: Some(element(1, "left", 2)),
                },
                Stmt::If {
                    condition: Expr::Compare {
                        op: CompareOp::Lt,
                        left: Box::new(Expr::LocalGet(5)),
                        right: Box::new(Expr::Integer(0)),
                    },
                    then_branch: vec![Stmt::Return(Some(Expr::Integer(1)))],
                    else_branch: None,
                },
                Stmt::Return(Some(Expr::LocalGet(5))),
            ],
        ),
    ];
    m.init_kind = ModuleInitKind::Eager;
    m
}

#[test]
fn element_compare_decides_the_ordered_case_inline() {
    let ir = ir(&arena_module(false));
    let check = body(&ir, "check");
    assert!(
        check.contains("tael.get.load"),
        "check lost its inline read"
    );
    assert!(
        check.contains("tael.rel.unordered") && check.contains("call double @js_rel_lt("),
        "the element compare must test the unordered case and keep the helper:\n{check}"
    );
    assert!(
        !check.contains("relnum."),
        "the generic relational sequence is still emitted:\n{check}"
    );
}
