//! #8774 exact-shape ordinary-argument clone ratchets.

use crate::{compile_module, AppMetadata, CompileOptions};
use perry_hir::types::Type;
use perry_hir::{Class, ClassField, Expr, Function, Module, Param, Stmt};

fn opts() -> CompileOptions {
    CompileOptions {
        emit_ir_only: true,
        is_entry_module: true,
        output_type: "executable".to_string(),
        app_metadata: AppMetadata::default(),
        ..CompileOptions::default()
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

fn class(id: u32, name: &str, fields: Vec<&str>, methods: Vec<Function>) -> Class {
    Class {
        id,
        name: name.to_string(),
        type_params: Vec::new(),
        extends: None,
        extends_name: None,
        native_extends: None,
        extends_expr: None,
        heritage_lexically_shadowed: false,
        fields: fields
            .into_iter()
            .map(|name| ClassField {
                name: name.to_string(),
                key_expr: None,
                ty: Type::Any,
                init: None,
                is_private: false,
                is_readonly: false,
                decorators: Vec::new(),
            })
            .collect(),
        constructor: None,
        methods,
        getters: Vec::new(),
        setters: Vec::new(),
        static_accessor_names: Vec::new(),
        static_accessor_fn_ids: Vec::new(),
        static_fields: Vec::new(),
        static_methods: Vec::new(),
        computed_members: Vec::new(),
        decorators: Vec::new(),
        is_exported: false,
        is_nested: false,
        alloc_width_hint: 0,
        specialized_from: None,
        aliases: Vec::new(),
    }
}

fn field_get(local: u32, property: &str) -> Expr {
    Expr::PropertyGet {
        object: Box::new(Expr::LocalGet(local)),
        property: property.to_string(),
        byte_offset: 0,
    }
}

fn fixture() -> Module {
    let entity_param = 20;
    let read = function(
        200,
        "read",
        vec![param(
            entity_param,
            "entity",
            Type::Named("Entity".to_string()),
        )],
        vec![Stmt::Expr(field_get(entity_param, "id"))],
    );
    let mut module = Module::new("argument_shape_clone.ts");
    module.classes.push(class(1, "Entity", vec!["id"], vec![]));
    module
        .classes
        .push(class(2, "Registry", vec![], vec![read]));
    module.init.extend([
        Stmt::Let {
            id: 10,
            name: "registry".to_string(),
            ty: Type::Named("Registry".to_string()),
            mutable: false,
            init: Some(Expr::New {
                class_name: "Registry".to_string(),
                args: Vec::new(),
                type_args: Vec::new(),
                byte_offset: 0,
                cap_args_appended: 0,
            }),
        },
        Stmt::Let {
            id: 11,
            name: "entity".to_string(),
            ty: Type::Named("Entity".to_string()),
            mutable: false,
            init: Some(Expr::New {
                class_name: "Entity".to_string(),
                args: Vec::new(),
                type_args: Vec::new(),
                byte_offset: 0,
                cap_args_appended: 0,
            }),
        },
        Stmt::Expr(Expr::Call {
            callee: Box::new(Expr::PropertyGet {
                object: Box::new(Expr::LocalGet(10)),
                property: "read".to_string(),
                byte_offset: 0,
            }),
            args: vec![Expr::LocalGet(11)],
            type_args: Vec::new(),
            byte_offset: 0,
        }),
    ]);
    module
}

fn function_ir<'a>(ir: &'a str, marker: &str) -> &'a str {
    let start = ir
        .match_indices("define ")
        .find(|(index, _)| {
            let end = ir[*index..]
                .find('\n')
                .map(|offset| index + offset)
                .unwrap_or(ir.len());
            ir[*index..end].contains(marker)
        })
        .map(|(index, _)| index)
        .unwrap_or_else(|| panic!("missing function {marker}:\n{ir}"));
    let end = ir[start..]
        .find("\n}")
        .map(|offset| start + offset)
        .expect("function terminator");
    &ir[start..end]
}

#[test]
fn guarded_call_routes_to_shadow_rooted_direct_field_clone() {
    // Native statepoint roots are the host default.  Pin the shadow-stack
    // lowering because this assertion specifically ratchets the portable
    // tagged-slot fallback required by the clone ABI.
    let _shadow = crate::codegen::helpers::NativeRootsPin::shadow();
    let ir = String::from_utf8(compile_module(&fixture(), opts()).expect("module compiles"))
        .expect("LLVM IR is UTF-8");
    let clone_name = "perry_method_argument_shape_clone_ts__Registry__read$pshape_args";
    let clone = function_ir(&ir, &format!("@{clone_name}("));
    let generic = function_ir(
        &ir,
        "@perry_method_argument_shape_clone_ts__Registry__read(",
    );

    assert!(
        ir.contains(&format!("call double @{clone_name}(")),
        "the exact contained call site must route to the argument clone:\n{ir}"
    );
    assert!(
        !ir.contains("pshape_arg.fallback")
            && ir.contains("call double @perry_method_argument_shape_clone_ts__Registry__read("),
        "fresh provenance must elide the redundant argument guard while other receiver routes retain the ordinary body:\n{ir}"
    );
    assert!(
        clone.contains("@js_shadow_slot_bind(")
            && clone.find("@js_shadow_slot_bind(")
                < [
                    clone.find("getelementptr double"),
                    clone.find("inttoptr i64"),
                ]
                .into_iter()
                .flatten()
                .min(),
        "the tagged parameter slot must be bound before fixed-offset access:\n{clone}"
    );
    assert!(
        clone.contains("inttoptr i64") && clone.contains("getelementptr double"),
        "the clone must use direct declared-field addressing:\n{clone}"
    );
    assert!(
        !clone.contains("js_typed_feedback_class_field_get_guard")
            && !clone.contains("shape_descriptor_by_id"),
        "the clone fast body must not rebuild the field IC diamond:\n{clone}"
    );
    assert!(
        generic.contains("js_typed_feedback_class_field_get_guard")
            || generic.contains("js_object_get_field"),
        "the generic fallback must retain guarded field semantics:\n{generic}"
    );
}

#[test]
fn routed_argument_is_a_contained_ptr_shape_win_in_the_opt_report() {
    let session = crate::opt_report::test_support::Session::start();
    compile_module(&fixture(), opts()).expect("module compiles");
    let entries = session.entries();
    let entity_entries: Vec<_> = entries
        .iter()
        .filter(|entry| entry.name == "entity" && entry.local_id == Some(11))
        .collect();

    assert!(
        entity_entries
            .iter()
            .any(|entry| entry.outcome == crate::opt_report::Outcome::Selected),
        "the guarded argument route must preserve the caller's Ptr<Shape> fact: {entries:#?}"
    );
    assert!(
        entity_entries.iter().all(|entry| {
            entry.outcome != crate::opt_report::Outcome::Denied
                || !entry
                    .reason
                    .as_deref()
                    .unwrap_or("")
                    .contains("passed as a call argument")
        }),
        "the retired call-argument denial must not survive a selected clone route: {entries:#?}"
    );
}

#[test]
fn unannotated_parameter_uses_the_runtime_validated_class_overlay() {
    let mut module = fixture();
    module.classes[1].methods[0].params[0].ty = Type::Any;
    let ir = String::from_utf8(compile_module(&module, opts()).expect("module compiles"))
        .expect("LLVM IR is UTF-8");
    let clone = function_ir(&ir, "Registry__read$pshape_args(");
    assert!(
        clone.contains("getelementptr double") && clone.contains("inttoptr i64"),
        "the exact runtime guard must recover the class layout for an unannotated parameter:\n{clone}"
    );
    assert!(
        !clone.contains("js_typed_feedback_class_field_get_guard")
            && !clone.contains("shape_descriptor_by_id"),
        "the unannotated clone must not rebuild the field IC diamond:\n{clone}"
    );
}

#[test]
fn ambiguous_unannotated_field_signature_stays_generic() {
    let mut module = fixture();
    module.classes[1].methods[0].params[0].ty = Type::Any;
    module
        .classes
        .push(class(3, "OtherEntity", vec!["id"], vec![]));
    let ir = String::from_utf8(compile_module(&module, opts()).expect("module compiles"))
        .expect("LLVM IR is UTF-8");
    assert!(
        !ir.contains("Registry__read$pshape_args"),
        "an unannotated field signature matching multiple classes must not nominate either:\n{ir}"
    );
}

#[test]
fn aliased_or_reassigned_parameter_does_not_get_a_clone() {
    let mut module = fixture();
    let method = &mut module.classes[1].methods[0];
    method.body.insert(
        0,
        Stmt::Expr(Expr::LocalSet(
            method.params[0].id,
            Box::new(Expr::Undefined),
        )),
    );
    let ir = String::from_utf8(compile_module(&module, opts()).expect("module compiles"))
        .expect("LLVM IR is UTF-8");
    assert!(
        !ir.contains("Registry__read$pshape_args"),
        "a reassigned parameter must keep only generic semantics:\n{ir}"
    );
}

fn define_property(target: Expr) -> Stmt {
    Stmt::Expr(Expr::ObjectDefineProperty(
        Box::new(target),
        Box::new(Expr::String("unrelated".to_string())),
        Box::new(Expr::Object(vec![("value".to_string(), Expr::Number(1.0))])),
    ))
}

/// An unrelated §5.2 barrier does not suppress the route, but it DOES make the
/// runtime guard load-bearing.
///
/// The route-only proof reaches this module by bypassing rule 5's module-wide
/// barrier kill. That kill is the belt-and-braces backstop against blind spots
/// in the containment walk (`ptr_shape.rs` rule 5), so with it bypassed the
/// entry guard is the only thing left that can observe a reshaped argument.
/// Eliding it here would leave the clone's fixed-offset reads with no check at
/// all in exactly the modules whose barriers the analysis refuses to attribute.
#[test]
fn unrelated_module_shape_barrier_keeps_guarded_argument_route() {
    let mut module = fixture();
    module.init.insert(0, define_property(Expr::Object(vec![])));
    let ir = String::from_utf8(compile_module(&module, opts()).expect("module compiles"))
        .expect("LLVM IR is UTF-8");
    let clone_name = "perry_method_argument_shape_clone_ts__Registry__read$pshape_args";

    assert!(
        ir.contains(&format!("call double @{clone_name}(")),
        "an unrelated barrier must not suppress the exact guarded route:\n{ir}"
    );
    assert!(
        ir.contains("pshape_arg.fallback")
            && ir.contains("call double @perry_method_argument_shape_clone_ts__Registry__read("),
        "a route that bypassed the module-wide barrier kill must keep its \
         runtime guard and its generic fallback:\n{ir}"
    );
}

#[test]
fn barrier_targeting_argument_stays_on_generic_route() {
    let mut module = fixture();
    module.init.insert(2, define_property(Expr::LocalGet(11)));
    let ir = String::from_utf8(compile_module(&module, opts()).expect("module compiles"))
        .expect("LLVM IR is UTF-8");
    let clone_name = "perry_method_argument_shape_clone_ts__Registry__read$pshape_args";

    assert!(
        !ir.contains(&format!("call double @{clone_name}(")),
        "a value reshaped before the call must not receive a containment route:\n{ir}"
    );
}

/// A clone that publishes its parameter gets NO caller-side route.
///
/// `PrefixContainedParamUse` proves a temporal property — the licensed field
/// reads happen before the body's first bare use of the parameter. The fact
/// map that would carry a caller-side route is keyed by local id and is
/// therefore flow-INSENSITIVE: a fact kept past a publishing call is consulted
/// again at every later route site for the same local, including sites that
/// run once the alias exists. A per-local map cannot express "before", so the
/// only sound reading is that no caller-side containment fact survives such a
/// call at all.
#[test]
fn publishing_clone_gets_no_caller_side_route() {
    let mut module = fixture();
    let param_id = module.classes[1].methods[0].params[0].id;
    module.classes[1].methods[0]
        .body
        .push(Stmt::Return(Some(Expr::LocalGet(param_id))));

    let session = crate::opt_report::test_support::Session::start();
    let ir = String::from_utf8(compile_module(&module, opts()).expect("module compiles"))
        .expect("LLVM IR is UTF-8");
    let clone_name = "perry_method_argument_shape_clone_ts__Registry__read$pshape_args";
    assert!(
        !ir.contains(&format!("call double @{clone_name}(")),
        "a clone that publishes its parameter must not be routed from a \
         caller-side containment fact:\n{ir}"
    );
    let entries = session.entries();
    assert!(
        !entries.iter().any(|entry| {
            entry.name == "entity"
                && entry.local_id == Some(11)
                && entry.outcome == crate::opt_report::Outcome::Selected
        }),
        "a publishing clone must not preserve the caller's broad Ptr<Shape> fact: {entries:#?}"
    );
}

/// #8833 regression: the three widenings must not compose into an unguarded
/// fixed-offset read of a published object.
///
/// Fixture: a §5.2 barrier the analysis cannot attribute, a callee that
/// publishes its parameter after its licensed read, and TWO route sites on the
/// same caller local — so the second one executes after the alias exists. Every
/// safety net that could catch a reshape here had been removed at once: rule
/// 5's module kill (bypassed by the route-only proof), the caller's post-call
/// containment requirement, and the runtime class+ShapeId guard.
#[test]
fn published_argument_in_a_barrier_module_never_reaches_an_unguarded_clone() {
    let mut module = fixture();
    let param_id = module.classes[1].methods[0].params[0].id;
    // The callee reads the declared field, then publishes the parameter.
    module.classes[1].methods[0]
        .body
        .push(Stmt::Return(Some(Expr::LocalGet(param_id))));
    // A module-wide §5.2 barrier whose target the containment walk cannot
    // attribute to any tracked local.
    module.init.insert(0, define_property(Expr::Object(vec![])));
    // A second route site on the same local, after the first published it.
    let second_call = module.init.last().expect("fixture call").clone();
    module.init.push(second_call);

    let ir = String::from_utf8(compile_module(&module, opts()).expect("module compiles"))
        .expect("LLVM IR is UTF-8");
    let clone_name = "perry_method_argument_shape_clone_ts__Registry__read$pshape_args";
    // Subject-liveness: the argument-clone machinery must actually be engaged
    // by this fixture, or the assertion below would pass for the wrong reason.
    assert!(
        ir.contains(&format!("@{clone_name}(")),
        "fixture must still emit the argument clone, or this test is vacuous:\n{ir}"
    );
    let unguarded_calls = ir.matches(&format!("call double @{clone_name}(")).count();
    let guard_blocks = ir.matches("pshape_arg.fallback").count();

    assert!(
        unguarded_calls == 0 || guard_blocks > 0,
        "a published argument in a barrier-carrying module reached the \
         argument clone with no runtime class+ShapeId guard \
         ({unguarded_calls} clone calls, {guard_blocks} guard blocks):\n{ir}"
    );
}

#[test]
fn field_read_after_publication_does_not_get_a_clone() {
    let mut module = fixture();
    let method = &mut module.classes[1].methods[0];
    method
        .body
        .insert(0, Stmt::Expr(Expr::LocalGet(method.params[0].id)));
    let ir = String::from_utf8(compile_module(&module, opts()).expect("module compiles"))
        .expect("LLVM IR is UTF-8");
    assert!(
        !ir.contains("Registry__read$pshape_args"),
        "an entry shape proof cannot license a field read after publication:\n{ir}"
    );
}

#[test]
fn forwarded_clone_parameter_retains_runtime_guard_and_fallback() {
    let mut module = fixture();
    let entity_param = 21;
    let forward = function(
        201,
        "forward",
        vec![param(
            entity_param,
            "entity",
            Type::Named("Entity".to_string()),
        )],
        vec![
            Stmt::Expr(field_get(entity_param, "id")),
            Stmt::Expr(Expr::Call {
                callee: Box::new(Expr::PropertyGet {
                    object: Box::new(Expr::This),
                    property: "read".to_string(),
                    byte_offset: 0,
                }),
                args: vec![Expr::LocalGet(entity_param)],
                type_args: Vec::new(),
                byte_offset: 0,
            }),
        ],
    );
    module.classes[1].methods.push(forward);
    let ir = String::from_utf8(compile_module(&module, opts()).expect("module compiles"))
        .expect("LLVM IR is UTF-8");

    assert!(
        ir.contains("Registry__forward$pshape_args")
            && ir.contains(
                "call double @perry_method_argument_shape_clone_ts__Registry__read$pshape_args("
            ),
        "the forwarding clone must route its selected parameter onward:\n{ir}"
    );
    assert!(
        ir.contains("pshape_arg.fallback"),
        "a fact inherited from a dynamic clone boundary must retain an exact guard and generic fallback:\n{ir}"
    );
}
