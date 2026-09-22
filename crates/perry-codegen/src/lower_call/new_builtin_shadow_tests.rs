//! #10589: `new X(...)` where `X` is a bare identifier whose NAME collides
//! with an unconditional (not `required_sources`-gated) builtin constructor
//! arm in `lower_builtin_new` must build the USER'S imported binding when one
//! exists, not the builtin.
//!
//! `lower_new_impl_inner` called `lower_builtin_new` for any `class_name`
//! absent from `ctx.classes` before ever checking `ctx.import_function_prefixes`
//! (where an imported PLAIN FUNCTION constructor is tracked — imported
//! CLASSES already land in `ctx.classes` and skip this block entirely, see
//! the `Headers`-as-class regression guard below, which passed before this
//! fix too). Since `"Headers"` has no `required_sources` gate, it fired
//! unconditionally.
//!
//! `new X()` and `new (X as any)()` are equivalent from `lower_new_impl_inner`
//! downward — HIR's `peel_new_callee` strips a `TsAs` cast before `lower_new`
//! ever branches on the callee's shape, so both forms lower to the identical
//! `Expr::New { class_name: "Headers", .. }`. There is deliberately no
//! separate "cast" test here for that reason; the two forms are provably one
//! code path once the AST reaches `Expr::New`.

use crate::{compile_module, CompileOptions, ImportedClass};
use perry_hir::{Expr, Module, Stmt};

fn new_headers_call() -> Module {
    let mut module = Module::new("new_builtin_shadow.ts");
    module.init = vec![Stmt::Expr(Expr::New {
        class_name: "Headers".to_string(),
        args: Vec::new(),
        type_args: Vec::new(),
        byte_offset: 0,
        cap_args_appended: 0,
    })];
    module
}

fn compile(opts: CompileOptions) -> String {
    let bytes = compile_module(&new_headers_call(), opts).expect("module compiles");
    String::from_utf8(bytes).expect("LLVM IR is UTF-8")
}

#[test]
fn imported_function_constructor_shadows_the_builtin_arm() {
    let mut opts = CompileOptions {
        emit_ir_only: true,
        ..Default::default()
    };
    opts.import_function_prefixes
        .insert("Headers".to_string(), "lib_ts".to_string());
    let ir = compile(opts);

    assert!(
        ir.contains("call double @js_new_function_construct("),
        "an imported function constructor named `Headers` must construct \
         through the imported-function path:\n{ir}"
    );
    assert!(
        !ir.contains("call double @js_headers_new("),
        "the builtin fetch Headers constructor must not fire once `Headers` \
         resolves to an imported function (#10589):\n{ir}"
    );
}

#[test]
fn unshadowed_builtin_name_still_builds_the_builtin() {
    // No `import_function_prefixes` entry for "Headers": nothing shadows the
    // name, so the builtin fetch API constructor must still fire. Guards
    // against an overly broad fix that stops builtin `new Headers()` from
    // working when the program never imports anything of that name.
    let ir = compile(CompileOptions {
        emit_ir_only: true,
        ..Default::default()
    });

    assert!(
        ir.contains("call double @js_headers_new("),
        "an unshadowed `Headers` must still build the builtin:\n{ir}"
    );
    assert!(
        !ir.contains("call double @js_new_function_construct("),
        "nothing resolves this name to an imported function value:\n{ir}"
    );
}

#[test]
fn v8_fallback_import_of_the_same_name_still_builds_the_builtin() {
    // A V8-fallback specifier for "Headers" (present in
    // `import_function_prefixes` but ALSO in `import_function_v8_specifiers`)
    // is not a compiled-source binding this fix can construct via
    // `js_new_function_construct` — it must keep falling through to the
    // builtin, same as the codegen's existing `import_function_v8_specifiers`
    // exclusion at the later `import_function_prefixes` arm.
    let mut opts = CompileOptions {
        emit_ir_only: true,
        ..Default::default()
    };
    opts.import_function_prefixes
        .insert("Headers".to_string(), "lib_ts".to_string());
    opts.import_function_v8_specifiers
        .insert("Headers".to_string(), "./lib.ts".to_string());
    let ir = compile(opts);

    assert!(
        ir.contains("call double @js_headers_new("),
        "a V8-fallback import must still build the builtin:\n{ir}"
    );
}

#[test]
fn imported_class_of_the_same_name_already_shadowed_the_builtin() {
    // Regression guard for the OTHER half of the shadowing story, unchanged
    // by this fix: an imported CLASS named "Headers" lands in `ctx.classes`
    // and always skipped the builtin block, function-constructor collisions
    // aside.
    let mut opts = CompileOptions {
        emit_ir_only: true,
        ..Default::default()
    };
    opts.imported_classes.push(ImportedClass {
        name: "Headers".to_string(),
        local_alias: None,
        namespace: None,
        source_prefix: "lib_ts".to_string(),
        constructor_param_count: 0,
        constructor_has_synthetic_arguments: false,
        has_own_constructor: false,
        constructor_has_rest: false,
        has_instance_fields: false,
        method_names: Vec::new(),
        proven_this_method_names: Vec::new(),
        proven_this_tower_method_names: Vec::new(),
        method_return_types: Vec::new(),
        method_param_counts: Vec::new(),
        method_has_rest: Vec::new(),
        method_has_synthetic_arguments: Vec::new(),
        method_arguments_length_only: Vec::new(),
        static_field_names: Vec::new(),
        static_method_names: Vec::new(),
        static_method_return_types: Vec::new(),
        static_method_param_counts: Vec::new(),
        static_method_has_rest: Vec::new(),
        static_method_has_user_rest: Vec::new(),
        static_method_has_synthetic_arguments: Vec::new(),
        getter_names: Vec::new(),
        getter_return_types: Vec::new(),
        setter_names: Vec::new(),
        parent_name: None,
        field_names: Vec::new(),
        field_types: Vec::new(),
        source_class_id: Some(9101),
        return_shape_imports: Vec::new(),
        object_literal: None,
    });
    let ir = compile(opts);

    assert!(
        !ir.contains("call double @js_headers_new("),
        "an imported class must already shadow the builtin, before and \
         after #10589's fix:\n{ir}"
    );
}
