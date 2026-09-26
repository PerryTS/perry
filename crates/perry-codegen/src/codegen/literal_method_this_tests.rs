//! #10906: `this` inside a closed-shape object-literal method.
//!
//! HIR lowers `{ a: 1, m() { … this.a … } }` to
//! `new __AnonShape_<hash>(1, <dynamic-this closure>)`. These pin the two
//! halves of the fix: the closure binds its receiver ONCE at entry into a
//! rooted `this` slot (it used to call `js_implicit_this_get_sloppy` at every
//! `this`), and `this.field` nominates the literal's shape class for the
//! guarded class-field path instead of the generic per-site IC.

use crate::{compile_module, CompileOptions};
use perry_hir::types::Type;
use perry_hir::{BinaryOp, Class, ClassField, Expr, Function, Module, Param, Stmt};

const SHAPE: &str = "__AnonShape_00000000000a1906";
const OTHER_SHAPE: &str = "__AnonShape_00000000000b1906";
const METHOD: u32 = 7;
const RECV: u32 = 50;

/// The class `mint_anon_shape_class` synthesizes for `{ a, m }`: shape-only
/// fields and a `this.f = f` constructor.
fn anon_shape(id: u32, name: &str, ctor_id: u32, param_base: u32) -> Class {
    let field_names = ["a", "m"];
    let field = |name: &str| ClassField {
        name: name.to_string(),
        key_expr: None,
        ty: if name == "a" { Type::Number } else { Type::Any },
        init: None,
        is_private: false,
        is_readonly: false,
        decorators: Vec::new(),
    };
    let params: Vec<Param> = field_names
        .iter()
        .enumerate()
        .map(|(i, name)| Param {
            id: param_base + i as u32,
            name: (*name).to_string(),
            ty: Type::Any,
            default: None,
            decorators: Vec::new(),
            is_rest: false,
            arguments_object: None,
        })
        .collect();
    let body = params
        .iter()
        .map(|p| {
            Stmt::Expr(Expr::PropertySet {
                object: Box::new(Expr::This),
                property: p.name.clone(),
                value: Box::new(Expr::LocalGet(p.id)),
            })
        })
        .collect();
    Class {
        id,
        name: name.to_string(),
        type_params: Vec::new(),
        extends: None,
        extends_name: None,
        native_extends: None,
        extends_expr: None,
        heritage_lexically_shadowed: false,
        fields: field_names.iter().map(|name| field(name)).collect(),
        constructor: Some(Function {
            id: ctor_id,
            name: "constructor".to_string(),
            type_params: Vec::new(),
            params,
            return_type: Type::Void,
            body,
            is_async: false,
            is_generator: false,
            is_strict: true,
            is_exported: false,
            captures: Vec::new(),
            decorators: Vec::new(),
            was_plain_async: false,
            was_unrolled: false,
        }),
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

fn this_a() -> Expr {
    Expr::PropertyGet {
        object: Box::new(Expr::This),
        property: "a".to_string(),
        byte_offset: 0,
    }
}

/// `m() { this.a = "s"; return this.a + this.a; }` — three `this` uses, a
/// store and two reads. The string store keeps the body off the typed-f64
/// closure clones, so the public symbol is the ordinary body.
fn method(func_id: u32, is_arrow: bool, is_strict: bool) -> Expr {
    Expr::Closure {
        func_id,
        params: Vec::new(),
        return_type: Type::Any,
        body: vec![
            Stmt::Expr(Expr::PropertySet {
                object: Box::new(Expr::This),
                property: "a".to_string(),
                value: Box::new(Expr::String("s".to_string())),
            }),
            Stmt::Return(Some(Expr::Binary {
                op: BinaryOp::Add,
                left: Box::new(this_a()),
                right: Box::new(this_a()),
            })),
        ],
        captures: Vec::new(),
        mutable_captures: Vec::new(),
        captures_this: false,
        captures_new_target: false,
        enclosing_class: None,
        is_arrow,
        is_async: false,
        is_generator: false,
        is_strict,
    }
}

/// `const o = new SHAPE(1, <closure>)` at module top level.
fn literal_module(closure: Expr) -> Module {
    let mut hir = Module::new("literal_method_this_test");
    hir.classes.push(anon_shape(1, SHAPE, 90, 60));
    hir.init.push(Stmt::Let {
        id: RECV,
        name: "o".to_string(),
        ty: Type::Any,
        mutable: false,
        init: Some(Expr::New {
            class_name: SHAPE.to_string(),
            args: vec![Expr::Number(1.0), closure],
            type_args: Vec::new(),
            byte_offset: 0,
            cap_args_appended: 0,
        }),
    });
    hir
}

fn compile_ir(hir: &Module) -> String {
    let opts = CompileOptions {
        emit_ir_only: true,
        ..Default::default()
    };
    String::from_utf8(compile_module(hir, opts).expect("test module compiles"))
        .expect("LLVM IR is UTF-8")
}

/// The body of the closure's public symbol.
fn closure_body(ir: &str, func_id: u32) -> &str {
    let symbol = format!("@perry_closure_literal_method_this_test__{func_id}(");
    let start = ir
        .find(&format!("define double {symbol}"))
        .or_else(|| ir.find(&format!("define internal double {symbol}")))
        .unwrap_or_else(|| panic!("no body for {symbol} in IR:\n{ir}"));
    let body = &ir[start..];
    &body[..body.find("\n}\n").map_or(body.len(), |end| end + 2)]
}

fn calls_to(body: &str, callee: &str) -> usize {
    body.matches(&format!("@{callee}()")).count()
}

#[test]
fn literal_method_reads_its_receiver_once_at_entry_into_a_rooted_slot() {
    let _shadow = crate::codegen::helpers::NativeRootsPin::shadow();
    let ir = compile_ir(&literal_module(method(METHOD, false, false)));
    let body = closure_body(&ir, METHOD);

    assert_eq!(
        calls_to(body, "js_implicit_this_get_sloppy"),
        1,
        "three `this` uses must share ONE entry read:\n{body}"
    );
    assert_eq!(calls_to(body, "js_implicit_this_get"), 0, "{body}");

    // The receiver lands in a slot the shadow frame roots, so an evacuating
    // minor inside the body rewrites it along with every other root.
    let read = body
        .lines()
        .find(|l| l.contains("@js_implicit_this_get_sloppy()"))
        .unwrap();
    let value = read.split('=').next().unwrap().trim();
    let slot = body
        .lines()
        .find_map(|l| {
            l.trim()
                .strip_prefix(&format!("store double {value}, ptr "))
                .map(str::trim)
        })
        .unwrap_or_else(|| panic!("entry read {value} is never stored:\n{body}"));
    assert!(
        body.lines().any(|l| l.contains("@js_shadow_slot_bind(")
            && l.trim_end().ends_with(&format!("ptr {slot})"))),
        "`this` slot {slot} must be shadow-bound:\n{body}"
    );
}

#[test]
fn strict_literal_method_binds_the_raw_receiver() {
    let ir = compile_ir(&literal_module(method(METHOD, false, true)));
    let body = closure_body(&ir, METHOD);
    assert_eq!(calls_to(body, "js_implicit_this_get"), 1, "{body}");
    assert_eq!(calls_to(body, "js_implicit_this_get_sloppy"), 0, "{body}");
}

#[test]
fn literal_method_this_field_takes_the_guarded_class_field_path() {
    let ir = compile_ir(&literal_module(method(METHOD, false, false)));
    let body = closure_body(&ir, METHOD);
    // The guarded read compares against the literal's shape class and owns a
    // `js_class_field_get_ic` fallback — the path a local binding of the same
    // literal already takes. The generic per-site IC is not used for `this.a`.
    assert!(
        body.contains("@js_class_field_get_ic("),
        "`this.a` must use the guarded class-field read:\n{body}"
    );
    assert!(
        body.contains(&format!(
            "@perry_class_shape_id_literal_method_this_test__{SHAPE}"
        )),
        "the guard must name the literal's shape class:\n{body}"
    );
}

#[test]
fn arrow_keeps_its_per_use_receiver_reads() {
    // An arrow with no captured `this` is outside the change: lexical `this`
    // is `captures_this`'s job, and this body must lower exactly as before.
    let ir = compile_ir(&literal_module(method(METHOD, true, false)));
    let body = closure_body(&ir, METHOD);
    assert_eq!(calls_to(body, "js_implicit_this_get_sloppy"), 3, "{body}");
    assert!(!body.contains("@js_class_field_get_ic("), "{body}");
}

#[test]
fn home_classes_name_only_unambiguous_dynamic_this_methods() {
    let mut hir = literal_module(method(METHOD, false, false));
    hir.classes.push(anon_shape(2, OTHER_SHAPE, 91, 70));
    let new_shape = |class: &str, closure: Expr| {
        Stmt::Expr(Expr::New {
            class_name: class.to_string(),
            args: vec![Expr::Number(2.0), closure],
            type_args: Vec::new(),
            byte_offset: 0,
            cap_args_appended: 0,
        })
    };
    // An arrow, and a closure seen under TWO shape classes, have no single
    // home; a closure nested in another function body is still found.
    hir.init.push(new_shape(SHAPE, method(8, true, false)));
    hir.init.push(new_shape(SHAPE, method(9, false, false)));
    hir.init
        .push(new_shape(OTHER_SHAPE, method(9, false, false)));
    hir.functions.push(Function {
        id: 30,
        name: "make".to_string(),
        type_params: Vec::new(),
        params: Vec::new(),
        return_type: Type::Any,
        body: vec![new_shape(OTHER_SHAPE, method(10, false, false))],
        is_async: false,
        is_generator: false,
        is_strict: true,
        is_exported: false,
        captures: Vec::new(),
        decorators: Vec::new(),
        was_plain_async: false,
        was_unrolled: false,
    });

    let homes = crate::collectors::literal_method_home_classes(&hir);
    assert_eq!(homes.get(&METHOD).map(String::as_str), Some(SHAPE));
    assert_eq!(homes.get(&10).map(String::as_str), Some(OTHER_SHAPE));
    assert!(!homes.contains_key(&8), "an arrow has lexical `this`");
    assert!(
        !homes.contains_key(&9),
        "two shape classes: no single candidate"
    );
}

/// Byte offset of the first line of `ir` that contains every one of `parts`.
fn first_line_with(ir: &str, parts: &[&str]) -> Option<usize> {
    let mut offset = 0;
    for line in ir.split_inclusive('\n') {
        if parts.iter().all(|p| line.contains(p)) {
            return Some(offset);
        }
        offset += line.len();
    }
    None
}

#[test]
fn anon_shape_is_registered_before_its_shape_id_is_minted() {
    // #11420: the module-init ShapeId minted beside a class's keys global
    // records `class_proto_id(cid)`, which answers the ordinary object
    // prototype only once the id is in the anon-shape set. Registered after
    // the mint, every literal allocation failed
    // `try_birth_stamp_preinstalled_shape`'s proto check, was stamped with a
    // second ShapeId, and every guarded `this.a` / `lit.a` read missed.
    //
    // Init ops are chunked into `__perry_init_strings_*_chunkN` functions
    // that are defined, and called, in emission order, so textual order in
    // the IR is execution order.
    let ir = compile_ir(&literal_module(method(METHOD, false, false)));
    let register = first_line_with(&ir, &["call void @js_register_anon_shape_class_id(i32 1)"])
        .unwrap_or_else(|| panic!("the literal's shape class is never registered:\n{ir}"));
    let mint = [
        "@js_object_shape_id_for_class_keys",
        "@js_gc_typed_shape_id_for_keys",
    ]
    .iter()
    .filter_map(|callee| first_line_with(&ir, &["call i32 ", callee]))
    .min()
    .unwrap_or_else(|| panic!("the literal's ShapeId is never minted:\n{ir}"));
    assert!(
        register < mint,
        "js_register_anon_shape_class_id must run before the class ShapeId is minted"
    );
}
