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
//! outright": an unexpected tag-shaped NaN payload takes the guarded arm and
//! records the slot exactly as it always did. Metadata-only numeric candidates
//! are tested separately below and stay on the runtime number guard. A test
//! that asserted the bookkeeping calls ABSENT would pin silent heap corruption.

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
const NUMERIC_NOTE_CALL: &str = "call void @js_array_note_numeric_write(";
/// `ARRAY_PUSH_NUMERIC_CLEAN_I16` as it appears in the `nofwd` admission test.
const WIDENED_ADMISSION_MASK: &str = "15367";
/// The historical integrity mask, which the numeric push must NOT still use.
const NARROW_INTEGRITY_MASK: &str = ", 1031";
const POINTER_LAYOUT_BLOCK: &str = "apush.pointer_layout.bookkeeping";
const POINTER_LAYOUT_MASK: &str = "63616";

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
        short_spread_method_candidates: std::sync::Arc::default(),
        object_literal_method_candidates: std::sync::Arc::default(),
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
                    field_writeback: None,
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

fn function_body<'a>(ir: &'a str, marker: &str) -> &'a str {
    let start = ir
        .match_indices("define ")
        .find(|(index, _)| {
            ir[*index..]
                .lines()
                .next()
                .is_some_and(|line| line.contains(marker))
        })
        .map(|(index, _)| index)
        .unwrap_or_else(|| panic!("missing function containing {marker}:\n{ir}"));
    let end = ir[start..]
        .find("\n}")
        .map(|offset| start + offset)
        .expect("function terminator");
    &ir[start..end]
}

/// A canonical numeric `+` whose operands have runtime-derived evidence. The
/// live-bits guard remains useful because NaN payloads still require GC-layout
/// bookkeeping even though neither operand rests on source metadata.
fn numeric_add_push() -> Expr {
    Expr::Binary {
        op: BinaryOp::Add,
        left: Box::new(Expr::LocalGet(COUNTER_ID)),
        right: Box::new(Expr::LocalGet(COUNTER_ID)),
    }
}

/// The benchmark-like shape whose left operand is only a declared-number
/// parameter. Perry does not enforce that annotation, so this candidate must
/// keep the live runtime-number guard and generic push fallback.
fn metadata_numeric_add_push() -> Expr {
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
    // Not an elision. A tag-shaped live payload still takes this arm.
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

#[test]
fn a_dynamic_push_consumes_a_live_all_pointer_array_proof() {
    let mut module = push_module(Type::Any, Expr::LocalGet(BASE_ID), Vec::new());
    module.functions[0].params[0].ty = Type::Any;
    let ir = ir_for(module);
    let inbounds = inbounds_block(&ir);
    assert!(
        inbounds.contains(POINTER_LAYOUT_BLOCK),
        "a dynamically typed append never emitted the live pointer/layout guard:\n{inbounds}"
    );
    assert!(
        inbounds.contains(POINTER_LAYOUT_MASK),
        "the guard does not test the complete all-pointer/raw-f64/element-shape header mask:\n{inbounds}"
    );
    assert!(
        inbounds.contains(crate::nanbox::POINTER_TAG_TOP16_I64),
        "the guard does not test the live value's exact POINTER_TAG:\n{inbounds}"
    );
    assert!(
        ir.contains(NOTE_CALL) && ir.contains(ADDREF_CALL) && ir.contains(NUMERIC_NOTE_CALL),
        "the proof-miss arm must retain every generic bookkeeping call:\n{ir}"
    );
}

// ---------------------------------------------------------------------------
// #7831/#7837 collision: an erased annotation is a hint, not a runtime proof.
// ---------------------------------------------------------------------------

/// `arr.push(<element read off a `number[]`>)` — a declared-type LIE vehicle.
///
/// A `number[]` can hold heap strings at runtime; Perry does not validate
/// declared types, and `gc-handoff/m0810/numarr_lie.ts` builds exactly such an
/// array. `is_numeric_expr` DOES admit this read (it consults the declared
/// element type, #7810), so the annotation alone would put a heap string on a
/// numeric push path.
///
/// What keeps it off #7839's guard is a second, independent test:
/// `expr_produces_canonical_raw_f64` excludes every READ ("cold fallbacks
/// return boxed bits"), so `keep_guarded_numeric_push` stays true and the push
/// takes the pre-existing RUNTIME numeric tier — `js_array_numeric_push_f64_
/// unboxed` behind its feedback guard — which validates the value at runtime.
/// #7839's inline guard is reached only when the value is canonical raw f64 BY
/// CONSTRUCTION, i.e. produced by a machine FP op that cannot yield a pointer
/// except through NaN-payload propagation, which is precisely what its
/// live-bits test catches.
///
/// This test pins that routing. If `expr_produces_canonical_raw_f64` ever
/// widened to admit a read, a declared-type lie would start arriving at the
/// inline guard, and this fails instead of the guard silently resting on an
/// erased annotation.
fn element_read_push_module() -> Module {
    let mut m = push_module(Type::Number, Expr::Number(0.0), Vec::new());
    let f = &mut m.functions[0];
    let Some(Stmt::For { body, .. }) = f.body.get_mut(1) else {
        panic!("push_module's second statement should be the `for`");
    };
    body[0] = Stmt::Expr(Expr::ArrayPush {
        array_id: ARRAY_ID,
        value: Box::new(Expr::IndexGet {
            object: Box::new(Expr::LocalGet(ARRAY_ID)),
            index: Box::new(Expr::LocalGet(COUNTER_ID)),
        }),
        field_writeback: None,
    });
    m
}

#[test]
fn a_declared_type_lie_is_routed_to_the_runtime_tier_not_the_guard() {
    let ir = ir_for(element_read_push_module());
    // Non-vacuity: the push must actually have been lowered on the tier this
    // test names. Without this the assertion below would also pass for a push
    // that was not lowered at all.
    assert!(
        ir.contains("js_array_numeric_push_f64_unboxed"),
        "expected the runtime numeric tier for an element-read value; this          test is not observing the tier it claims to:\n{ir}"
    );
    assert!(
        !ir.contains(GUARD_BLOCK),
        "an element read off a `number[]` reached the #7839 inline guard. That          array can hold heap strings at runtime (numarr_lie.ts), so the guard          would be resting on an erased annotation instead of on the value's          construction:\n{ir}"
    );
}

#[test]
fn the_guard_branches_on_the_live_bits_not_on_a_constant() {
    let ir = ir_for(push_module(Type::Number, numeric_add_push(), Vec::new()));
    let inbounds = inbounds_block(&ir);
    let branch = inbounds
        .lines()
        .find(|l| l.contains("br i1") && l.contains(GUARD_BLOCK))
        .unwrap_or_else(|| panic!("no branch into the guarded arm:\n{inbounds}"));
    let cond = branch
        .trim()
        .strip_prefix("br i1 ")
        .and_then(|r| r.split(',').next())
        .expect("br i1 <cond>, ...");
    // A constant condition is the exact shape a "simplification" of the guard
    // collapses to, and it is invisible to every output-equality test: a
    // sabotaged build with `br i1 false` still prints the right answer on every
    // probe, because the elided bookkeeping is a GC-liveness fact, not an
    // arithmetic one. Pin the condition to a computed register.
    assert!(
        cond.starts_with('%'),
        "the guard branches on the constant `{cond}` — the bookkeeping arm is \
         unreachable and the guard proves nothing:\n{inbounds}"
    );
    assert!(
        inbounds.contains(&format!("{cond} = or i1 ")),
        "the guard's condition {cond} is not the `or` of the live-bits tests:\n{inbounds}"
    );
    // ...and the live-bits tests themselves: POINTER_TAG / STRING_TAG /
    // BIGINT_TAG top-16 comparands, plus the bare-heap-address floor.
    for needle in ["lshr i64", "32765", "32767", "32762", "4096"] {
        assert!(
            inbounds.contains(needle),
            "the live-bits predicate lost `{needle}`, so it no longer covers \
             every heap tag `layout_pointer_bearing_bits` accepts:\n{inbounds}"
        );
    }
}

#[test]
fn a_metadata_selected_add_keeps_the_runtime_number_guard() {
    let ir = ir_for(push_module(
        Type::Number,
        metadata_numeric_add_push(),
        Vec::new(),
    ));
    // #8079 may additionally emit a declaration-guarded clone. This test's
    // safety subject is the unchanged generic fallback, where the annotation
    // is still metadata rather than proof.
    let generic = function_body(&ir, "$generic(");
    assert!(
        generic.contains("call i32 @js_typed_feedback_numeric_array_push_guard")
            && generic.contains("call i64 @js_array_numeric_push_f64_unboxed")
            && generic.contains("call i64 @js_array_push_f64_spec"),
        "a declared-number addition must validate the live value and retain the generic push fallback:\n{generic}"
    );
    assert!(
        !generic.contains(GUARD_BLOCK),
        "metadata alone must not reach the pointer-only inline bookkeeping guard:\n{generic}"
    );
}

/// `class Buffer { items: number[]; add() { this.items.push(1) } }` as
/// `perry-transform::field_push_local_bind` leaves it: the receiver local plus
/// an `ArrayPush` that carries the field to write back.
fn field_push_module(field_writeback: Option<String>) -> Module {
    const RECV_ID: u32 = 40;
    let method = Function {
        id: 710,
        name: "add".to_string(),
        type_params: Vec::new(),
        params: Vec::new(),
        return_type: Type::Void,
        body: vec![
            Stmt::Let {
                id: RECV_ID,
                name: "__push_recv".to_string(),
                ty: Type::Array(Box::new(Type::Number)),
                mutable: true,
                init: Some(Expr::PropertyGet {
                    object: Box::new(Expr::This),
                    property: "items".to_string(),
                    byte_offset: 0,
                }),
            },
            Stmt::Expr(Expr::ArrayPush {
                array_id: RECV_ID,
                value: Box::new(Expr::Number(1.0)),
                field_writeback,
            }),
        ],
        is_async: false,
        is_generator: false,
        is_strict: false,
        is_exported: false,
        captures: Vec::new(),
        decorators: Vec::new(),
        was_plain_async: false,
        was_unrolled: false,
    };
    let mut class = node_class();
    class.id = 405;
    class.name = "Buffer".to_string();
    class.fields = vec![ClassField {
        name: "items".to_string(),
        key_expr: None,
        ty: Type::Array(Box::new(Type::Number)),
        init: None,
        is_private: false,
        is_readonly: false,
        decorators: Vec::new(),
    }];
    class.methods = vec![method];
    let mut m = Module::new("array_push_field_writeback.ts");
    m.classes = vec![class];
    m.init = vec![Stmt::Expr(Expr::New {
        class_name: "Buffer".to_string(),
        args: Vec::new(),
        type_args: Vec::new(),
        byte_offset: 0,
        cap_args_appended: 0,
    })];
    m.init_kind = ModuleInitKind::Eager;
    m
}

/// #8897: the field write-back after `this.items.push(v)` must be decided on
/// the receiver local's HANDLE BITS, not on JS equality — equality sees
/// through the growth-forwarding stub a re-allocating append leaves behind,
/// so a `!==` guard never fired and the field kept the stub. The IR must
/// contain the bits compare, the plain-object header gate (the repair is
/// skipped on frozen / sealed / descriptor-bearing receivers rather than
/// throwing), and the field store behind it; a push without a write-back
/// target must emit none of it.
#[test]
fn a_field_push_writes_the_field_back_on_a_handle_bits_change_behind_a_plain_object_gate() {
    let ir = ir_for(field_push_module(Some("items".to_string())));
    let body = function_body(&ir, "add");
    assert!(
        body.contains("apush.field.writeback"),
        "the write-back arm must exist:\n{body}"
    );
    let deref = super::class_field_barrier_tests::block_body(body, "apush.field.deref")
        .expect("the header gate block is defined");
    assert!(
        deref.contains("and i16 %") && deref.contains(", 2055"),
        "the gate must test the frozen/sealed/no-extend/descriptor flags in one mask:\n{deref}"
    );
    assert!(
        deref.contains("icmp eq i8 %") && deref.contains(", 2\n"),
        "the gate must require a GC_TYPE_OBJECT receiver:\n{deref}"
    );
    // The store arm is the ordinary class-field set: the inline IC store
    // with its barrier, and the descriptor-aware runtime fallback behind it.
    assert!(
        body.contains("class_field_set.fast") && body.contains("@js_class_field_set_fallback("),
        "the write-back arm must be the class-field store:\n{body}"
    );
    // Between the header gate and the store: `this.items` is re-read and its
    // bits compared with the captured head, so an argument that assigned the
    // field (`this.items.push(this.reset())`) keeps its assignment.
    assert!(
        body.contains("apush.field.still_held"),
        "the field-still-held gate must exist:\n{body}"
    );
    let bits_compares = body.matches("icmp eq i64 %").count();
    assert!(
        bits_compares >= 2,
        "both the local and the field are compared against the captured bits ({bits_compares}):\n{body}"
    );
    assert!(
        body.contains("icmp eq i64 %"),
        "the decision must be a handle-bits compare, not a JS equality:\n{body}"
    );

    let plain = ir_for(field_push_module(None));
    let plain_body = function_body(&plain, "add");
    assert!(
        !plain_body.contains("apush.field.") && !plain_body.contains("class_field_set."),
        "a push with no write-back target must emit neither the field arm nor a field store"
    );
}
