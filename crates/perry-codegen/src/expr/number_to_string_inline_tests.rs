//! #10762: which lowering the number-to-string spellings and a following
//! `charCodeAt` select. The runtime answers are pinned by
//! `test_gap_10762_inline_number_to_string.ts`; these pin that the inline arms
//! are actually emitted, and emitted only where the operand is a number.

use crate::{compile_module, CompileOptions};
use perry_hir::types::Type;
use perry_hir::{BinaryOp, Expr, Function, Module, ModuleInitKind, Param, Stmt};

const NUM_ID: u32 = 1;
const STR_ID: u32 = 2;
const INLINE_FAST: &str = "num2str.fast";
const SSO_CHAR_CODE: &str = "cca.sso_fast";

fn ir_opts() -> CompileOptions {
    CompileOptions {
        emit_ir_only: true,
        output_type: "executable".to_string(),
        ..Default::default()
    }
}

fn param(ty: Type) -> Param {
    Param {
        id: NUM_ID,
        name: "n".to_string(),
        ty,
        default: None,
        decorators: Vec::new(),
        is_rest: false,
        arguments_object: None,
    }
}

fn module_with(params: Vec<Param>, body: Vec<Stmt>) -> Module {
    Module {
        name: "num2str_inline.ts".to_string(),
        imports: Vec::new(),
        exports: Vec::new(),
        classes: Vec::new(),
        interfaces: Vec::new(),
        type_aliases: Vec::new(),
        enums: Vec::new(),
        globals: Vec::new(),
        functions: vec![Function {
            id: 1,
            name: "probe".to_string(),
            type_params: Vec::new(),
            params,
            return_type: Type::Any,
            body,
            is_async: false,
            is_generator: false,
            is_strict: true,
            was_plain_async: false,
            was_unrolled: false,
            is_exported: true,
            captures: Vec::new(),
            decorators: Vec::new(),
        }],
        script_global_functions: Vec::new(),
        references_global_this: false,
        annexb_global_undefined_names: Vec::new(),
        init_is_strict: false,
        init: Vec::new(),
        classic_for_lexical_bindings: std::collections::HashSet::new(),
        exported_native_instances: Vec::new(),
        exported_func_return_native_instances: Vec::new(),
        exported_objects: Vec::new(),
        exported_functions: Vec::new(),
        widgets: Vec::new(),
        uses_fetch: false,
        uses_webassembly: false,
        extern_funcs: Vec::new(),
        init_was_unrolled: false,
        has_top_level_await: false,
        init_kind: ModuleInitKind::Eager,
        async_step_closures: std::collections::HashSet::new(),
        closure_display_names: std::collections::HashMap::new(),
        class_display_names: std::collections::HashMap::new(),
        closure_source_text: std::collections::HashMap::new(),
        class_source_text: std::collections::HashMap::new(),
        async_generator_funcs: std::collections::HashSet::new(),
        local_source_spans: std::collections::HashMap::new(),
        gen_param_prologue_len: std::collections::HashMap::new(),
    }
}

/// The whole module's IR: whichever clones of the body a typed parameter
/// produces, the lowering under test is in it.
fn ir(params: Vec<Param>, body: Vec<Stmt>) -> String {
    let module = module_with(params, body);
    String::from_utf8(compile_module(&module, ir_opts()).unwrap()).expect("LLVM IR is UTF-8")
}

fn number_n() -> Expr {
    // `n * 1` — numeric by construction, whatever the parameter's proof.
    Expr::Binary {
        op: BinaryOp::Mul,
        left: Box::new(Expr::LocalGet(NUM_ID)),
        right: Box::new(Expr::Integer(1)),
    }
}

fn method(object: Expr, property: &str, args: Vec<Expr>) -> Expr {
    Expr::Call {
        callee: Box::new(Expr::PropertyGet {
            object: Box::new(object),
            property: property.to_string(),
            byte_offset: 0,
        }),
        args,
        type_args: Vec::new(),
        byte_offset: 0,
    }
}

fn returns(e: Expr) -> Vec<Stmt> {
    vec![Stmt::Return(Some(e))]
}

/// Every spelling of a number's conversion gets the inline arm.
///
/// Sabotage: removing any one call site's `emit_number_to_string_inline`
/// leaves its IR without `num2str.fast`.
#[test]
fn each_spelling_of_a_number_conversion_is_inlined() {
    let spellings = [
        ("String(n)", Expr::StringCoerce(Box::new(number_n()))),
        ("`${n}`", Expr::TemplateStringCoerce(Box::new(number_n()))),
        ("n.toString()", method(number_n(), "toString", Vec::new())),
        (
            "\"\" + n",
            Expr::Binary {
                op: BinaryOp::Add,
                left: Box::new(Expr::String(String::new())),
                right: Box::new(number_n()),
            },
        ),
    ];
    for (label, expr) in spellings {
        let ir = ir(vec![param(Type::Number)], returns(expr));
        assert!(
            ir.contains(INLINE_FAST),
            "{label} must build its text inline:\n{ir}"
        );
    }
}

/// An operand the analysis cannot call a number keeps the plain call: the
/// inline arm is code size spent on every site, so it is only emitted where it
/// is expected to fire.
#[test]
fn an_unproven_operand_keeps_the_runtime_call() {
    let ir = ir(
        vec![param(Type::Any)],
        returns(Expr::StringCoerce(Box::new(Expr::LocalGet(NUM_ID)))),
    );
    assert!(
        !ir.contains(INLINE_FAST),
        "an `any` operand must not be inlined:\n{ir}"
    );
    assert!(ir.contains("@js_string_coerce_box("));
}

/// `const s = String(n); s.charCodeAt(0)` (and `${n}`, `"" + n`) keeps the
/// string proof its initializer carries, so it takes the inline `charCodeAt`, including the
/// arm that reads a short string's byte out of the value.
///
/// Sabotage: without these initializers in `proven_type_from_init` the call
/// goes to the generic method site and `cca.sso_fast` is absent; without the
/// SSO arm the inline lowering has no `cca.sso_fast` block.
#[test]
fn a_string_coerced_local_reads_char_codes_inline() {
    let concat = Expr::Binary {
        op: BinaryOp::Add,
        left: Box::new(Expr::String(String::new())),
        right: Box::new(number_n()),
    };
    let inits = [
        ("String(n)", Expr::StringCoerce(Box::new(number_n()))),
        ("`${n}`", Expr::TemplateStringCoerce(Box::new(number_n()))),
        ("\"\" + n", concat),
    ];
    for (label, init) in inits {
        let body = vec![
            Stmt::Let {
                id: STR_ID,
                name: "s".to_string(),
                ty: Type::String,
                mutable: false,
                init: Some(init),
            },
            Stmt::Return(Some(method(
                Expr::LocalGet(STR_ID),
                "charCodeAt",
                vec![Expr::Integer(0)],
            ))),
        ];
        let ir = ir(vec![param(Type::Number)], body);
        assert!(
            ir.contains(SSO_CHAR_CODE),
            "s = {label}; s.charCodeAt must be inline:\n{ir}"
        );
    }
}
