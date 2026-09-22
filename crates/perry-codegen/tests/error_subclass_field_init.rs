//! #10443: `super()` into a built-in Error must be followed by the derived
//! class's own field initializers.
//!
//! Every other arm of the non-user-parent `super()` block (`EventEmitter`,
//! `Map`/`Set`, the streams, `Promise`, `DOMException`, ...) applies
//! `FieldInitMode::SelfOnly` once the base is installed. The Error-family arm
//! did not, so `class E extends Error { labels = new Set(); constructor(m) {
//! super(m); } }` constructed directly ran no initializer at all and every
//! field read `undefined` — mongodb's `MongoError.errorLabelSet`, and then a
//! SIGSEGV in `js_set_add` (#10446).
//!
//! An IR census rather than an execution test because the ORDER is the
//! contract: the field install has to come after the base's `super()` work
//! (spec: derived field initializers run when `super()` returns), and it has
//! to be emitted exactly once — a second copy is what the staging side of the
//! fix (`root_fields_run_at_own_super`) exists to prevent, and for a private
//! field it would throw at run time.

use perry_codegen::{compile_module, CompileOptions};
use perry_hir::types::Type;
use perry_hir::{Class, ClassField, Expr, Function, Module, ModuleInitKind, Stmt};

const CAPTURE_STACK: &str = "@js_error_subclass_capture_stack(";
const FIELD_INSTALL: [&str; 2] = ["@js_class_field_add(", "@js_object_set_field_by_name("];

fn ir_opts() -> CompileOptions {
    CompileOptions {
        is_entry_module: true,
        emit_ir_only: true,
        output_type: "executable".to_string(),
        ..Default::default()
    }
}

fn field(name: &str, init: Expr) -> ClassField {
    ClassField {
        name: name.to_string(),
        key_expr: None,
        ty: Type::Number,
        init: Some(init),
        is_private: false,
        is_readonly: false,
        decorators: Vec::new(),
    }
}

fn ctor(id: u32, body: Vec<Stmt>) -> Function {
    Function {
        id,
        name: "constructor".to_string(),
        type_params: Vec::new(),
        params: Vec::new(),
        return_type: Type::Void,
        body,
        is_async: false,
        is_generator: false,
        is_strict: false,
        is_exported: false,
        captures: Vec::new(),
        decorators: Vec::new(),
        was_plain_async: false,
        was_unrolled: false,
    }
}

fn class(
    id: u32,
    name: &str,
    extends: &str,
    fields: Vec<ClassField>,
    ctor: Option<Function>,
) -> Class {
    Class {
        id,
        name: name.to_string(),
        type_params: Vec::new(),
        extends: None,
        extends_name: Some(extends.to_string()),
        native_extends: None,
        extends_expr: None,
        heritage_lexically_shadowed: false,
        fields,
        constructor: ctor,
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

fn module_with(classes: Vec<Class>, body: Vec<Stmt>) -> Module {
    Module {
        name: "error_subclass_field_init.ts".to_string(),
        imports: Vec::new(),
        exports: Vec::new(),
        classes,
        interfaces: Vec::new(),
        type_aliases: Vec::new(),
        enums: Vec::new(),
        globals: Vec::new(),
        functions: vec![Function {
            id: 1,
            name: "probe".to_string(),
            type_params: Vec::new(),
            params: Vec::new(),
            return_type: Type::Void,
            body,
            is_async: false,
            is_generator: false,
            is_strict: false,
            is_exported: false,
            captures: Vec::new(),
            decorators: Vec::new(),
            was_plain_async: false,
            was_unrolled: false,
        }],
        init_is_strict: false,
        init: Vec::new(),
        classic_for_lexical_bindings: std::collections::HashSet::new(),
        exported_native_instances: Vec::new(),
        exported_func_return_native_instances: Vec::new(),
        exported_objects: Vec::new(),
        exported_functions: Vec::new(),
        script_global_functions: Vec::new(),
        references_global_this: false,
        annexb_global_undefined_names: Vec::new(),
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

fn ir_for(classes: Vec<Class>, body: Vec<Stmt>) -> String {
    String::from_utf8(compile_module(&module_with(classes, body), ir_opts()).unwrap())
        .expect("LLVM IR should be UTF-8")
}

fn new_expr(name: &str) -> Expr {
    Expr::New {
        class_name: name.to_string(),
        args: vec![Expr::String("m".to_string())],
        type_args: Vec::new(),
        byte_offset: 0,
        cap_args_appended: 0,
    }
}

/// The body of the emitted function whose definition line contains `needle`.
///
/// Counting installs over the whole module would mix the inlined `new` site
/// with the per-class standalone `<class>_constructor` symbol, which installs
/// the same fields for the cross-module construction path. Both are checked,
/// one at a time.
fn function_body<'a>(ir: &'a str, needle: &str) -> &'a str {
    let define_at = ir
        .match_indices("define ")
        .find(|(idx, _)| {
            let line_end = ir[*idx..].find('\n').map(|e| idx + e).unwrap_or(ir.len());
            ir[*idx..line_end].contains(needle)
        })
        .map(|(idx, _)| idx)
        .unwrap_or_else(|| panic!("no emitted function matching {needle}:\n{ir}"));
    let rest = &ir[define_at..];
    let end = rest.find("\n}").map(|e| e + 2).unwrap_or(rest.len());
    &rest[..end]
}

/// Byte offsets of every field-install call in `ir`.
fn field_install_offsets(ir: &str) -> Vec<usize> {
    let mut offsets: Vec<usize> = Vec::new();
    for needle in FIELD_INSTALL {
        let mut from = 0usize;
        while let Some(idx) = ir[from..].find(needle) {
            offsets.push(from + idx);
            from += idx + needle.len();
        }
    }
    offsets.sort_unstable();
    offsets
}

/// Assert `body` installs exactly `expected` fields, all of them after the
/// Error base's `super()` work.
fn assert_installs_after_super(body: &str, expected: usize, what: &str) {
    let super_at = body
        .find(CAPTURE_STACK)
        .unwrap_or_else(|| panic!("{what}: the Error super() arm must run:\n{body}"));
    let installs = field_install_offsets(body);
    assert_eq!(
        installs.len(),
        expected,
        "{what}: one install per declared field, no duplicate staging \
         (installs at {installs:?}):\n{body}"
    );
    assert!(
        installs.iter().all(|at| *at > super_at),
        "{what}: field initializers run AFTER super() returns (super at \
         {super_at}, installs at {installs:?}):\n{body}"
    );
}

#[test]
fn own_ctor_error_subclass_installs_its_fields_after_super() {
    let e = class(
        5,
        "E",
        "Error",
        vec![field("n", Expr::Integer(7))],
        Some(ctor(
            2,
            vec![Stmt::Expr(Expr::SuperCall(vec![Expr::String(
                "m".to_string(),
            )]))],
        )),
    );
    let ir = ir_for(vec![e], vec![Stmt::Expr(new_expr("E"))]);

    // The standalone `<class>_constructor` symbol is the body every `new E`
    // runs (the call site routes through it when the class owns a ctor), and
    // the one a cross-module `new` or a dynamic construct replay reaches.
    assert_installs_after_super(function_body(&ir, "E_constructor"), 1, "E ctor");
}

#[test]
fn an_error_rooted_chain_installs_each_level_once() {
    // `class Mid extends Error { m = 1; ctor }` + `class Leaf extends Mid
    // { n = 2; ctor }`: Mid is the chain ROOT, so before the fix its fields
    // were staged up front by the construction site AND (once the Error arm
    // started applying them) would have been installed a second time at its
    // own `super()`. Two fields, two installs.
    let mid = class(
        5,
        "Mid",
        "Error",
        vec![field("m", Expr::Integer(1))],
        Some(ctor(
            2,
            vec![Stmt::Expr(Expr::SuperCall(vec![Expr::String(
                "m".to_string(),
            )]))],
        )),
    );
    let leaf = class(
        6,
        "Leaf",
        "Mid",
        vec![field("n", Expr::Integer(2))],
        Some(ctor(
            3,
            vec![Stmt::Expr(Expr::SuperCall(vec![Expr::String(
                "m".to_string(),
            )]))],
        )),
    );
    let ir = ir_for(vec![mid, leaf], vec![Stmt::Expr(new_expr("Leaf"))]);

    // Leaf's constructor inlines Mid's body: `m` and `n`, once each.
    assert_installs_after_super(function_body(&ir, "Leaf_constructor"), 2, "Leaf ctor");
    // Mid's own standalone symbol installs only Mid's field.
    assert_installs_after_super(function_body(&ir, "Mid_constructor"), 1, "Mid ctor");
}
