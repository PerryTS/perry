//! #11516: codegen half of the handle-free hash/HMAC chains.
//!
//! `perry_transform::crypto_hash_chain` rewrites a non-escaping
//! `createHash`/`createHmac` into `crypto.__perryHash*` calls (its own unit
//! tests pin which shapes are rewritten). These tests pin what codegen does
//! with them: each init site gets a 16-aligned frame slot in the entry block
//! and the chain lowers to the `js_crypto_chain_*` entry points, with no
//! handle-creating `js_crypto_create_hash`/`js_crypto_create_hmac` call and no
//! generic `js_native_call_method` dispatch. An escaping `createHash` (the
//! shape the pass leaves alone) still registers a handle.
//!
//! Assertions match CALL SITES (`call double @js_...(`), never the `declare`
//! lines, which are emitted unconditionally.

use perry_codegen::{compile_module, AppMetadata, CompileOptions};
use perry_hir::crypto_chain::{CHAIN_DIGEST, CHAIN_INIT_HASH, CHAIN_INIT_HMAC, CHAIN_UPDATE};
use perry_hir::types::Type;
use perry_hir::{Expr, Module, ModuleInitKind, Stmt};

fn entry_opts() -> CompileOptions {
    CompileOptions {
        static_shape_ids: Vec::new(),
        program_class_shape_ids: Default::default(),
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
        constructor_param_counts: Default::default(),
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
        program_is_synchronous: false,
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
        fp_contract_mode: perry_codegen::FpContractMode::Off,
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

fn module_with_init(name: &str, init: Vec<Stmt>) -> Module {
    Module {
        name: name.to_string(),
        imports: Vec::new(),
        exports: Vec::new(),
        classes: Vec::new(),
        interfaces: Vec::new(),
        type_aliases: Vec::new(),
        enums: Vec::new(),
        globals: Vec::new(),
        functions: Vec::new(),
        script_global_functions: Vec::new(),
        references_global_this: false,
        annexb_global_undefined_names: Vec::new(),
        init_is_strict: false,
        init,
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

/// Compile `body` as the body of a plain function and return the module IR.
fn ir_for_fn_body(name: &str, body: Vec<Stmt>) -> String {
    let mut m = module_with_init(name, Vec::new());
    m.functions.push(perry_hir::Function {
        id: 7000,
        name: "driver".to_string(),
        type_params: Vec::new(),
        params: Vec::new(),
        return_type: Type::Any,
        body,
        is_async: false,
        is_generator: false,
        is_strict: false,
        is_exported: false,
        captures: Vec::new(),
        decorators: Vec::new(),
        was_plain_async: false,
        was_unrolled: false,
    });
    String::from_utf8(compile_module(&m, entry_opts()).unwrap()).expect("LLVM IR should be UTF-8")
}

fn crypto() -> Expr {
    Expr::NativeModuleRef("crypto".to_string())
}

fn call(recv: Expr, method: &str, args: Vec<Expr>) -> Expr {
    Expr::Call {
        callee: Box::new(Expr::PropertyGet {
            object: Box::new(recv),
            property: method.to_string(),
            byte_offset: 0,
        }),
        args,
        type_args: Vec::new(),
        byte_offset: 0,
    }
}

fn s(v: &str) -> Expr {
    Expr::String(v.to_string())
}

fn assert_handle_free(ir: &str, init: &str) {
    assert!(
        ir.contains("alloca [1024 x i8], align 16"),
        "each init site needs its own aligned frame slot:\n{ir}"
    );
    assert!(
        ir.contains(&format!("call double @{init}(")),
        "missing {init}:\n{ir}"
    );
    assert!(ir.contains("call double @js_crypto_chain_update("), "{ir}");
    assert!(ir.contains("call double @js_crypto_chain_digest("), "{ir}");
    for handle_call in [
        "call double @js_crypto_create_hash(",
        "call double @js_crypto_create_hash_options(",
        "call double @js_crypto_create_hmac(",
    ] {
        assert!(
            !ir.contains(handle_call),
            "no handle may be registered ({handle_call}):\n{ir}"
        );
    }
}

/// `crypto.createHash("sha1").update(x).update(y, "utf8").digest("hex")` as
/// the pass emits it.
#[test]
fn hash_chain_lowers_to_frame_state_calls() {
    let init = call(crypto(), CHAIN_INIT_HASH, vec![s("sha1")]);
    let u1 = call(crypto(), CHAIN_UPDATE, vec![init, s("request-path")]);
    let u2 = call(crypto(), CHAIN_UPDATE, vec![u1, s("more"), s("utf8")]);
    let digest = call(crypto(), CHAIN_DIGEST, vec![u2, s("hex")]);
    let ir = ir_for_fn_body("hash_chain", vec![Stmt::Return(Some(digest))]);
    assert_handle_free(&ir, "js_crypto_chain_hash_init");
    let chain_calls = ir.matches("call double @js_crypto_chain_update(").count();
    assert_eq!(chain_calls, 2, "one runtime call per update:\n{ir}");
    assert!(!ir.contains("call double @js_native_call_method("), "{ir}");
}

/// The block-local form: `const h = <init>; <update>(h, x); return <digest>(h)`.
#[test]
fn block_local_hmac_lowers_to_frame_state_calls() {
    let body = vec![
        Stmt::Let {
            id: 1,
            name: "h".to_string(),
            ty: Type::Any,
            mutable: false,
            init: Some(call(crypto(), CHAIN_INIT_HMAC, vec![s("sha256"), s("key")])),
        },
        Stmt::Expr(call(
            crypto(),
            CHAIN_UPDATE,
            vec![Expr::LocalGet(1), s("payload")],
        )),
        Stmt::Return(Some(call(crypto(), CHAIN_DIGEST, vec![Expr::LocalGet(1)]))),
    ];
    let ir = ir_for_fn_body("hmac_local", body);
    assert_handle_free(&ir, "js_crypto_chain_hmac_init");
}

/// Two init sites in one function must not share a slot.
#[test]
fn every_init_site_gets_its_own_slot() {
    let chain = |alg: &str| {
        let init = call(crypto(), CHAIN_INIT_HASH, vec![s(alg)]);
        let u = call(crypto(), CHAIN_UPDATE, vec![init, s("data-bytes")]);
        call(crypto(), CHAIN_DIGEST, vec![u, s("hex")])
    };
    let body = vec![
        Stmt::Expr(chain("md5")),
        Stmt::Return(Some(chain("sha512"))),
    ];
    let ir = ir_for_fn_body("two_sites", body);
    assert_eq!(
        ir.matches("alloca [1024 x i8], align 16").count(),
        2,
        "{ir}"
    );
}

/// An escaping hash (left alone by the pass) keeps the registered handle.
#[test]
fn escaping_create_hash_still_registers_a_handle() {
    let body = vec![Stmt::Return(Some(call(
        crypto(),
        "createHash",
        vec![s("sha1")],
    )))];
    let ir = ir_for_fn_body("escaping", body);
    assert!(ir.contains("call double @js_crypto_create_hash("), "{ir}");
    assert!(
        !ir.contains("call double @js_crypto_chain_hash_init("),
        "{ir}"
    );
}
