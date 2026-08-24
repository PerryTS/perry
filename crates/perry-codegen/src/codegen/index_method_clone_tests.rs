//! Non-negative index method-clone reachability and fallback ratchets.
//!
//! A clone that merely exists is dead code. These tests require the full
//! contract: a proven non-negative integer argument routes the guarded direct
//! call to the clone, an unproven `number` argument still reaches the ordinary
//! body, and only the clone receives the raw-i32 index fact. The ordinary body
//! is the semantic fallback for fractional, negative, string-like, and
//! dynamically mutated calls, so it must retain generic property-key
//! dispatch.

use crate::{compile_module, CompileOptions};
use perry_hir::types::Type;
use perry_hir::{Class, Expr, Function, Module, ModuleInitKind, Param, Stmt};

const COLUMN_ID: u32 = 11;
const INDEX_ID: u32 = 12;

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

fn function(
    id: u32,
    name: &str,
    params: Vec<Param>,
    return_type: Type,
    body: Vec<Stmt>,
) -> Function {
    Function {
        id,
        name: name.to_string(),
        type_params: Vec::new(),
        params,
        return_type,
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

fn read_method() -> Function {
    function(
        90,
        "read",
        vec![
            param(COLUMN_ID, "column", Type::Array(Box::new(Type::Any))),
            param(INDEX_ID, "index", Type::Number),
        ],
        Type::Any,
        vec![Stmt::Return(Some(Expr::IndexGet {
            object: Box::new(Expr::LocalGet(COLUMN_ID)),
            index: Box::new(Expr::LocalGet(INDEX_ID)),
        }))],
    )
}

fn checked_read_method() -> Function {
    const VALUE_ID: u32 = 13;
    function(
        91,
        "checkedRead",
        vec![
            param(COLUMN_ID, "column", Type::Array(Box::new(Type::Any))),
            param(INDEX_ID, "index", Type::Number),
        ],
        Type::Any,
        vec![
            Stmt::If {
                condition: Expr::Compare {
                    op: perry_hir::CompareOp::Eq,
                    left: Box::new(Expr::LocalGet(COLUMN_ID)),
                    right: Box::new(Expr::Undefined),
                },
                then_branch: vec![Stmt::Throw(Expr::String("absent".to_string()))],
                else_branch: None,
            },
            Stmt::Let {
                id: VALUE_ID,
                name: "value".to_string(),
                ty: Type::Any,
                mutable: false,
                init: Some(Expr::IndexGet {
                    object: Box::new(Expr::LocalGet(COLUMN_ID)),
                    index: Box::new(Expr::LocalGet(INDEX_ID)),
                }),
            },
            Stmt::If {
                condition: Expr::Compare {
                    op: perry_hir::CompareOp::Eq,
                    left: Box::new(Expr::LocalGet(VALUE_ID)),
                    right: Box::new(Expr::Integer(99)),
                },
                then_branch: vec![Stmt::Throw(Expr::String("sentinel".to_string()))],
                else_branch: None,
            },
            Stmt::Return(Some(Expr::LocalGet(VALUE_ID))),
        ],
    )
}

fn reader_class() -> Class {
    Class {
        id: 100,
        name: "Reader".to_string(),
        type_params: Vec::new(),
        extends: None,
        extends_name: None,
        native_extends: None,
        extends_expr: None,
        heritage_lexically_shadowed: false,
        fields: Vec::new(),
        constructor: None,
        methods: vec![read_method()],
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

fn method_call(receiver: Expr, column: Expr, index: Expr) -> Expr {
    Expr::Call {
        callee: Box::new(Expr::PropertyGet {
            object: Box::new(receiver),
            property: "read".to_string(),
            byte_offset: 0,
        }),
        args: vec![column, index],
        type_args: Vec::new(),
        byte_offset: 0,
    }
}

fn fixture() -> Module {
    const READER: u32 = 1;
    const COLUMN: u32 = 2;
    const DYNAMIC_INDEX: u32 = 3;
    let probe = function(
        1,
        "probe",
        vec![
            param(READER, "reader", Type::Named("Reader".to_string())),
            param(COLUMN, "column", Type::Array(Box::new(Type::Any))),
            param(DYNAMIC_INDEX, "dynamicIndex", Type::Number),
        ],
        Type::Any,
        vec![
            // A literal is a call-site proof for the clone.
            Stmt::Let {
                id: 4,
                name: "proven".to_string(),
                ty: Type::Any,
                mutable: false,
                init: Some(method_call(
                    Expr::LocalGet(READER),
                    Expr::LocalGet(COLUMN),
                    Expr::Integer(0),
                )),
            },
            // Negative and fractional constants are known precisely, but do
            // not satisfy the clone's nonnegative-i32 call boundary.
            Stmt::Let {
                id: 5,
                name: "negative".to_string(),
                ty: Type::Any,
                mutable: false,
                init: Some(method_call(
                    Expr::LocalGet(READER),
                    Expr::LocalGet(COLUMN),
                    Expr::Integer(-1),
                )),
            },
            Stmt::Let {
                id: 6,
                name: "fractional".to_string(),
                ty: Type::Any,
                mutable: false,
                init: Some(method_call(
                    Expr::LocalGet(READER),
                    Expr::LocalGet(COLUMN),
                    Expr::Number(0.5),
                )),
            },
            // A plain Number parameter may be negative or fractional. It must
            // keep the public/generic route even though the method has a clone.
            Stmt::Return(Some(method_call(
                Expr::LocalGet(READER),
                Expr::LocalGet(COLUMN),
                Expr::LocalGet(DYNAMIC_INDEX),
            ))),
        ],
    );
    let mut module = Module::new("index_method_clone.ts");
    module.classes = vec![reader_class()];
    module.functions = vec![probe];
    module.init_kind = ModuleInitKind::Eager;
    module
}

fn emit() -> String {
    let opts = CompileOptions {
        emit_ir_only: true,
        output_type: "executable".to_string(),
        ..Default::default()
    };
    String::from_utf8(compile_module(&fixture(), opts).expect("fixture compiles"))
        .expect("LLVM IR is UTF-8")
}

fn emit_checked_reader() -> String {
    let mut class = reader_class();
    class.methods = vec![checked_read_method()];
    let mut module = Module::new("checked_index_method_clone.ts");
    module.classes = vec![class];
    module.init_kind = ModuleInitKind::Eager;
    let opts = CompileOptions {
        emit_ir_only: true,
        output_type: "executable".to_string(),
        ..Default::default()
    };
    String::from_utf8(compile_module(&module, opts).expect("checked reader compiles"))
        .expect("LLVM IR is UTF-8")
}

fn emit_versioned_checked_reader_loop() -> String {
    const ENTITIES: u32 = 20;
    const COLUMN: u32 = 21;
    const BOUND: u32 = 22;
    const CALLBACK: u32 = 23;
    const FILTER: u32 = 24;
    const COUNTER: u32 = 25;
    const ENTITY: u32 = 26;

    let checked_call = Expr::Call {
        callee: Box::new(Expr::PropertyGet {
            object: Box::new(Expr::This),
            property: "checkedRead".to_string(),
            byte_offset: 0,
        }),
        args: vec![Expr::LocalGet(COLUMN), Expr::LocalGet(COUNTER)],
        type_args: Vec::new(),
        byte_offset: 0,
    };
    let iterate = function(
        92,
        "iterate",
        vec![
            param(ENTITIES, "entities", Type::Array(Box::new(Type::Any))),
            param(COLUMN, "column", Type::Array(Box::new(Type::Any))),
            param(BOUND, "bound", Type::Number),
            param(
                CALLBACK,
                "callback",
                Type::Function(perry_hir::types::FunctionType {
                    params: vec![
                        ("entity".to_string(), Type::Any, false),
                        ("value".to_string(), Type::Any, false),
                    ],
                    return_type: Box::new(Type::Void),
                    is_async: false,
                    is_generator: false,
                }),
            ),
            param(FILTER, "filter", Type::Any),
        ],
        Type::Void,
        vec![Stmt::For {
            init: Some(Box::new(Stmt::Let {
                id: COUNTER,
                name: "i".to_string(),
                ty: Type::Number,
                mutable: true,
                init: Some(Expr::Integer(0)),
            })),
            condition: Some(Expr::Compare {
                op: perry_hir::CompareOp::Lt,
                left: Box::new(Expr::LocalGet(COUNTER)),
                right: Box::new(Expr::LocalGet(BOUND)),
            }),
            update: Some(Expr::Update {
                id: COUNTER,
                op: perry_hir::UpdateOp::Increment,
                prefix: false,
            }),
            body: vec![
                Stmt::Let {
                    id: ENTITY,
                    name: "entity".to_string(),
                    ty: Type::Any,
                    mutable: false,
                    init: Some(Expr::IndexGet {
                        object: Box::new(Expr::LocalGet(ENTITIES)),
                        index: Box::new(Expr::LocalGet(COUNTER)),
                    }),
                },
                Stmt::If {
                    condition: Expr::Logical {
                        op: perry_hir::LogicalOp::And,
                        left: Box::new(Expr::LocalGet(FILTER)),
                        right: Box::new(Expr::Unary {
                            op: perry_hir::UnaryOp::Not,
                            operand: Box::new(Expr::Call {
                                callee: Box::new(Expr::LocalGet(FILTER)),
                                args: vec![Expr::LocalGet(ENTITY)],
                                type_args: Vec::new(),
                                byte_offset: 0,
                            }),
                        }),
                    },
                    then_branch: vec![Stmt::Continue],
                    else_branch: None,
                },
                Stmt::Expr(Expr::Call {
                    callee: Box::new(Expr::LocalGet(CALLBACK)),
                    args: vec![Expr::LocalGet(ENTITY), checked_call],
                    type_args: Vec::new(),
                    byte_offset: 0,
                }),
            ],
        }],
    );
    let mut class = reader_class();
    class.methods = vec![checked_read_method(), iterate];
    let mut module = Module::new("versioned_checked_reader_loop.ts");
    module.classes = vec![class];
    module.init_kind = ModuleInitKind::Eager;
    let opts = CompileOptions {
        emit_ir_only: true,
        output_type: "executable".to_string(),
        ..Default::default()
    };
    String::from_utf8(compile_module(&module, opts).expect("versioned loop compiles"))
        .expect("LLVM IR is UTF-8")
}

fn function_body(ir: &str, definition_contains: &str) -> String {
    let start = ir
        .lines()
        .position(|line| line.starts_with("define") && line.contains(definition_contains))
        .unwrap_or_else(|| panic!("no definition containing {definition_contains:?}:\n{ir}"));
    ir.lines()
        .skip(start)
        .take_while(|line| *line != "}")
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn proven_index_routes_to_live_clone_while_unproven_index_keeps_public_fallback() {
    let _native = crate::codegen::helpers::NativeRootsPin::native();
    let ir = emit();
    let clone_symbol = "perry_method_index_method_clone_ts__Reader__read$idx_u31_12";
    let clone = function_body(&ir, &format!("@{clone_symbol}("));
    let public_symbol = "perry_method_index_method_clone_ts__Reader__read";
    let public = function_body(&ir, &format!("@{public_symbol}("));

    assert!(
        clone.lines().next().is_some_and(|line| line.contains(" alwaysinline ")),
        "the proven index clone must be admitted before RS4GC turns its call into a statepoint:\n{clone}"
    );
    assert!(
        public
            .lines()
            .next()
            .is_some_and(|line| !line.contains(" alwaysinline ")),
        "the public fallback must not consume the scoped pre-statepoint code-size budget:\n{public}"
    );

    assert!(
        clone.contains("fptosi double %arg12 to i32")
            && clone.contains("arr.guard.deref")
            && clone.contains("call double @js_typed_feedback_array_index_get_fallback_boxed("),
        "the clone must consume the integer proof through a guarded direct-slot tier:\n{clone}"
    );
    assert!(
        !clone.contains("js_array_get_index_or_string"),
        "the proven clone must not retain generic key dispatch:\n{clone}"
    );

    let clone_calls: Vec<&str> = ir
        .lines()
        .filter(|line| line.contains(&format!("call double @{clone_symbol}(")))
        .collect();
    assert!(
        !clone_calls.is_empty()
            && clone_calls
                .iter()
                .all(|line| line.trim_end().ends_with("double 0.0)")),
        "every emitted body clone must route only the proven zero-index call:\n{clone_calls:#?}\n{ir}"
    );
    let public_calls = ir
        .lines()
        .filter(|line| line.contains(&format!("call double @{public_symbol}(")))
        .count();
    assert!(
        public_calls >= 3,
        "negative, fractional, and unproven Number calls must retain the public fallback:\n{ir}"
    );
    assert!(
        public.contains("aidxkey.sso") && public.contains("js_array_get_index_or_string"),
        "the public body must preserve arbitrary JavaScript property-key semantics:\n{public}"
    );
}

#[test]
fn selector_rejects_mutated_defaulted_and_closure_captured_indices() {
    let candidate = read_method();
    assert_eq!(
        super::typed_abi::nonnegative_index_method_params(&candidate),
        vec![INDEX_ID]
    );

    let mut mutated = candidate.clone();
    mutated.body.insert(
        0,
        Stmt::Expr(Expr::LocalSet(INDEX_ID, Box::new(Expr::Integer(0)))),
    );
    assert!(super::typed_abi::nonnegative_index_method_params(&mutated).is_empty());

    let mut defaulted = candidate.clone();
    defaulted.params[1].default = Some(Expr::Integer(0));
    assert!(super::typed_abi::nonnegative_index_method_params(&defaulted).is_empty());

    let mut captured = candidate;
    captured.body.insert(
        0,
        Stmt::Expr(Expr::Closure {
            func_id: 901,
            params: Vec::new(),
            return_type: Type::Number,
            body: vec![Stmt::Return(Some(Expr::LocalGet(INDEX_ID)))],
            captures: vec![INDEX_ID],
            mutable_captures: Vec::new(),
            captures_this: false,
            captures_new_target: false,
            enclosing_class: None,
            is_arrow: true,
            is_async: false,
            is_generator: false,
            is_strict: true,
        }),
    );
    assert!(super::typed_abi::nonnegative_index_method_params(&captured).is_empty());
}

#[test]
fn checked_reader_gets_a_handle_abi_clone_with_no_array_fallback() {
    let method = checked_read_method();
    let index_params = super::typed_abi::nonnegative_index_method_params(&method);
    assert_eq!(index_params, vec![INDEX_ID]);
    assert_eq!(
        super::typed_abi::nonnegative_index_fast_array_params(&method, &index_params),
        vec![COLUMN_ID]
    );

    let ir = emit_checked_reader();
    let clone = function_body(
        &ir,
        "@perry_method_checked_index_method_clone_ts__Reader__checkedRead$idx_fast_array_u31_12(",
    );
    assert!(
        clone.lines().next().is_some_and(|line| {
            line.contains("i64 %fast_array_handle11") && line.contains(" alwaysinline ")
        }),
        "the fallback-free clone must expose the private live-handle ABI:\n{clone}"
    );
    assert!(
        clone.contains("load double")
            && clone.contains("select i1")
            && !clone.contains("js_typed_feedback_array_index_get_fallback_boxed")
            && !clone.contains("js_array_get_index_or_string")
            && !clone.contains("arr.guard"),
        "the private clone must contain a hole-aware direct load and no ordinary fallback:\n{clone}"
    );
}

#[test]
fn checked_reader_callback_loop_versions_to_fast_and_resumable_slow_bodies() {
    let ir = emit_versioned_checked_reader_loop();
    let iterate = function_body(
        &ir,
        "@perry_method_versioned_checked_reader_loop_ts__Reader__iterate(",
    );
    assert!(
        iterate.contains("versioned_index.loop.fast.preheader")
            && iterate.contains("versioned_index.loop.slow.preheader")
            && iterate.contains("versioned_index.iteration.fast")
            && iterate.contains("label %versioned_index.loop.slow.preheader"),
        "the loop must have an iteration-entry guard and a current-index slow side exit:\n{iterate}"
    );
    assert!(
        iterate.contains(
            "@perry_method_versioned_checked_reader_loop_ts__Reader__checkedRead$idx_fast_array_u31_12("
        ),
        "the fast body must route the checked reader through its live-handle ABI:\n{iterate}"
    );
    let fast_call = iterate
        .lines()
        .find(|line| line.contains("$idx_fast_array_u31_12("))
        .expect("fast clone call exists");
    assert!(
        fast_call.contains("i64 %"),
        "the versioned call must pass a live array handle:\n{fast_call}"
    );
}

#[test]
fn versioned_checked_reader_admission_canonicalizes_one_forwarding_edge() {
    let ir = emit_versioned_checked_reader_loop();
    let iterate = function_body(
        &ir,
        "@perry_method_versioned_checked_reader_loop_ts__Reader__iterate(",
    );
    let source_guard = iterate
        .split("\nversioned_index.array.source_deref.")
        .nth(1)
        .and_then(|body| body.split("\nversioned_index.array.live_deref.").next())
        .unwrap_or_else(|| panic!("loop has no forwarding-source guard:\n{iterate}"));
    let live_handle = source_guard
        .lines()
        .find(|line| line.contains(" = select i1") && line.contains(", i64 "))
        .and_then(|line| line.trim().split_once(" = ").map(|(name, _)| name))
        .unwrap_or_else(|| panic!("source guard has no selected live handle:\n{source_guard}"));
    assert!(
        source_guard.contains("and i8")
            && source_guard.contains(", 128")
            && source_guard.contains("load i64")
            && source_guard.contains("label %versioned_index.array.live_deref.")
            && source_guard.contains("label %versioned_index.loop.slow.preheader")
            && !source_guard.contains(&format!("sub i64 {live_handle}, 8")),
        "admission must select one forwarding target and validate its address before \
         reading its header:\n{source_guard}"
    );
    let live_guard = iterate
        .split("\nversioned_index.array.live_deref.")
        .nth(1)
        .and_then(|body| body.split("\nversioned_index.array.canonicalize.").next())
        .unwrap_or_else(|| panic!("loop has no selected-target header guard:\n{iterate}"));
    assert!(
        live_guard.contains(&format!("sub i64 {live_handle}, 8"))
            && live_guard.contains("label %versioned_index.array.canonicalize.")
            && live_guard.contains("label %versioned_index.loop.slow.preheader"),
        "the selected target must be fully re-branded before admission:\n{live_guard}"
    );
    let canonicalize = iterate
        .split("\nversioned_index.array.canonicalize.")
        .nth(1)
        .and_then(|body| body.split("\nversioned_index.array.source_deref.").next())
        .unwrap_or_else(|| panic!("loop has no canonicalization block:\n{iterate}"));
    assert!(
        canonicalize.contains(&format!(
            "or i64 {live_handle}, {}",
            crate::nanbox::POINTER_TAG_I64
        )) && canonicalize.contains("store ptr addrspace(1)"),
        "the uncaptured array local must be rewritten to the admitted live target so \
         iteration guards do not revisit an identity stub:\n{canonicalize}"
    );
}

#[test]
fn guarded_read_can_follow_one_forwarding_edge_but_rechecks_the_live_header() {
    let ir = emit();
    let clone = function_body(
        &ir,
        "@perry_method_index_method_clone_ts__Reader__read$idx_u31_12(",
    );
    assert!(
        clone.contains("select i1")
            && clone.matches("and i8").count() >= 2
            && clone.contains(", 128")
            && clone.matches("icmp eq i8").count() >= 3,
        "the guard must select a one-edge forwarding target and then re-brand/recheck it:\n{clone}"
    );
    let live_handle = clone
        .lines()
        .find(|line| line.contains(" = select i1") && line.contains(", i64 "))
        .and_then(|line| line.trim().split_once(" = ").map(|(name, _)| name))
        .unwrap_or_else(|| panic!("clone has no selected live array handle:\n{clone}"));
    let selected_target_guard = clone
        .split("\narr.guard.deref.")
        .nth(1)
        .and_then(|body| body.split("\narr.guard.live.").next())
        .unwrap_or_else(|| panic!("clone has no selected-target guard block:\n{clone}"));
    assert!(
        selected_target_guard.contains("label %arr.guard.live.")
            && selected_target_guard.contains("label %arr.fallback.")
            && !selected_target_guard.contains(&format!("sub i64 {live_handle}, 8")),
        "the selected target must branch on its address before any live-header load:\n{selected_target_guard}"
    );
    let live_header_guard = clone
        .split("\narr.guard.live.")
        .nth(1)
        .unwrap_or_else(|| panic!("clone has no live-header guard block:\n{clone}"));
    assert!(
        live_header_guard.contains(&format!("sub i64 {live_handle}, 8")),
        "the live header must be loaded only after the selected address is validated:\n{live_header_guard}"
    );
    let fast = clone
        .split("arr.fast")
        .nth(1)
        .unwrap_or_else(|| panic!("clone has no fast block:\n{clone}"));
    assert!(
        fast.contains(live_handle)
            && fast.contains("load double")
            && !fast.contains("js_array_get_index_or_string"),
        "the revalidated live handle {live_handle} must feed the raw slot load:\n{fast}"
    );
}
