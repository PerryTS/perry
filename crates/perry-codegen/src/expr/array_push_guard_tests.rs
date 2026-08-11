//! #7839: the inline array append's GC bookkeeping behind ONE live test of the
//! stored bits.
//!
//! These are IR-census tests, and both directions matter.
//!
//! The positive one asserts the subject is LIVE. A guard predicate that
//! silently never fires still compiles, still prints the right answer, and
//! shows up in no other test — `push_num.ts` would simply stay slow. Only the
//! emitted block label separates "implemented" from "reached" (CLAUDE.md, "a
//! gate must assert its subject was live"), so the block name is asserted
//! present AND the `apush.inbounds` fast path is asserted free of the two calls
//! the guard exists to move out of it.
//!
//! The negative is the safety half, twice over. A pointer-valued push must keep
//! the historical unguarded shape — widening the guard to it would pay a
//! predicate for a test that always says "yes" — and, more importantly, the
//! bookkeeping calls must still be REACHABLE from the guarded arm. The change
//! is "skip the calls when the live bits prove them dead", never "elide them
//! outright": a `number`-annotated parameter that actually holds a string at
//! runtime (Perry does not validate declared types) takes the guarded arm and
//! records the slot exactly as it always did. A test that asserted the calls
//! ABSENT would be pinning silent heap corruption.

use crate::{compile_module, AppMetadata, CompileOptions};
use perry_hir::types::Type;
use perry_hir::{
    BinaryOp, Class, ClassField, CompareOp, Expr, Function, Module, ModuleInitKind, Param, Stmt,
    UpdateOp,
};

/// The block that exists only when the #7839 guard was emitted.
const GUARD_BLOCK: &str = "apush.gc_bookkeeping";
const NOTE_CALL: &str = "call void @js_gc_note_slot_layout(";
const ADDREF_CALL: &str = "call void @js_string_addref_if_heap_string(";
/// `ARRAY_PUSH_NUMERIC_CLEAN_I16` as it appears in the `nofwd` admission test.
const WIDENED_ADMISSION_MASK: &str = "15367";
/// The historical integrity mask, which the numeric push must NOT still use.
const NARROW_INTEGRITY_MASK: &str = ", 1031";

fn ir_opts() -> CompileOptions {
    CompileOptions {
        target: None,
        is_entry_module: true,
        non_entry_module_prefixes: Vec::new(),
        nextjs_path_init_modules: Vec::new(),
        import_function_prefixes: std::collections::HashMap::new(),
        import_function_ffi_aliases: std::collections::HashMap::new(),
        import_function_origin_names: std::collections::HashMap::new(),
        import_function_v8_specifiers: std::collections::HashMap::new(),
        import_function_node_submodule: std::collections::HashMap::new(),
        namespace_node_submodules: std::collections::HashMap::new(),
        namespace_v8_specifiers: std::collections::HashMap::new(),
        namespace_member_prefixes: std::collections::HashMap::new(),
        namespace_member_origin_names: std::collections::HashMap::new(),
        emit_ir_only: true,
        verify_native_regions: false,
        disable_buffer_fast_path: false,
        namespace_imports: Vec::new(),
        namespace_member_nested: Vec::new(),
        imported_classes: Vec::new(),
        imported_enums: Vec::new(),
        imported_async_funcs: std::collections::HashSet::new(),
        type_aliases: std::collections::HashMap::new(),
        imported_func_param_counts: std::collections::HashMap::new(),
        imported_func_has_rest: std::collections::HashSet::new(),
        imported_func_synthetic_arguments: std::collections::HashSet::new(),
        imported_func_return_types: std::collections::HashMap::new(),
        imported_vars: std::collections::HashSet::new(),
        output_type: "executable".to_string(),
        needs_stdlib: false,
        needs_ui: false,
        needs_geisterhand: false,
        geisterhand_port: 7676,
        enabled_features: Vec::new(),
        native_module_init_names: Vec::new(),
        js_module_specifiers: Vec::new(),
        bundled_extensions: Vec::new(),
        native_library_functions: Vec::new(),
        i18n_table: None,
        fast_math: false,
        fp_contract_mode: crate::FpContractMode::Off,
        app_metadata: AppMetadata::default(),
        namespace_entries: Vec::new(),
        dynamic_import_path_to_prefix: std::collections::HashMap::new(),
        deferred_module_prefixes: std::collections::HashSet::new(),
        module_init_deps: Vec::new(),
        is_dynamic_import_target: false,
        debug_locations: false,
        module_source: None,
        debug_source_line_offset: 0,
    }
}

const ARRAY_ID: u32 = 1;
const COUNTER_ID: u32 = 2;
const BASE_ID: u32 = 3;

fn node_class() -> Class {
    Class {
        id: 404,
        name: "Node".to_string(),
        type_params: Vec::new(),
        extends: None,
        extends_name: None,
        native_extends: None,
        extends_expr: None,
        heritage_lexically_shadowed: false,
        fields: vec![ClassField {
            name: "v".to_string(),
            key_expr: None,
            ty: Type::Number,
            init: None,
            is_private: false,
            is_readonly: false,
            decorators: Vec::new(),
        }],
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

/// `function chunk(base: number) { const keep: <elem>[] = []; for (let j = 0;
/// j < 1000; j++) keep.push(<value>) }` — `bench/push_num.ts`'s kernel, in the
/// position that matters: the array is a plain function LOCAL, which is what
/// puts the push on the inline `apush` tier at all.
fn push_module(elem: Type, value: Expr, classes: Vec<Class>) -> Module {
    let mut m = Module::new("array_push_guard.ts");
    m.classes = classes;
    m.functions = vec![Function {
        id: 700,
        name: "chunk".to_string(),
        type_params: Vec::new(),
        params: vec![Param {
            id: BASE_ID,
            name: "base".to_string(),
            ty: Type::Number,
            default: None,
            decorators: Vec::new(),
            is_rest: false,
            arguments_object: None,
        }],
        return_type: Type::Void,
        body: vec![
            Stmt::Let {
                id: ARRAY_ID,
                name: "keep".to_string(),
                ty: Type::Array(Box::new(elem)),
                mutable: false,
                init: Some(Expr::Array(Vec::new())),
            },
            Stmt::For {
                init: Some(Box::new(Stmt::Let {
                    id: COUNTER_ID,
                    name: "j".to_string(),
                    ty: Type::Number,
                    mutable: true,
                    init: Some(Expr::Integer(0)),
                })),
                condition: Some(Expr::Compare {
                    op: CompareOp::Lt,
                    left: Box::new(Expr::LocalGet(COUNTER_ID)),
                    right: Box::new(Expr::Integer(1000)),
                }),
                update: Some(Expr::Update {
                    id: COUNTER_ID,
                    op: UpdateOp::Increment,
                    prefix: false,
                }),
                body: vec![Stmt::Expr(Expr::ArrayPush {
                    array_id: ARRAY_ID,
                    value: Box::new(value),
                })],
            },
        ],
        is_async: false,
        is_generator: false,
        is_strict: false,
        is_exported: false,
        captures: Vec::new(),
        decorators: Vec::new(),
        was_plain_async: false,
        was_unrolled: false,
    }];
    // Called once from module init so the function is not dead-stripped before
    // the census can see it.
    m.init = vec![Stmt::Expr(Expr::Call {
        callee: Box::new(Expr::FuncRef(700)),
        args: vec![Expr::Number(1.0)],
        type_args: Vec::new(),
        byte_offset: 0,
    })];
    m.init_kind = ModuleInitKind::Eager;
    m
}

fn ir_for(m: Module) -> String {
    String::from_utf8(compile_module(&m, ir_opts()).expect("module compiles"))
        .expect("LLVM IR should be UTF-8")
}

/// The one block between `apush.inbounds` and the next label, i.e. the fast
/// path the guard exists to empty. Asserting over the WHOLE function would pass
/// while the calls sat in the fast path, because the guarded arm contains them
/// too.
fn inbounds_block(ir: &str) -> String {
    let start = ir
        .find("\napush.inbounds")
        .unwrap_or_else(|| panic!("no apush.inbounds block in:\n{ir}"));
    let rest = &ir[start + 1..];
    let body_start = rest.find('\n').expect("label line") + 1;
    let end = rest[body_start..]
        .find("\n\n")
        .map(|e| body_start + e)
        .unwrap_or(rest.len());
    rest[..end].to_string()
}

/// `keep.push(base + j)` — `push_num.ts` verbatim. `Expr::Binary { Add }` is
/// the shape no static non-pointer proof can admit (`+` is string
/// concatenation for non-numeric operands), which is exactly why the live test
/// is what retires the calls here.
fn numeric_add_push() -> Expr {
    Expr::Binary {
        op: BinaryOp::Add,
        left: Box::new(Expr::LocalGet(BASE_ID)),
        right: Box::new(Expr::LocalGet(COUNTER_ID)),
    }
}

#[test]
fn a_numeric_push_moves_its_gc_bookkeeping_behind_one_live_test() {
    let ir = ir_for(push_module(Type::Number, numeric_add_push(), Vec::new()));
    assert!(
        ir.contains(GUARD_BLOCK),
        "the #7839 guard was never emitted for `keep.push(base + j)` on a \
         `number[]`; without it every element of push_num.ts pays two \
         cross-crate calls:\n{ir}"
    );
    let inbounds = inbounds_block(&ir);
    assert!(
        !inbounds.contains(NOTE_CALL),
        "js_gc_note_slot_layout is still on the inline fast path:\n{inbounds}"
    );
    assert!(
        !inbounds.contains(ADDREF_CALL),
        "js_string_addref_if_heap_string is still on the inline fast path:\n{inbounds}"
    );
    // The array's half of the proof: the `nofwd` admission test must have
    // widened, or an element-shape-proven / all-pointer / typed-descriptor
    // array would reach the inline store and silently skip the note it needs.
    assert!(
        ir.contains(WIDENED_ADMISSION_MASK),
        "the nofwd admission mask did not widen to 0x3C07 for a numeric push:\n{ir}"
    );
    assert!(
        !ir.contains(NARROW_INTEGRITY_MASK),
        "a numeric push still admits on the narrow 0x0407 integrity mask, so \
         the guard rests on nothing about the array:\n{ir}"
    );
}

#[test]
fn the_guarded_arm_still_reaches_every_call_it_moved() {
    let ir = ir_for(push_module(Type::Number, numeric_add_push(), Vec::new()));
    // Not an elision. A `number`-annotated value that is a heap string at
    // runtime (Perry does not validate declared types) takes this arm.
    assert!(
        ir.contains(NOTE_CALL),
        "the layout note was ELIDED rather than guarded — a pointer reaching \
         this push would strand a live child:\n{ir}"
    );
    assert!(
        ir.contains(ADDREF_CALL),
        "the string addref was ELIDED rather than guarded:\n{ir}"
    );
}

#[test]
fn a_pointer_push_keeps_the_historical_unguarded_shape() {
    let ir = ir_for(push_module(
        Type::Named("Node".to_string()),
        Expr::New {
            class_name: "Node".to_string(),
            args: vec![Expr::LocalGet(COUNTER_ID)],
            type_args: Vec::new(),
            byte_offset: 0,
            cap_args_appended: 0,
        },
        vec![node_class()],
    ));
    assert!(
        !ir.contains(GUARD_BLOCK),
        "a `new Node()` push took the numeric guard: it would pay the \
         predicate for a test whose answer is always yes:\n{ir}"
    );
}
