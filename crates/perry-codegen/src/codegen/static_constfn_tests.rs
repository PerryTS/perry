use super::super::CompileOptions;
use super::*;
use perry_hir::types::Type;
use perry_hir::{Function, Param};

static ENV: std::sync::Mutex<()> = std::sync::Mutex::new(());
struct Pin(Option<std::ffi::OsString>);
impl Drop for Pin {
    fn drop(&mut self) {
        match self.0.take() {
            Some(v) => std::env::set_var("PERRY_CONSTFN_SHAPE", v),
            None => std::env::remove_var("PERRY_CONSTFN_SHAPE"),
        }
    }
}
fn closure(id: u32, this: bool) -> Expr {
    Expr::Closure {
        func_id: id,
        params: Vec::new(),
        return_type: Type::Any,
        body: vec![Stmt::Return(Some(Expr::Number(1.0)))],
        captures: Vec::new(),
        mutable_captures: Vec::new(),
        captures_this: this,
        captures_new_target: false,
        enclosing_class: None,
        is_arrow: !this,
        is_async: false,
        is_generator: false,
        is_strict: true,
    }
}
fn fixture() -> Module {
    let mut m = Module::new("cf_final_order");
    m.init.push(Stmt::Expr(Expr::Object(vec![
        ("m".into(), closure(1, false)),
        ("unsafe".into(), closure(2, true)),
        ("collecting".into(), Expr::Object(Vec::new())),
    ])));
    m
}
fn opts(output: &str) -> CompileOptions {
    CompileOptions {
        emit_ir_only: true,
        output_type: output.into(),
        ..Default::default()
    }
}

#[test]
fn default_and_explicit_constfn_modes_discover_and_emit_only_executable_final_shapes() {
    let _lock = ENV.lock().unwrap_or_else(|e| e.into_inner());
    let _pin = Pin(std::env::var_os("PERRY_CONSTFN_SHAPE"));
    let module = fixture();
    assert!(
        module.classes.is_empty(),
        "exercise a classless literal module"
    );
    for setting in [None, Some("1"), Some("0")] {
        match setting {
            Some(value) => std::env::set_var("PERRY_CONSTFN_SHAPE", value),
            None => std::env::remove_var("PERRY_CONSTFN_SHAPE"),
        }
        for output in ["executable", "dylib"] {
            let admitted = output == "executable" && setting != Some("0");
            let births = crate::module_birth_shapes(&module, opts(output)).unwrap();
            assert_eq!(births.len(), usize::from(admitted), "{setting:?}/{output}");
            let mut options = opts(output);
            options.static_shape_ids = super::super::static_shape_ids::assign_static_shape_ids(
                births.iter().map(|birth| &birth.shape),
            )
            .into_iter()
            .collect();
            let ir = String::from_utf8(crate::compile_module(&module, options).unwrap()).unwrap();
            assert_eq!(
                ir.contains("call i64 @js_object_finalize_constfn_static"),
                admitted,
                "{setting:?}/{output}"
            );
            assert_eq!(
                !super::super::static_shape_ids::take_module_static_seeds().is_empty(),
                admitted,
                "{setting:?}/{output}"
            );
        }
    }
}

#[test]
fn final_literal_seed_and_lowering_share_symbols_and_stamp_after_stores_and_patches() {
    let _lock = ENV.lock().unwrap_or_else(|e| e.into_inner());
    let _pin = Pin(std::env::var_os("PERRY_CONSTFN_SHAPE"));
    std::env::set_var("PERRY_CONSTFN_SHAPE", "1");
    let m = fixture();
    let births = crate::module_birth_shapes(&m, opts("executable")).unwrap();
    assert_eq!(
        births.len(),
        1,
        "the collecting literal must have a final producer"
    );
    let shape = &births[0].shape;
    assert_eq!(shape.rep, 3, "only the safe closure lane becomes SPECIAL");
    assert_eq!(
        shape.constfn[0].symbol,
        "perry_closure_cf_final_order__1$info"
    );
    let assigned =
        super::super::static_shape_ids::assign_static_shape_ids(births.iter().map(|b| &b.shape));
    let id = assigned[shape];
    let mut options = opts("executable");
    options.static_shape_ids = assigned.into_iter().collect();
    let ir = String::from_utf8(crate::compile_module(&m, options.clone()).unwrap()).unwrap();
    let body = ir
        .lines()
        .filter(|l| !l.starts_with("declare "))
        .collect::<Vec<_>>()
        .join("\n");
    let stamp = body
        .find("call i64 @js_object_finalize_constfn_static")
        .expect("finalizer was emitted");
    assert!(
        body.rfind("call void @js_object_set_field").unwrap() < stamp,
        "every store must precede promotion"
    );
    assert!(
        body.rfind("call void @js_closure_set_capture_bits")
            .unwrap()
            < stamp,
        "this patches must precede promotion"
    );
    let allocation = body
        .find("call i64 @js_object_alloc_with_shape")
        .expect("ordinary allocation");
    assert!(allocation < stamp);
    assert!(
        !body[allocation..body[allocation..].find('\n').unwrap() + allocation]
            .contains(&format!("i32 {id},")),
        "allocation cannot name the final id"
    );
    assert!(
        ir.contains("hidden constant") && ir.contains("perry_closure_cf_final_order__1$info"),
        "seed body info must be linkable"
    );
    let seeds = super::super::static_shape_ids::take_module_static_seeds();
    assert_eq!(seeds, vec![(id, shape.clone())]);
    let warm = crate::decode_static_seed(&crate::encode_static_seed(id, shape)).unwrap();
    assert_eq!(
        crate::stubs::static_shape_seed_ll(&seeds),
        crate::stubs::static_shape_seed_ll(&[warm]),
        "cold and sidecar replay must have identical seed references"
    );
    std::env::set_var("PERRY_CONSTFN_SHAPE", "0");
    let off = String::from_utf8(crate::compile_module(&m, opts("executable")).unwrap()).unwrap();
    let ordinary_atoms = off.matches("call i64 @js_string_pool_atom").count();
    assert!(
        ordinary_atoms > 0,
        "the ordinary string pool must be exercised"
    );
    assert_eq!(
        ir.matches("call i64 @js_string_pool_atom").count(),
        ordinary_atoms,
        "packed finalizer metadata must not allocate JavaScript string-pool atoms"
    );
    std::env::set_var("PERRY_CONSTFN_SHAPE", "1");
    options.output_type = "dylib".into();
    let unloadable = String::from_utf8(crate::compile_module(&m, options).unwrap()).unwrap();
    assert!(!unloadable.contains("call i64 @js_object_finalize_constfn_static"));
    assert!(super::super::static_shape_ids::take_module_static_seeds().is_empty());
}

#[test]
fn literal_proof_rejects_duplicates_and_rebindable_rest_bodies() {
    let safe = closure(1, false);
    let first = literal_final("p", &[("m".into(), safe.clone())], 0).unwrap();
    let second = literal_final("p", &[("m".into(), closure(2, false))], 0).unwrap();
    assert_ne!(first, second, "exact body symbols split final identity");
    assert_eq!(
        first,
        literal_final("p", &[("m".into(), safe.clone())], 0).unwrap()
    );
    assert!(literal_final(
        "p",
        &[("m".into(), safe.clone()), ("m".into(), safe.clone())],
        0
    )
    .is_none());
    assert!(literal_final("p", &[("__proto__".into(), safe.clone())], 0).is_none());
    assert!(literal_final("p", &[("m".into(), closure(3, true))], 0).is_none());
    let mut rest = safe;
    if let Expr::Closure { params, .. } = &mut rest {
        params.push(Param {
            id: 4,
            name: "args".into(),
            ty: Type::Any,
            default: None,
            decorators: Vec::new(),
            is_rest: true,
            arguments_object: None,
        });
    }
    assert!(literal_final("p", &[("m".into(), rest)], 0).is_none());
}

#[cfg(feature = "llvm-inprocess")]
#[test]
fn final_key_bytes_survive_llvm_decoding_and_codegen_unit_splitting() {
    use crate::types::{I32, PTR, VOID};

    let shape = literal_final(
        "packed_keys",
        &[
            ("mé雪🦀".into(), closure(1, false)),
            ("quote\"slash\\".into(), Expr::Number(1.0)),
        ],
        0,
    )
    .unwrap();
    let expected = "mé雪🦀\0quote\"slash\\\0".as_bytes();
    let entries = entries_symbol("packed_keys", 23);
    let keys = format!("{entries}_keys");
    for target in [
        "x86_64-unknown-linux-gnu",
        "x86_64-pc-windows-msvc",
        "arm64-apple-macosx15.0.0",
    ] {
        let mut module = crate::module::LlModule::new(target);
        module.add_external_global(&shape.constfn[0].symbol, PTR);
        emit_final_entries(&mut module, "packed_keys", &[(shape.clone(), 23)]);
        module.declare_function("consume", VOID, &[PTR, PTR, I32]);
        // Both units use the same layout: ELF/COFF need one owner plus an
        // external reference; Mach-O needs duplicate-safe definitions.
        for name in ["first_factory", "second_factory"] {
            let block = module
                .define_function(name, VOID, vec![])
                .create_block("entry");
            block.call_void(
                "consume",
                &[
                    (PTR, &format!("@{keys}")),
                    (PTR, &format!("@{entries}")),
                    (I32, &expected.len().to_string()),
                ],
            );
            block.ret_void();
        }
        let units = module.render_codegen_units(2);
        assert_eq!(units.len(), 2);
        let context = inkwell::context::Context::create();
        let mut definitions = 0;
        for unit in units {
            let parsed = crate::inprocess::parse_ir_text(&context, &unit, "final_key_bytes")
                .expect("packed metadata and all cross-unit references must parse");
            parsed.verify().expect("valid finalizer metadata unit");
            let global = parsed.get_global(&keys).expect("every consumer needs keys");
            assert!(global.is_constant());
            if let Some(initializer) = global.get_initializer() {
                definitions += 1;
                assert_eq!(
                    initializer.into_array_value().as_const_string().unwrap(),
                    expected,
                    "LLVM must decode exact UTF-8 bytes and one NUL per key"
                );
            }
        }
        assert_eq!(definitions, if target.contains("apple") { 2 } else { 1 });
    }
}

fn empty_class() -> Class {
    Class {
        id: 7,
        name: "__AnonShape_test".into(),
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

#[test]
fn anonymous_record_admission_uses_the_full_constructor_proof() {
    let mut class = empty_class();
    class.constructor = Some(Function {
        id: 8,
        name: "constructor".into(),
        type_params: Vec::new(),
        params: Vec::new(),
        return_type: Type::Any,
        body: vec![Stmt::Return(Some(Expr::Object(Vec::new())))],
        is_async: false,
        is_generator: false,
        is_strict: true,
        is_exported: false,
        captures: Vec::new(),
        decorators: Vec::new(),
        was_plain_async: false,
        was_unrolled: false,
    });
    assert!(anon_props(&class, &[]).is_none());
    class.constructor.as_mut().unwrap().body.clear();
    assert!(
        anon_props(&class, &[]).is_some(),
        "the exact synthetic constructor must be admitted"
    );
    class
        .getters
        .push(("m".into(), class.constructor.clone().unwrap()));
    assert!(
        anon_props(&class, &[]).is_none(),
        "descriptor-bearing constructors stay ordinary"
    );
}

#[test]
fn closed_literal_constructor_emits_a_separate_final_shape() {
    let _lock = ENV.lock().unwrap_or_else(|e| e.into_inner());
    let _pin = Pin(std::env::var_os("PERRY_CONSTFN_SHAPE"));
    std::env::set_var("PERRY_CONSTFN_SHAPE", "1");
    let mut class = empty_class();
    class.fields.push(perry_hir::ClassField {
        name: "m".into(),
        key_expr: None,
        ty: Type::Any,
        init: None,
        is_private: false,
        is_readonly: false,
        decorators: Vec::new(),
    });
    class.constructor = Some(Function {
        id: 8,
        name: "constructor".into(),
        type_params: Vec::new(),
        params: vec![Param {
            id: 9,
            name: "m".into(),
            ty: Type::Any,
            default: None,
            decorators: Vec::new(),
            is_rest: false,
            arguments_object: None,
        }],
        return_type: Type::Void,
        body: vec![Stmt::Expr(Expr::PropertySet {
            object: Box::new(Expr::This),
            property: "m".into(),
            value: Box::new(Expr::LocalGet(9)),
        })],
        is_async: false,
        is_generator: false,
        is_strict: true,
        is_exported: false,
        captures: Vec::new(),
        decorators: Vec::new(),
        was_plain_async: false,
        was_unrolled: false,
    });
    assert!(
        class.is_literal_shape(),
        "record constructor proof must be live"
    );
    let mut m = Module::new("cf_closed_literal");
    m.init.push(Stmt::Expr(Expr::New {
        class_name: class.name.clone(),
        args: vec![closure(1, false)],
        type_args: Vec::new(),
        byte_offset: 0,
        cap_args_appended: 0,
    }));
    m.classes.push(class);
    let births = crate::module_birth_shapes(&m, opts("executable")).unwrap();
    let final_shape = births
        .iter()
        .find(|b| !b.shape.constfn.is_empty())
        .expect("closed literal final content")
        .shape
        .clone();
    assert_eq!(
        births.len(),
        2,
        "ordinary allocation and final content must coexist"
    );
    assert_eq!(final_shape.rep, 3);
    assert!(births
        .iter()
        .any(|b| b.shape.constfn.is_empty() && b.shape.rep == 0));
    let assigned =
        super::super::static_shape_ids::assign_static_shape_ids(births.iter().map(|b| &b.shape));
    let mut options = opts("executable");
    options.static_shape_ids = assigned.into_iter().collect();
    let ir = String::from_utf8(crate::compile_module(&m, options).unwrap()).unwrap();
    assert_eq!(
        ir.matches("call i64 @js_object_finalize_constfn_static")
            .count(),
        1,
        "completed record must call the finalizer once"
    );
    assert!(super::super::static_shape_ids::take_module_static_seeds()
        .iter()
        .any(|(_, s)| s == &final_shape));
}

fn user_class(name: &str, id: u32, method_id: u32) -> Class {
    let mut class = empty_class();
    class.name = name.into();
    class.id = id;
    class.fields.push(perry_hir::ClassField {
        name: format!("m{id}"),
        key_expr: None,
        ty: Type::Any,
        init: Some(closure(method_id, false)),
        is_private: false,
        is_readonly: false,
        decorators: Vec::new(),
    });
    class
}

fn constructor(body: Vec<Stmt>) -> Function {
    Function {
        id: 100,
        name: "constructor".into(),
        type_params: Vec::new(),
        params: Vec::new(),
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
    }
}

#[test]
fn general_class_proof_covers_local_inheritance_and_declines_uncertain_construction() {
    use super::super::static_constfn_class::class_final;
    let base = user_class("Base", 7, 1);
    let mut child = user_class("Child", 8, 2);
    child.extends = Some(7);
    child.extends_name = Some("Base".into());
    child.constructor = Some(constructor(vec![Stmt::Expr(Expr::SuperCall(Vec::new()))]));
    let parents = HashMap::from([("Base".into(), &base)]);
    let shape = class_final("p", &child, &parents, 0, 88).unwrap();
    assert_eq!(shape.proto, BirthProto::Class(88));
    assert_eq!(shape.keys, b"m7\0m8\0");
    assert_eq!(shape.rep, 15);
    assert_eq!(
        shape.constfn.iter().map(|e| e.slot).collect::<Vec<_>>(),
        vec![0, 1]
    );
    assert_eq!(shape.constfn[0].symbol, "perry_closure_p__1$info");
    assert_eq!(
        class_final("p", &child, &HashMap::new(), 0, 88).unwrap_err(),
        "unresolved named heritage"
    );
    child.constructor = Some(constructor(vec![Stmt::Return(None)]));
    assert_eq!(
        class_final("p", &child, &parents, 0, 88).unwrap_err(),
        "constructor control flow or property mutation"
    );
    child.constructor = Some(constructor(vec![Stmt::Return(Some(Expr::Object(
        Vec::new(),
    )))]));
    assert!(class_final("p", &child, &parents, 0, 88).is_err());
    child.constructor = None;
    child.fields[0].key_expr = Some(Expr::String("m8".into()));
    assert_eq!(
        class_final("p", &child, &parents, 0, 88).unwrap_err(),
        "nonpublic or captured field layout"
    );
    child.fields[0].key_expr = None;
    child.fields[0].is_private = true;
    assert!(class_final("p", &child, &parents, 0, 88).is_err());
    child.fields[0].is_private = false;
    child.fields[0].name = "m7".into();
    assert_eq!(
        class_final("p", &child, &parents, 0, 88).unwrap_err(),
        "no safe closure initializer or uncertain slot order"
    );
    child.fields[0].name = "m8".into();
    child.extends_expr = Some(Box::new(Expr::GlobalGet(1)));
    assert_eq!(
        class_final("p", &child, &parents, 0, 88).unwrap_err(),
        "unresolved or external heritage"
    );
    let mut mutated = base.clone();
    mutated.constructor = Some(constructor(vec![Stmt::Expr(Expr::PropertySet {
        object: Box::new(Expr::This),
        property: "added".into(),
        value: Box::new(Expr::Number(1.0)),
    })]));
    assert_eq!(
        class_final("p", &mutated, &HashMap::new(), 0, 77).unwrap_err(),
        "constructor control flow or property mutation"
    );
}

#[test]
fn general_class_records_follow_registration_and_finalize_the_completed_result() {
    let _lock = ENV.lock().unwrap_or_else(|e| e.into_inner());
    let _pin = Pin(std::env::var_os("PERRY_CONSTFN_SHAPE"));
    std::env::set_var("PERRY_CONSTFN_SHAPE", "1");
    let mut class = user_class("User", 7, 1);
    class.fields.push(perry_hir::ClassField {
        name: "effect".into(),
        key_expr: None,
        ty: Type::Any,
        init: Some(Expr::Call {
            callee: Box::new(Expr::FuncRef(90)),
            args: Vec::new(),
            type_args: Vec::new(),
            byte_offset: 0,
        }),
        is_private: false,
        is_readonly: false,
        decorators: Vec::new(),
    });
    class.constructor = Some(constructor(vec![Stmt::Expr(Expr::PropertySet {
        object: Box::new(Expr::This),
        property: "effect".into(),
        value: Box::new(Expr::Object(Vec::new())),
    })]));
    let mut m = Module::new("cf_user_class");
    let mut effect = constructor(vec![
        Stmt::Expr(Expr::Object(Vec::new())),
        Stmt::Return(Some(Expr::Number(1.0))),
    ]);
    effect.id = 90;
    effect.name = "collect".into();
    effect.return_type = Type::Any;
    m.functions.push(effect);
    m.classes.push(class);
    m.init.push(Stmt::Expr(Expr::New {
        class_name: "User".into(),
        args: Vec::new(),
        type_args: Vec::new(),
        byte_offset: 0,
        cap_args_appended: 0,
    }));
    let births = crate::module_birth_shapes(&m, opts("executable")).unwrap();
    let ordinary = births.iter().find(|b| b.shape.constfn.is_empty()).unwrap();
    let final_content = births.iter().find(|b| !b.shape.constfn.is_empty()).unwrap();
    assert_eq!(births.len(), 2);
    assert_eq!(ordinary.shape.keys, final_content.shape.keys);
    assert_eq!(ordinary.shape.proto, final_content.shape.proto);
    assert_eq!(ordinary.shape.rep, 0);
    assert_eq!(final_content.shape.rep, 3);
    let assigned =
        super::super::static_shape_ids::assign_static_shape_ids(births.iter().map(|b| &b.shape));
    let final_id = assigned[&final_content.shape];
    let mut options = opts("executable");
    options.static_shape_ids = assigned.into_iter().collect();
    let ir = String::from_utf8(crate::compile_module(&m, options.clone()).unwrap()).unwrap();
    let calls = ir
        .lines()
        .filter(|l| l.contains(" call "))
        .collect::<Vec<_>>();
    let mint = calls
        .iter()
        .position(|l| l.contains("@js_object_final_shape_id_for_class_keys_static_constfn"))
        .unwrap();
    assert!(
        calls
            .iter()
            .rposition(|l| l.contains("@js_register_class_name"))
            .unwrap()
            < mint
    );
    let mint_line = calls[mint];
    assert!(
        mint_line.contains(&format!("i32 {final_id}, i64 3")),
        "{mint_line}"
    );
    assert!(mint_line.contains("ptr @perry_constfn_final_cf_user_class__"));
    assert!(ir.contains("perry_closure_cf_user_class__1$info = hidden constant"));
    let stamp = calls
        .iter()
        .position(|l| l.contains("@js_object_finalize_constfn_static"))
        .unwrap();
    let override_pos = calls
        .iter()
        .position(|l| l.contains("@js_ctor_return_override"))
        .unwrap();
    assert!(
        override_pos < stamp,
        "completed receiver selection must precede finalization"
    );
    assert_eq!(
        calls
            .iter()
            .filter(|l| l.contains("@js_object_finalize_constfn_static"))
            .count(),
        1
    );
    for line in calls.iter().filter(|l| l.contains("@js_object_alloc")) {
        assert!(
            !line.contains(&format!("i32 {final_id}")),
            "allocation used final shape: {line}"
        );
    }
    // Class facts are minted by the cached defining object after registration,
    // while startup seed sidecars remain reserved for literal prototypes.
    assert!(super::super::static_shape_ids::take_module_static_seeds()
        .iter()
        .all(|(_, shape)| shape.proto == BirthProto::Literal));
    let warm = String::from_utf8(crate::compile_module(&m, options.clone()).unwrap()).unwrap();
    assert_eq!(ir, warm);
    options.output_type = "dylib".into();
    let unloadable = String::from_utf8(crate::compile_module(&m, options).unwrap()).unwrap();
    assert!(
        !unloadable.contains("call i32 @js_object_final_shape_id_for_class_keys_static_constfn")
    );
    assert!(!unloadable.contains("call i64 @js_object_finalize_constfn_static"));
}

#[test]
fn class_replacement_returns_never_produce_a_final_record() {
    let _lock = ENV.lock().unwrap_or_else(|e| e.into_inner());
    let _pin = Pin(std::env::var_os("PERRY_CONSTFN_SHAPE"));
    std::env::set_var("PERRY_CONSTFN_SHAPE", "1");
    let mut class = user_class("Replacement", 7, 1);
    class.constructor = Some(constructor(vec![Stmt::Return(Some(Expr::Object(
        Vec::new(),
    )))]));
    let mut m = Module::new("cf_replacement");
    m.init.push(Stmt::Expr(Expr::Object(vec![(
        "unrelated".into(),
        closure(2, false),
    )])));
    m.classes.push(class);
    m.init.push(Stmt::Expr(Expr::New {
        class_name: "Replacement".into(),
        args: Vec::new(),
        type_args: Vec::new(),
        byte_offset: 0,
        cap_args_appended: 0,
    }));
    let births = crate::module_birth_shapes(&m, opts("executable")).unwrap();
    assert!(births
        .iter()
        .all(|b| b.shape.constfn.is_empty() || b.shape.proto == BirthProto::Literal));
    assert!(
        births.iter().any(|b| !b.shape.constfn.is_empty()),
        "unrelated final ids keep the finalizer supplier active"
    );
    let mut options = opts("executable");
    options.static_shape_ids =
        super::super::static_shape_ids::assign_static_shape_ids(births.iter().map(|b| &b.shape))
            .into_iter()
            .collect();
    let ir = String::from_utf8(crate::compile_module(&m, options).unwrap()).unwrap();
    assert_eq!(
        ir.matches("call i64 @js_object_finalize_constfn_static")
            .count(),
        1,
        "only the unrelated literal is finalized; the class allocation stays ordinary"
    );
}

#[test]
fn default_derived_class_finalizes_inherited_and_own_closure_fields() {
    let _lock = ENV.lock().unwrap_or_else(|e| e.into_inner());
    let _pin = Pin(std::env::var_os("PERRY_CONSTFN_SHAPE"));
    std::env::set_var("PERRY_CONSTFN_SHAPE", "1");
    let base = user_class("Base", 7, 1);
    let mut child = user_class("Child", 8, 2);
    child.extends = Some(base.id);
    child.extends_name = Some(base.name.clone());
    let mut m = Module::new("cf_user_inherit");
    m.classes.extend([base, child]);
    m.init.push(Stmt::Expr(Expr::New {
        class_name: "Child".into(),
        args: Vec::new(),
        type_args: Vec::new(),
        byte_offset: 0,
        cap_args_appended: 0,
    }));
    let births = crate::module_birth_shapes(&m, opts("executable")).unwrap();
    assert_eq!(
        births
            .iter()
            .filter(|b| !b.shape.constfn.is_empty())
            .count(),
        2
    );
    let child_final = &births
        .iter()
        .find(|b| !b.shape.constfn.is_empty() && b.shape.key_count == 2)
        .unwrap()
        .shape;
    assert_eq!(child_final.keys, b"m7\0m8\0");
    assert_eq!(child_final.rep, 15);
    let assigned =
        super::super::static_shape_ids::assign_static_shape_ids(births.iter().map(|b| &b.shape));
    let id = assigned[child_final];
    let allocation_ids =
        super::super::static_shape_ids::ProgramClassShapeIds::from_births(&births, &assigned);
    assert!(
        allocation_ids
            .0
            .values()
            .all(|b| b.shape.constfn.is_empty()),
        "general final records cannot enter the class allocation supplier"
    );
    let mut options = opts("executable");
    options.static_shape_ids = assigned.into_iter().collect();
    let ir = String::from_utf8(crate::compile_module(&m, options).unwrap()).unwrap();
    let finalizers = ir
        .lines()
        .filter(|l| l.contains("call i64 @js_object_finalize_constfn_static"))
        .collect::<Vec<_>>();
    assert_eq!(finalizers.len(), 1);
    assert!(finalizers[0].contains(&format!("i32 {id},")) && finalizers[0].contains("i64 15,"));
    assert_eq!(
        ir.matches("call i32 @js_object_final_shape_id_for_class_keys_static_constfn")
            .count(),
        2
    );
}
