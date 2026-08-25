//! #8132: a boxed local's slot must not be lowered as a GC root.
//!
//! A boxed local's alloca holds a `js_box_alloc_bits` result (or the
//! TAG_UNDEFINED sentinel) — never a GC-heap value. Boxes are `std::alloc`
//! allocations outside the GC heap: no collector phase moves them, box.rs
//! never frees them, and the JSValue inside is traced through the registered
//! `scan_box_roots_mut` scanner. Rooting the slot therefore protects nothing —
//! and under the RS4GC lowering it costs a relocation of the box pointer at
//! every statepoint it stays live across. On #8132's bundled webpack module
//! factory, ~300 preallocated boxes were live across ~90% of one function's
//! 5.5k statepoints: roughly a third of its 1.43M `gc.relocate`s protected
//! pointers the collector can never move.
//!
//! The fixture pairs the boxed local with an unboxed twin of the same shape
//! (`let plain = []`), so the assertions discriminate in both directions:
//! if the bind gate in `emit_shadow_slot_bind_for_local` is reverted, the box
//! pointer gets `inttoptr`-retyped into `addrspace(1)` and the negative
//! assertion fails; if binds were skipped wholesale, the twin's
//! `alloca ptr addrspace(1)` disappears and the premise assertion fails.

use crate::{compile_module, AppMetadata, CompileOptions};
use perry_hir::types::Type;
use perry_hir::{Expr, Function, Module, ModuleInitKind, Stmt};

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

const ACC: u32 = 101;
const BUMP: u32 = 102;
const PLAIN: u32 = 103;

/// `function f() { let acc = []; const bump = () => { acc = [1]; };
/// let plain = []; bump(); return plain; }`
///
/// `acc` is captured AND mutated by `bump`, so `collect_boxed_vars` boxes it;
/// `plain` has the identical pointer-bearing init but no capture, so it keeps
/// an ordinary rooted slot.
fn module_with_boxed_local() -> Module {
    let mut m = Module::new("boxed_root.ts");
    let f = Function {
        id: 1,
        name: "f".to_string(),
        type_params: Vec::new(),
        params: Vec::new(),
        return_type: Type::Any,
        body: vec![
            Stmt::Let {
                id: ACC,
                name: "acc".to_string(),
                ty: Type::Any,
                mutable: true,
                init: Some(Expr::Array(Vec::new())),
            },
            Stmt::Let {
                id: BUMP,
                name: "bump".to_string(),
                ty: Type::Any,
                mutable: false,
                init: Some(Expr::Closure {
                    func_id: 2,
                    params: Vec::new(),
                    return_type: Type::Any,
                    body: vec![Stmt::Expr(Expr::LocalSet(
                        ACC,
                        Box::new(Expr::Array(vec![Expr::Integer(1)])),
                    ))],
                    captures: vec![ACC],
                    mutable_captures: vec![ACC],
                    captures_this: false,
                    captures_new_target: false,
                    enclosing_class: None,
                    is_arrow: true,
                    is_async: false,
                    is_generator: false,
                    is_strict: false,
                }),
            },
            Stmt::Let {
                id: PLAIN,
                name: "plain".to_string(),
                ty: Type::Any,
                mutable: false,
                init: Some(Expr::Array(Vec::new())),
            },
            Stmt::Expr(Expr::Call {
                callee: Box::new(Expr::LocalGet(BUMP)),
                args: Vec::new(),
                type_args: Vec::new(),
                byte_offset: 0,
            }),
            Stmt::Return(Some(Expr::LocalGet(PLAIN))),
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
    m.functions.push(f);
    m.init_kind = ModuleInitKind::Eager;
    m
}

/// The `define` block that contains `needle` — the boxed function's body,
/// found by content rather than by name so the module-prefix naming scheme
/// cannot silently retarget the assertions.
fn define_containing<'a>(ir: &'a str, needle: &str) -> &'a str {
    let mut starts: Vec<usize> = vec![0];
    let mut from = 0usize;
    while let Some(i) = ir[from..].find("\ndefine ") {
        starts.push(from + i + 1);
        from = from + i + 1;
    }
    starts.push(ir.len());
    for pair in starts.windows(2) {
        let block = &ir[pair[0]..pair[1]];
        if block.starts_with("define") && block.contains(needle) {
            return block;
        }
    }
    panic!("no define block contains `{needle}`");
}

#[test]
fn a_boxed_locals_slot_is_not_a_native_gc_root() {
    let _native = crate::codegen::helpers::NativeRootsPin::native();
    let ir = String::from_utf8(compile_module(&module_with_boxed_local(), ir_opts()).unwrap())
        .expect("LLVM IR should be UTF-8");
    let f = define_containing(&ir, "@js_box_alloc_bits");

    // Premise 1: `acc` really is boxed — the fixture would otherwise assert
    // about a lowering that never ran.
    let alloc_line = f
        .lines()
        .find(|l| l.contains("call i64 @js_box_alloc_bits"))
        .expect("boxed local must allocate its box");
    let box_reg = alloc_line
        .trim()
        .split(" = ")
        .next()
        .expect("box alloc must name a result register")
        .to_string();

    // Premise 2: the unboxed twin keeps a rooted, addrspace(1)-retyped slot,
    // so the box slot's missing root below is the gate's doing — not a
    // collector that stopped rooting array-holding locals.
    assert!(
        f.contains("alloca ptr addrspace(1)"),
        "the unboxed twin must still lower a precise root:\n{f}"
    );

    // The point: the box pointer is stored as a plain i64 and is never
    // retyped into the GC address space, so RS4GC has nothing to relocate.
    assert!(
        f.contains(&format!("store i64 {box_reg},")),
        "box pointer must be stored to its plain i64 slot:\n{f}"
    );
    assert!(
        !f.contains(&format!("inttoptr i64 {box_reg} to ptr addrspace(1)")),
        "#8132: a boxed local's slot must not be lowered as an addrspace(1) \
         GC root — the box never moves and its contents are traced through \
         the box-registry scanner:\n{f}"
    );
}
