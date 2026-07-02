//! Module-entry function emission. Split out of `codegen.rs` (now `codegen/mod.rs`).

use std::collections::{HashMap, HashSet};

use anyhow::{Context, Result};
use perry_hir::Module as HirModule;

use crate::expr::FnCtx;
use crate::module::LlModule;
use crate::stmt;
use crate::strings::StringPool;
use crate::types::{DOUBLE, I32, I64, I8, PTR, VOID};

use super::helpers::{
    emit_namespace_populator, enable_module_init_shadow_frame, init_static_fields_early,
    init_static_fields_late, is_macos_triple, register_module_globals_as_gc_roots,
    write_barriers_enabled,
};
use super::opts::CrossModuleCtx;

/// Emit the plugin ABI shim — `perry_plugin_abi_version`, `plugin_activate`,
/// and (when the user exports `deactivate`) `plugin_deactivate` — for a
/// dylib/staticlib's **entry** module.
///
/// The host's `perry_plugin_load`/`perry_plugin_unload`
/// (`crates/perry-runtime/src/plugin.rs`) resolve exactly these names via
/// `dlsym` / `GetProcAddress`, and the Windows link step lists them in the
/// generated `.def` (see `compile_module_entry`'s caller in
/// `crates/perry/src/commands/compile.rs`). A plugin dylib must therefore
/// export them.
///
/// These are emitted from the entry module only — the file passed on the
/// command line, where the dylib's top-level `activate`/`deactivate` exports
/// live. Issue #5273: this previously sat in the non-entry-module branch, so a
/// single-file plugin (which IS the entry module) got none of the three
/// symbols and failed to load, while a hypothetical multi-module plugin would
/// have emitted `perry_plugin_abi_version` once per non-entry module and failed
/// to link on the duplicate symbol.
///
/// `perry_plugin_abi_version` returns the ABI version the runtime checks
/// (`PLUGIN_ABI_VERSION` in `plugin.rs` — keep in sync). `plugin_activate`
/// unwraps the NaN-boxed `api` handle and calls the user's `activate(api)`,
/// returning 1 on success / 0 if the module doesn't export `activate` (the host
/// treats 0 / a missing user `activate` as load failure).
fn emit_plugin_abi_shim(llmod: &mut LlModule, hir: &HirModule, module_prefix: &str) {
    use crate::codegen::helpers::scoped_fn_name;
    use crate::nanbox::{POINTER_MASK_I64, POINTER_TAG_I64};

    let has_plugin_activate = hir
        .exported_functions
        .iter()
        .any(|(name, _)| name == "activate");
    let has_plugin_deactivate = hir
        .exported_functions
        .iter()
        .any(|(name, _)| name == "deactivate");

    {
        let abi_fn = llmod.define_function("perry_plugin_abi_version", I64, vec![]);
        let _ = abi_fn.create_block("entry");
        let blk = abi_fn.block_mut(0).unwrap();
        blk.ret(I64, "2");
    }

    if has_plugin_activate {
        let user_activate = scoped_fn_name(module_prefix, "activate");
        llmod.declare_function(&user_activate, DOUBLE, &[DOUBLE]);
        let fn_def = llmod.define_function(
            "plugin_activate",
            I64,
            vec![(I64, "%api_handle".to_string())],
        );
        let _ = fn_def.create_block("entry");
        let blk = fn_def.block_mut(0).unwrap();
        let lower48 = blk.and(I64, "%api_handle", POINTER_MASK_I64);
        let tagged = blk.or(I64, &lower48, POINTER_TAG_I64);
        let boxed = blk.bitcast_i64_to_double(&tagged);
        let _ = blk.call(DOUBLE, &user_activate, &[(DOUBLE, &boxed)]);
        blk.ret(I64, "1");
    } else {
        let fn_def = llmod.define_function(
            "plugin_activate",
            I64,
            vec![(I64, "%_api_handle".to_string())],
        );
        let _ = fn_def.create_block("entry");
        let blk = fn_def.block_mut(0).unwrap();
        blk.ret(I64, "0");
    }

    if has_plugin_deactivate {
        let user_deactivate = scoped_fn_name(module_prefix, "deactivate");
        llmod.declare_function(&user_deactivate, DOUBLE, &[]);
        let fn_def = llmod.define_function("plugin_deactivate", VOID, vec![]);
        let _ = fn_def.create_block("entry");
        let blk = fn_def.block_mut(0).unwrap();
        // The user's `deactivate` is declared/defined as `double ()` (every
        // lowered TS function returns a NaN-boxed value), so call it as
        // `double` and discard the result — mirroring the `activate` path
        // above. A `call_void` here would emit `call void @<deactivate>()`,
        // a signature mismatch against the `double` definition.
        let _ = blk.call(DOUBLE, &user_deactivate, &[]);
        blk.ret_void();
    }
}

/// Collect the entry module's top-level `process.env.<NAME> = "<literal>"`
/// assignments so they can be applied to the OS environment BEFORE eager
/// module init (see the call site in `compile_module_entry`).
///
/// Node runs the entry script top-to-bottom, so a `process.env.NODE_ENV =
/// 'production'` on line 1 is observed by every `require()`d dependency's
/// init. Perry hoists `require`s to eager imports that init before the entry
/// body runs, so without this the dependency observes the unmodified env —
/// e.g. `react-dom/index.js` branches on `process.env.NODE_ENV === 'production'`
/// to pick the production vs development bundle, and the development file is
/// pruned from a Next.js standalone build, so the wrong branch yields an empty
/// module and a downstream `ReactDOMSharedInternals.d` crash.
///
/// Only *unconditional module-top-level* assignments are collected: the entry
/// init statements, plus one+ levels into a cjs-wrap IIFE (`_cjs =
/// (function(){ ... })()`), which is where the wrapped entry's top-level
/// statements live. Assignments nested in conditionals or inner functions are
/// deliberately skipped — those run conditionally/lazily, exactly as in Node.
fn collect_entry_env_literals(init: &[perry_hir::Stmt]) -> Vec<(String, String)> {
    use perry_hir::{Expr, Stmt};

    fn record(expr: &Expr, out: &mut Vec<(String, String)>) {
        // `process.env.X = "lit"` lowers to either form depending on path.
        if let Expr::PutValueSet {
            target, key, value, ..
        } = expr
        {
            if matches!(target.as_ref(), Expr::ProcessEnv) {
                if let (Expr::String(k), Expr::String(v)) = (key.as_ref(), value.as_ref()) {
                    out.push((k.clone(), v.clone()));
                }
            }
        }
        if let Expr::PropertySet {
            object,
            property,
            value,
        } = expr
        {
            if matches!(object.as_ref(), Expr::ProcessEnv) {
                if let Expr::String(v) = value.as_ref() {
                    out.push((property.clone(), v.clone()));
                }
            }
        }
    }

    fn descend_iife(expr: &Expr, out: &mut Vec<(String, String)>, depth: u32) {
        if depth >= 4 {
            return;
        }
        if let Expr::Call { callee, .. } = expr {
            if let Expr::Closure { body, .. } = callee.as_ref() {
                scan(body, out, depth + 1);
            }
        }
    }

    fn scan(stmts: &[Stmt], out: &mut Vec<(String, String)>, depth: u32) {
        for s in stmts {
            match s {
                Stmt::Expr(e) => {
                    record(e, out);
                    descend_iife(e, out, depth);
                }
                Stmt::Let { init: Some(e), .. } => descend_iife(e, out, depth),
                _ => {}
            }
        }
    }

    let mut out = Vec::new();
    scan(init, &mut out, 0);
    out
}

/// Emit the module's entry function.
///
/// For the **entry module**: emits `int main()` that bootstraps GC, runs
/// the entry module's own string pool init, then calls every non-entry
/// module's `<prefix>__init` function in order, then runs the entry
/// module's top-level statements, then `return 0`.
///
/// #5579: emit the global-object reflection of a Script's bare top-level
/// `function` declarations (`globalThis[name] = <fn>`). Called from the
/// entry-module branch only for non-ESM programs, before user init runs.
///
/// Each name is reflected with a heap closure built exactly as `Expr::FuncRef`
/// does (`js_closure_alloc_singleton(@__perry_wrap_<sym>)`), so the property
/// value is callable and `typeof globalThis[name] === "function"`. The
/// `hir.script_global_functions` list is already deduped (last declaration
/// wins) and excludes nested closures / object-literal methods, which must
/// not pollute the global object.
fn emit_script_global_function_decls(ctx: &mut FnCtx<'_>, hir: &HirModule) {
    for (name, fid) in &hir.script_global_functions {
        if ctx.block().is_terminated() {
            break;
        }
        let func_name = match ctx.func_names.get(fid) {
            Some(n) => n.clone(),
            None => continue,
        };
        let wrap_ptr = format!("@__perry_wrap_{}", func_name);
        let key_idx = ctx.strings.intern(name);
        let key_handle_global = format!("@{}", ctx.strings.entry(key_idx).handle_global);
        let blk = ctx.block();
        let global_box = blk.call(DOUBLE, "js_get_global_this", &[]);
        let obj_raw = crate::expr::unbox_to_i64(blk, &global_box);
        let closure_handle = blk.call(I64, "js_closure_alloc_singleton", &[(PTR, &wrap_ptr)]);
        let closure_box = crate::expr::nanbox_pointer_inline(blk, &closure_handle);
        let key_box = blk.load(DOUBLE, &key_handle_global);
        let key_raw = crate::expr::unbox_to_i64(blk, &key_box);
        // #5833: GlobalDeclarationInstantiation's `CreateGlobalFunctionBinding`
        // runs with `D = false` for a Script (only sloppy-eval's Annex B.3.3.3
        // path uses `D = true`), so the reflected property must be
        // non-configurable — a plain `js_object_set_field_by_name` created it
        // configurable, failing `verifyProperty(this, name, {configurable:
        // false})` (test262 `language/global-code/decl-func.js`).
        blk.call_void(
            "js_object_set_field_by_name_nonconfigurable",
            &[(I64, &obj_raw), (I64, &key_raw), (DOUBLE, &closure_box)],
        );
    }
}

/// For **non-entry modules**: emits `void <prefix>__init()` that runs the
/// non-entry module's string pool init followed by its top-level
/// statements. The entry module's main calls these via the
/// `non_entry_module_prefixes` list.
///
/// Each module gets its OWN string pool init function
/// (`__perry_init_strings_<prefix>`) so multiple modules in the same
/// program don't collide on the symbol name.
#[allow(clippy::too_many_arguments)]
pub(super) fn compile_module_entry(
    llmod: &mut LlModule,
    hir: &HirModule,
    func_names: &HashMap<u32, String>,
    strings: &mut StringPool,
    classes: &HashMap<String, &perry_hir::Class>,
    methods: &HashMap<(String, String), String>,
    module_globals: &HashMap<u32, String>,
    import_function_prefixes: &HashMap<String, String>,
    enums: &HashMap<(String, String), perry_hir::EnumValue>,
    static_field_globals: &HashMap<(String, String), String>,
    class_ids: &HashMap<String, u32>,
    func_signatures: &HashMap<u32, (usize, bool, bool, bool)>,
    func_synthetic_arguments: &std::collections::HashSet<u32>,
    module_prefix: &str,
    is_entry: bool,
    non_entry_module_prefixes: &[String],
    module_boxed_vars: &std::collections::HashSet<u32>,
    closure_rest_params: &HashMap<u32, usize>,
    cross_module: &CrossModuleCtx,
    output_type: &str,
    // Issue #100: parallel-to-`cross_module.namespace_entries` list of
    // `(string_constant_global_name, byte_len)` for each export-key.
    // The populator emits one `getelementptr` per key into the stack
    // keys array — `byte_len` becomes the corresponding entry in the
    // key-lengths array passed to `js_create_namespace`. Empty when
    // this module is not a dynamic-import target.
    namespace_key_globals: &[(String, usize)],
) -> Result<()> {
    let strings_init_name = format!("__perry_init_strings_{}", module_prefix);

    // #1088 — staticlib output is functionally identical to dylib at the
    // codegen layer: both expose `perry_module_init` instead of `main`, both
    // skip the embedded event loop (host drives it), both skip the
    // app-group/geisterhand init that only makes sense for a stand-alone
    // executable. The variable name stays for diff hygiene with the
    // historical dylib-only branches downstream.
    let is_dylib = output_type == "dylib" || output_type == "staticlib";

    if is_entry {
        // Pre-declare each non-entry module's init function as an
        // extern so the entry main can call them. The actual definition
        // lives in the OTHER module's compiled .o file; the linker
        // resolves the symbols at link time.
        for prefix in non_entry_module_prefixes {
            llmod.declare_function(&format!("{}__init", prefix), VOID, &[]);
        }
        // Issue #753: emit a no-op `<entry_prefix>__init` stub so the
        // dispatch site in some other module that does `await
        // import("./entry.ts")` resolves at link time. The entry
        // module's actual body runs in `main`, not in a separate
        // `__init` — the stub exists purely to satisfy the dispatch's
        // unconditional init call. The namespace populator at the
        // tail of `main` (when `cross_module.namespace_entries` is
        // non-empty) is what makes the entry observable through the
        // dynamic-import namespace; the stub does no work.
        {
            let stub_name = format!("{}__init", module_prefix);
            let stub = llmod.define_function(&stub_name, VOID, vec![]);
            let _ = stub.create_block("entry");
            stub.block_mut(0).unwrap().ret_void();
        }

        // For dylib output, emit `void perry_module_init()` instead of
        // `int main()`. The host process calls this once after dlopen to
        // initialize the GC, string pools, module globals (including GC
        // root registration), and run top-level statements. Without this,
        // module-level Maps/Arrays would never be registered as GC roots
        // and the first GC cycle after connect() would free them (issue #54).
        let ic_base = llmod.ic_counter;
        let buffer_alias_base = llmod.buffer_alias_counter;
        // Declare `perry_geisterhand_start` BEFORE `main` is created — once
        // `main` holds a mutable borrow on `llmod`, no further
        // `llmod.declare_function` calls are allowed. Inline (not in
        // `runtime_decls`) because most builds don't link geisterhand.
        if cross_module.needs_geisterhand && !is_dylib {
            llmod.declare_function("perry_geisterhand_start", VOID, &[I32]);
        }
        // #1178 — bake `[ios] app_group` from perry.toml into a single
        // `perry_app_group_init(ptr, len)` call at the top of `main`,
        // before any user code runs (and before any `appGroupSet/Get/
        // Delete` site could fire). Skipped entirely when the manifest
        // doesn't configure a suite, so non-App-Group apps pay no extra
        // bytes. Allocated up-front while `llmod` is still mutable —
        // `main` claims the borrow below.
        let app_group_init: Option<(String, usize)> = if is_dylib {
            None
        } else {
            cross_module
                .app_metadata
                .app_group
                .as_deref()
                .filter(|s| !s.is_empty())
                .map(|suite| llmod.add_string_constant(suite))
        };
        // Next.js wall 54 (part 2): emit a string constant for every Deferred
        // `.next/server/**` module path now (before `main` borrows `llmod`); the
        // registration calls go in the block below. `(string_const_name,
        // byte_len, sanitized_prefix)`.
        let nextjs_path_inits: Vec<(String, usize, String)> = if is_dylib {
            Vec::new()
        } else {
            cross_module
                .nextjs_path_init_modules
                .iter()
                .map(|(path, prefix)| {
                    let (cn, len) = llmod.add_string_constant(path);
                    (cn, len, prefix.clone())
                })
                .collect()
        };
        let main = if is_dylib {
            llmod.define_function("perry_module_init", VOID, vec![])
        } else {
            // Allow the host build to override the C entry symbol. On arm64_32
            // watchOS we can't rename `_main → __perry_user_main` after the
            // fact (rust-objcopy's MachOWriter crashes on arm64_32 objects), so
            // we emit the final symbol directly. Pass e.g. `_perry_user_main`
            // (the leading underscore yields Mach-O `__perry_user_main`, which
            // the Swift `@main` shell references via @_silgen_name).
            let entry_name =
                std::env::var("PERRY_ENTRY_SYMBOL").unwrap_or_else(|_| "main".to_string());
            llmod.define_function(&entry_name, I32, vec![])
        };
        main.add_pre_return_void_call("js_typed_feedback_maybe_dump_trace");
        let _ = main.create_block("entry");
        {
            let blk = main.block_mut(0).unwrap();
            blk.call_void("js_gc_init", &[]);
            if write_barriers_enabled() {
                blk.call_void("js_gc_write_barriers_emitted", &[(I32, "1")]);
            }
            // macOS `.app` assets live in `Contents/Resources/`, but a Finder
            // launch starts at CWD=`/`. chdir there before any user code or
            // native engine init so relative asset paths (`assets/...`) resolve.
            // No-op on non-macOS and on non-bundle binaries (see the runtime fn).
            // Emitted only for macOS triples (#4856): the runtime fn is a no-op
            // everywhere else anyway, and referencing it from every `main` made
            // iOS/tvOS links depend on non-macOS runtime archives carrying a
            // macOS-only symbol — a stale cross runtime then failed the link
            // with `undefined symbol: _perry_macos_bundle_chdir`.
            if is_macos_triple(&cross_module.target_triple) {
                blk.call_void("perry_macos_bundle_chdir", &[]);
            }
            if let Some((const_name, byte_len)) = app_group_init.as_ref() {
                let suite_ptr = format!("@{}", const_name);
                let len_str = byte_len.to_string();
                blk.call_void(
                    "perry_app_group_init",
                    &[(PTR, suite_ptr.as_str()), (I32, len_str.as_str())],
                );
            }
            // Wire up stdlib HANDLE_METHOD_DISPATCH eagerly when stdlib is
            // linked. Previously this was only called from
            // `ensure_pump_registered`, which fires lazily on the first
            // deferred-promise resolution — so sync-only programs (e.g.
            // pure crypto/hash pipelines — issue #86) never registered
            // the dispatcher and handle-based method calls fell through
            // to `js_native_call_method` which returned a non-Perry NaN
            // (`typeof === 'number'`). Guarded on `needs_stdlib` because
            // the runtime-only link doesn't pull in the stub symbol.
            if cross_module.needs_stdlib {
                blk.call_void("js_stdlib_init_dispatch", &[]);
            }
            // Start the Geisterhand HTTP inspector if requested. The
            // port comes from `--geisterhand-port` (default 7676). Calling
            // `perry_geisterhand_start` here also pins the geisterhand
            // server module against macOS's lazy-load `-dead_strip`, so
            // the inspector_ui HTML embedded via `include_str!` makes it
            // into the final binary instead of being eliminated as
            // unreferenced rodata.
        }
        if !is_dylib && cross_module.needs_geisterhand {
            // Function was declared above (before `main` claimed
            // `&mut llmod`). Lifetime: `port_str` lives for the body of
            // this block, long enough for `call_void` to consume the
            // `&str` reference.
            let port_str = cross_module.geisterhand_port.to_string();
            let blk = main.block_mut(0).unwrap();
            blk.call_void("perry_geisterhand_start", &[(I32, port_str.as_str())]);
        }
        {
            let blk = main.block_mut(0).unwrap();
            // Entry module's own string pool first.
            blk.call_void(&strings_init_name, &[]);
            // Apply the entry module's top-level `process.env.<NAME> =
            // "<literal>"` assignments NOW — after the string pool is live but
            // BEFORE any dependency's `__init` runs — so eager-inited deps that
            // branch on `process.env` at init time observe what the entry sets,
            // matching Node's require-is-lazy ordering. See
            // `collect_entry_env_literals`. The "NODE_ENV"/"production" string
            // handles are interned here and populated by the strings-init call
            // above (the entry body also references them, so they share slots).
            for (name, value) in collect_entry_env_literals(&hir.init) {
                let name_idx = strings.intern(&name);
                let value_idx = strings.intern(&value);
                let name_global = format!("@{}", strings.entry(name_idx).handle_global);
                let value_global = format!("@{}", strings.entry(value_idx).handle_global);
                let name_box = blk.load(DOUBLE, &name_global);
                let name_bits = blk.bitcast_double_to_i64(&name_box);
                let name_handle = blk.and(I64, &name_bits, crate::nanbox::POINTER_MASK_I64);
                let value_box = blk.load(DOUBLE, &value_global);
                blk.call_void("js_setenv", &[(I64, &name_handle), (DOUBLE, &value_box)]);
            }
            // Then every non-entry module's init in order. Each
            // non-entry module's `<prefix>__init` runs its own string
            // pool init internally before its top-level statements.
            //
            // Issue #753: skip Deferred modules — those reached only
            // through dynamic `import()` edges. Their `<prefix>__init`
            // fires lazily from each `Expr::DynamicImport` dispatch
            // site, idempotently guarded by `@__perry_init_done_<prefix>`
            // so a program that never reaches the dispatch never pays
            // the startup cost. The extern declaration at line ~3947
            // still emits for every non-entry prefix so the dispatch
            // site can resolve the symbol at link time.
            for prefix in non_entry_module_prefixes {
                if cross_module.deferred_module_prefixes.contains(prefix) {
                    continue;
                }
                blk.call_void(&format!("{}__init", prefix), &[]);
            }
            // Next.js wall 54 (part 2): record each Deferred `.next/server/**`
            // module's `__init` address under its absolute path so a runtime
            // `require(absolutePath)` (turbopack page/chunk loading) can trigger
            // its init lazily. No init runs here — only the address is recorded.
            // The `<prefix>__init` symbols are already declared above for every
            // non-entry prefix, so `ptrtoint` of the symbol resolves at link.
            for (const_name, byte_len, prefix) in &nextjs_path_inits {
                let path_ptr = format!("@{}", const_name);
                let len_str = byte_len.to_string();
                let init_addr = format!("ptrtoint (ptr @{}__init to i64)", prefix);
                blk.call_void(
                    "js_register_path_init",
                    &[
                        (PTR, path_ptr.as_str()),
                        (I64, len_str.as_str()),
                        (I64, init_addr.as_str()),
                    ],
                );
            }
        }
        // Mark the boundary between init prelude and user code so
        // hoisted post-init setup (cached `@perry_class_keys_*` loads
        // for the inline allocator) is spliced AFTER the init calls.
        // Without this, the load reads the global before
        // `__perry_init_strings_*` populates it — `keys_array` is null
        // on every freshly allocated object and field-by-name lookup
        // returns undefined.
        main.mark_entry_init_boundary();
        let flat_const_ids: std::collections::HashSet<u32> =
            cross_module.flat_const_arrays.keys().copied().collect();
        let (main_shadow_slot_map, main_shadow_slot_clears_after_stmt) =
            enable_module_init_shadow_frame(main, &hir.init, &flat_const_ids);

        let main_boxed_vars = module_boxed_vars.clone();
        let clamp_fn_ids: std::collections::HashSet<u32> = cross_module
            .clamp3_functions
            .union(&cross_module.clamp_u8_functions)
            .chain(cross_module.returns_int_functions.iter())
            .copied()
            .collect();
        let main_native_facts = crate::collectors::collect_native_region_fact_graph(
            &hir.init,
            &flat_const_ids,
            &clamp_fn_ids,
            &cross_module.clamp3_functions,
            &main_boxed_vars,
            module_globals,
            classes,
            &cross_module.compile_time_constants,
        );
        let mut init_local_types: HashMap<u32, perry_types::Type> = HashMap::new();
        crate::boxed_vars::collect_let_types_in_stmts(&hir.init, &mut init_local_types);
        let mut ctx = FnCtx {
            func: main,
            module_slug: crate::expr::native_region_slug(strings.module_prefix()),
            source_function: "module_init".to_string(),
            source_function_slug: crate::expr::native_region_slug("module_init"),
            active_region_id: None,
            native_facts: &main_native_facts,
            locals: HashMap::new(),
            local_types: init_local_types,
            current_block: 0,
            discard_expr_value: false,
            func_names,
            strings,
            loop_targets: Vec::new(),
            label_targets: HashMap::new(),
            pending_labels: Vec::new(),
            classes,
            this_stack: Vec::new(),
            inline_ctor_return: Vec::new(),
            new_target_stack: Vec::new(),
            class_stack: Vec::new(),
            methods,
            module_globals,
            import_function_prefixes,
            import_function_origin_names: &cross_module.import_function_origin_names,
            import_function_v8_specifiers: &cross_module.import_function_v8_specifiers,
            // Issue #841: node:submodule named-import + namespace registries.
            import_function_node_submodule: &cross_module.import_function_node_submodule,
            namespace_node_submodules: &cross_module.namespace_node_submodules,
            namespace_v8_specifiers: &cross_module.namespace_v8_specifiers,
            closure_captures: HashMap::new(),
            current_closure_ptr: None,
            enums,
            is_async_fn: false,
            is_strict_fn: false,
            static_field_globals,
            class_ids,
            class_keys_globals: &cross_module.class_keys_globals,
            class_field_counts: &cross_module.class_field_counts,
            class_init_chains: &cross_module.class_init_chains,
            imported_class_ctors: &cross_module.imported_class_ctors,
            func_signatures,
            func_synthetic_arguments,
            func_returns_class: &cross_module.func_returns_class,
            boxed_vars: main_boxed_vars,
            prealloc_boxes: std::collections::HashSet::new(),
            compiler_private_async_i32_control_locals: &cross_module
                .compiler_private_async_i32_control_locals,
            compiler_private_async_i1_control_locals: &cross_module
                .compiler_private_async_i1_control_locals,
            closure_rest_params,
            local_closure_func_ids: HashMap::new(),
            local_closure_param_counts: HashMap::new(),
            option_object_locals: HashMap::new(),
            object_literal_locals: HashSet::new(),
            namespace_imports: &cross_module.namespace_imports,
            namespace_reexport_named_imports: &cross_module.namespace_reexport_named_imports,
            namespace_member_prefixes: &cross_module.namespace_member_prefixes,
            imported_async_funcs: &cross_module.imported_async_funcs,
            local_async_funcs: &cross_module.local_async_funcs,
            local_generator_funcs: &cross_module.local_generator_funcs,
            funcs_reading_dynamic_this: &cross_module.funcs_reading_dynamic_this,
            type_aliases: &cross_module.type_aliases,
            imported_func_param_counts: &cross_module.imported_func_param_counts,
            imported_func_has_rest: &cross_module.imported_func_has_rest,
            imported_func_synthetic_arguments: &cross_module.imported_func_synthetic_arguments,
            method_param_counts: &cross_module.method_param_counts,
            method_has_rest: &cross_module.method_has_rest,
            imported_func_return_types: &cross_module.imported_func_return_types,
            ffi_signatures: &cross_module.ffi_signatures,
            ffi_aliases: &cross_module.ffi_aliases,
            imported_class_sources: &cross_module.imported_class_sources,
            imported_class_original_names: &cross_module.imported_class_original_names,
            interfaces: &cross_module.interfaces,
            try_depth: 0,
            pending_declares: Vec::new(),
            integer_locals: main_native_facts.integer_locals(),
            unsigned_i32_locals: main_native_facts.unsigned_i32_locals(),
            shadow_slot_map: main_shadow_slot_map,
            shadow_slot_clears_after_stmt: main_shadow_slot_clears_after_stmt,
            arena_state_slot: None,
            class_keys_slots: HashMap::new(),
            cached_lengths: HashMap::new(),
            bounded_index_pairs: Vec::new(),
            packed_f64_loop_facts: Vec::new(),
            i32_counter_slots: HashMap::new(),
            i1_local_slots: HashMap::new(),
            index_used_locals: main_native_facts.index_used_locals(),
            strictly_i32_bounded_locals: main_native_facts.strictly_i32_bounded_locals(),
            i18n: &cross_module.i18n,
            dynamic_import_path_to_prefix: &cross_module.dynamic_import_path_to_prefix,
            local_class_aliases: HashMap::new(),
            local_class_field_aliases: HashMap::new(),
            local_id_to_name: HashMap::new(),
            local_value_aliases: HashMap::new(),
            imported_vars: &cross_module.imported_vars,
            compile_time_constants: main_native_facts.compile_time_constants(),
            target_triple: &cross_module.target_triple,
            app_metadata: &cross_module.app_metadata,
            scalar_replaced: std::collections::HashMap::new(),
            pod_records: std::collections::HashMap::new(),
            pod_views: std::collections::HashMap::new(),
            scalar_replaced_arrays: std::collections::HashMap::new(),
            scalar_ctor_target: Vec::new(),
            non_escaping_news: main_native_facts.non_escaping_news().clone(),
            non_escaping_new_used_fields: main_native_facts.non_escaping_new_used_fields().clone(),
            non_escaping_arrays: main_native_facts.non_escaping_arrays().clone(),
            non_escaping_array_used_indices: main_native_facts
                .non_escaping_array_used_indices()
                .clone(),
            non_escaping_object_literals: main_native_facts.non_escaping_object_literals().clone(),
            non_escaping_object_literal_used_fields: main_native_facts
                .non_escaping_object_literal_used_fields()
                .clone(),
            flat_const_arrays: &cross_module.flat_const_arrays,
            array_row_aliases: HashMap::new(),
            clamp3_functions: &cross_module.clamp3_functions,
            clamp_u8_functions: &cross_module.clamp_u8_functions,
            integer_returning_functions: &cross_module.returns_int_functions,
            i32_identity_functions: &cross_module.i32_identity_functions,
            typed_f64_functions: &cross_module.typed_f64_functions,
            typed_i32_functions: &cross_module.typed_i32_functions,
            typed_string_functions: &cross_module.typed_string_functions,
            typed_i1_functions: &cross_module.typed_i1_functions,
            typed_i1_function_param_reps: &cross_module.typed_i1_function_param_reps,
            typed_f64_methods: &cross_module.typed_f64_methods,
            typed_i32_methods: &cross_module.typed_i32_methods,
            typed_i1_methods: &cross_module.typed_i1_methods,
            typed_string_methods: &cross_module.typed_string_methods,
            typed_i1_method_param_reps: &cross_module.typed_i1_method_param_reps,
            typed_f64_closures: &cross_module.typed_f64_closures,
            typed_i32_closures: &cross_module.typed_i32_closures,
            typed_i1_closures: &cross_module.typed_i1_closures,
            typed_i1_closure_param_reps: &cross_module.typed_i1_closure_param_reps,
            typed_string_closures: &cross_module.typed_string_closures,
            typed_string_closure_capture_counts: &cross_module.typed_string_closure_capture_counts,
            was_unrolled: hir.init_was_unrolled,
            ic_site_counter: ic_base,
            ic_globals: Vec::new(),
            typed_parse_rodata: Vec::new(),
            typed_parse_counter: 0,
            buffer_data_slots: HashMap::new(),
            buffer_view_slots: HashMap::new(),
            native_arena_owner_aliases: HashMap::new(),
            native_arena_ambiguous_owner_aliases: HashSet::new(),
            disable_buffer_fast_path: cross_module.disable_buffer_fast_path,
            min_length_bounds: HashMap::new(),
            bounded_buffer_index_pairs: Vec::new(),
            guarded_buffer_index_pairs: Vec::new(),
            buffer_hazard_reasons: HashMap::new(),
            native_i32_aliases: HashMap::new(),
            int_range_aliases: HashMap::new(),
            int_range_facts: Vec::new(),
            next_loop_proof_scope_id: 0,
            nonnegative_integer_locals: HashSet::new(),
            native_rep_records: Vec::new(),
            known_noalias_buffer_locals: main_native_facts.known_noalias_buffer_locals(),
            buffer_alias_base,
        };
        // Register every module-level global's ADDRESS as a GC root so
        // the mark phase can discover pointer-typed values (Maps, Arrays,
        // user class instances) stored in them. Without this, a Map
        // held only in a module `const CACHE = new Map<...>()` would be
        // freed by the next GC cycle because the conservative stack
        // scan can't see the global's address — only `js_gc_register_global_root`
        // populates `GLOBAL_ROOTS`, which `mark_global_roots` scans.
        // Closes issue #36 (pg driver's CONN_STATES Map crash after bulk
        // decode crossed the malloc-count GC threshold). Safe to register
        // number-valued globals too — `try_mark_value` + the raw-pointer
        // fallback both validate against the known-heap-pointer set and
        // discard non-matching bits.
        register_module_globals_as_gc_roots(&mut ctx, module_globals);
        // ESM entry (import/export syntax or top-level await — Node's module
        // detection): mark the pending module-evaluation checkpoint so the
        // first microtask drain finishes promise/queueMicrotask jobs before
        // the nextTick queue, matching Node's job-within-checkpoint ordering
        // for ESM evaluation (#788). CJS-style entries keep ticks-first.
        if !hir.imports.is_empty() || !hir.exports.is_empty() || hir.has_top_level_await {
            ctx.block().call_void("js_mark_entry_module_esm", &[]);
        }
        // Initialize static class fields with their declared init
        // expressions. Runs once at the top of main, before user code.
        //
        // Split into two phases (#894): early emits the bits that don't
        // read user-let values (Error-extending class registry, well-
        // known symbol method hooks); late runs AFTER user init so
        // computed-Symbol-key static fields whose key/init reference
        // module-level lets see populated slots.
        init_static_fields_early(&mut ctx, hir)?;
        // #5579: GlobalDeclarationInstantiation for a Script. A non-ESM entry
        // program runs as a *Script*, so its bare top-level `function`
        // declarations become own properties of the global object (observable
        // via `Object.prototype.hasOwnProperty.call(globalThis, name)` — the
        // check the Test262 async harness uses for `$DONE`). ESM modules
        // (import/export syntax or top-level await) instead bind in the
        // module record and do NOT reflect.
        //
        // Gated additionally on the program actually referencing `globalThis`:
        // if it never reads the global object the reflection is unobservable,
        // so skipping it avoids adding dynamic-property-helper calls (and their
        // startup cost) to every pure program's module init. Emitted before
        // user init so the functions are visible to top-level code (hoisting).
        let is_esm_entry =
            !hir.imports.is_empty() || !hir.exports.is_empty() || hir.has_top_level_await;
        if !is_esm_entry && hir.references_global_this {
            emit_script_global_function_decls(&mut ctx, hir);
        }
        stmt::lower_top_level_stmts(&mut ctx, &hir.init)
            .with_context(|| format!("lowering init statements of module '{}'", hir.name))?;
        init_static_fields_late(&mut ctx, hir)?;

        // Issue #100: populate `@__perry_ns_<module_prefix>` from the
        // namespace_entries list AFTER user init has run (so every
        // local export's binding is set) and BEFORE the event-loop
        // bootstrap (so the namespace is observable to any consumer
        // who dispatches `await import("./this_module.ts")` during
        // event-loop turns). For the entry-module case this is the
        // unusual scenario where some other module dynamic-imports
        // the entry itself — uncommon but supported.
        // Issue #842: also run the populator for side-effect-only
        // dynamic-import targets (`namespace_entries` empty but module
        // is a target). The populator emits `js_create_namespace(0, ...)`
        // → an empty NaN-boxed object → stored into `@__perry_ns_<prefix>`,
        // satisfying the consumer-side extern reference.
        if (!cross_module.namespace_entries.is_empty() || cross_module.is_dynamic_import_target)
            && !ctx.block().is_terminated()
        {
            emit_namespace_populator(
                &mut ctx,
                &cross_module.namespace_entries,
                namespace_key_globals,
                module_prefix,
            );
        }

        if !ctx.block().is_terminated() {
            if is_dylib {
                // Dylib: no event loop — the host manages its own event
                // loop and calls perry_fn_* entry points as needed. Just
                // return after running top-level statements (which set up
                // module-level state like Maps, class registrations, etc.).
                ctx.block().ret_void();
            } else {
                // Event loop: keep running while there are active event
                // sources (timers, intervals, WS servers, pending stdlib
                // async ops). Without this, event-driven servers (WS,
                // setInterval-based) exit immediately after init.
                //
                // Structure:
                //   loop_header: check if any source is active → body or exit
                //   loop_body:   tick all queues, sleep 10ms, jump to header
                //   loop_exit:   ret 0
                let header_idx = ctx.new_block("event_loop.header");
                let body_idx = ctx.new_block("event_loop.body");
                let exit_idx = ctx.new_block("event_loop.exit");
                let header_label = ctx.block_label(header_idx);
                let body_label = ctx.block_label(body_idx);
                let exit_label = ctx.block_label(exit_idx);

                // Initial microtask flush (4 rounds) before entering the
                // event loop — handles fire-and-forget .then() chains that
                // don't need the full event loop.
                for _ in 0..4 {
                    let _ = ctx.block().call(I32, "js_promise_run_microtasks", &[]);
                    let _ = ctx.block().call(I32, "js_timer_tick_if_refed", &[]);
                    let _ = ctx.block().call(I32, "js_callback_timer_tick", &[]);
                    let _ = ctx.block().call(I32, "js_interval_timer_tick", &[]);
                }
                ctx.block().call_void("js_run_stdlib_pump", &[]);
                ctx.block().br(&header_label);

                // loop_header: check if there's any reason to keep running
                ctx.current_block = header_idx;
                let has_timers = ctx.block().call(I32, "js_timer_has_pending", &[]);
                let has_callbacks = ctx.block().call(I32, "js_callback_timer_has_pending", &[]);
                let has_intervals = ctx.block().call(I32, "js_interval_timer_has_pending", &[]);
                let has_stdlib = ctx.block().call(I32, "js_stdlib_has_active_handles", &[]);
                // #591: TASK_QUEUE may carry a pending `.then` continuation
                // that was queued by `js_run_stdlib_pump`'s resolution path
                // in the SAME body iteration that already drained the inflight
                // counter and PENDING_RESOLUTIONS to zero. Without this gate,
                // the header check would flip to "exit" before the next body's
                // microtask drain ran the continuation.
                let has_microtasks = ctx.block().call(I32, "js_microtasks_pending", &[]);
                let any1 = ctx.block().or(I32, &has_timers, &has_callbacks);
                let any2 = ctx.block().or(I32, &has_intervals, &has_stdlib);
                let any3 = ctx.block().or(I32, &any1, &any2);
                let any = ctx.block().or(I32, &any3, &has_microtasks);
                let zero = "0".to_string();
                let cmp = ctx.block().icmp_ne(I32, &any, &zero);
                ctx.block().cond_br(&cmp, &body_label, &exit_label);

                // loop_body: tick everything, sleep, loop
                ctx.current_block = body_idx;
                let _ = ctx.block().call(I32, "js_promise_run_microtasks", &[]);
                let _ = ctx.block().call(I32, "js_timer_tick", &[]);
                let _ = ctx.block().call(I32, "js_callback_timer_tick", &[]);
                let _ = ctx.block().call(I32, "js_interval_timer_tick", &[]);
                ctx.block().call_void("js_run_stdlib_pump", &[]);
                // Issue #84: condvar-backed wait. Returns immediately when
                // a tokio worker (net/ws/http/fetch/redis/spawn) notifies
                // after pushing to its queue; otherwise blocks until the
                // next timer/interval deadline or a 1 s safety cap.
                ctx.block().call_void("js_wait_for_event", &[]);
                ctx.block().br(&header_label);

                // loop_exit: fire `beforeExit` (#2135) with the would-be
                // exit code, then drain microtasks/timers once more so any
                // last-minute work the listener queued still runs before
                // we ret. Mirrors Node's "event loop drained → one
                // beforeExit pass" semantics.
                //
                // We pass `0` as the code today: Perry doesn't yet wire
                // `process.exitCode` into this codegen path, and the test
                // surface in #2135 only pins the firing + the default
                // code. Explicit `process.exit(N)` bypasses this whole
                // block via libc::_exit.
                ctx.current_block = exit_idx;
                let zero_code = "0x0".to_string();
                ctx.block()
                    .call_void("js_process_emit_before_exit", &[(DOUBLE, &zero_code)]);
                let _ = ctx.block().call(I32, "js_promise_run_microtasks", &[]);
                let _ = ctx.block().call(I32, "js_timer_tick_if_refed", &[]);
                let _ = ctx.block().call(I32, "js_callback_timer_tick", &[]);
                let _ = ctx.block().call(I32, "js_interval_timer_tick", &[]);
                ctx.block()
                    .call_void("js_process_run_finalization_exit", &[]);
                // After the event loop drains, surface any still-unhandled
                // promise rejection (Node exits non-zero; this matches the
                // oracle for `Promise.reject`/combinator-reject programs).
                ctx.block()
                    .call_void("js_promise_report_unhandled_rejections", &[]);
                ctx.block().ret(I32, "0");
            }
        }
        let ic_globals = std::mem::take(&mut ctx.ic_globals);
        let typed_parse_rodata = std::mem::take(&mut ctx.typed_parse_rodata);
        let ic_end = ctx.ic_site_counter;
        let pending = std::mem::take(&mut ctx.pending_declares);
        let buffer_alias_used = ctx.buffer_data_slots.len() as u32;
        let native_rep_records = std::mem::take(&mut ctx.native_rep_records);
        drop(ctx);
        llmod.ic_counter = ic_end;
        llmod.buffer_alias_counter += buffer_alias_used;
        llmod.native_rep_records.extend(native_rep_records);
        for (name, ret, params) in pending {
            llmod.declare_function(&name, ret, &params);
        }
        for ic_name in &ic_globals {
            llmod.add_raw_global(format!(
                "@{} = private global [2 x i64] zeroinitializer",
                ic_name
            ));
        }
        for raw in &typed_parse_rodata {
            llmod.add_raw_global(raw.clone());
        }
        // Plugin ABI shim — emitted once, from the dylib/staticlib's entry
        // module (this is where the top-level `activate`/`deactivate` exports
        // live). See `emit_plugin_abi_shim` and issue #5273.
        if is_dylib {
            emit_plugin_abi_shim(llmod, hir, module_prefix);
        }
    } else {
        // Issue #753: idempotent init guard. Every non-entry module gets
        // a one-byte `@__perry_init_done_<prefix>` flag and a thin
        // wrapper `<prefix>__init` that returns immediately when the
        // flag is set or stores 1 + dispatches to `<prefix>__init_body`
        // when it isn't. The wrapper is what the entry main calls
        // eagerly (for Eager modules) and what every
        // `Expr::DynamicImport` dispatch site calls (for any module
        // that's a dynamic-import target — possibly multiple sites in
        // the same program). The 2-state guard matches ESM's
        // partial-cycle semantics: re-entry during init returns without
        // re-running the body, leaving the namespace populator's work
        // partially observable. The wrapper sets `done = 1` BEFORE
        // calling the body so the re-entry path returns immediately.
        let done_global = format!("__perry_init_done_{}", module_prefix);
        llmod.add_internal_global(&done_global, I8, "0");
        let init_name = format!("{}__init", module_prefix);
        let init_body_name = format!("{}__init_body", module_prefix);
        {
            let wrap_fn = llmod.define_function(&init_name, VOID, vec![]);
            let _ = wrap_fn.create_block("entry");
            let _ = wrap_fn.create_block("guard.ret");
            let _ = wrap_fn.create_block("guard.do");
            let ret_label = wrap_fn.block_mut(1).unwrap().label.clone();
            let do_label = wrap_fn.block_mut(2).unwrap().label.clone();
            {
                let blk = wrap_fn.block_mut(0).unwrap();
                let done = blk.load(I8, &format!("@{}", done_global));
                let already = blk.icmp_ne(I8, &done, "0");
                blk.cond_br(&already, &ret_label, &do_label);
            }
            {
                let blk = wrap_fn.block_mut(1).unwrap();
                blk.ret_void();
            }
            {
                let blk = wrap_fn.block_mut(2).unwrap();
                blk.store(I8, "1", &format!("@{}", done_global));
                // Trigger init of static-dep + re-export source modules
                // before the body runs. Each `<dep>__init` is itself
                // wrapped by the same guard pattern, so this short-
                // circuits when the dep was already initialized
                // (Eager-via-main path) and fires the body when the
                // dep is Deferred and this is the first reach. The
                // entry module has no `__init` so the driver excludes
                // it from `module_init_deps`.
                for dep_prefix in &cross_module.module_init_deps {
                    if dep_prefix == module_prefix {
                        continue;
                    }
                    blk.call_void(&format!("{}__init", dep_prefix), &[]);
                }
                blk.call_void(&init_body_name, &[]);
                blk.ret_void();
            }
        }
        // Declare every dep's `__init` symbol so the wrapper's calls
        // resolve at link time. Most overlap with `non_entry_module_prefixes`
        // (whose declarations live in the entry module's compilation),
        // but a non-entry module compiled standalone has no entry-side
        // declaration list — emit them here too. `declare_function`
        // dedupes by name.
        for dep_prefix in &cross_module.module_init_deps {
            if dep_prefix == module_prefix {
                continue;
            }
            llmod.declare_function(&format!("{}__init", dep_prefix), VOID, &[]);
        }
        // The body retains every existing semantic of `<prefix>__init`
        // (strings init, globals/GC registration, top-level statements,
        // namespace populator at the tail). It's `internal` linkage:
        // only the wrapper above ever calls it, both within this module
        // and across modules via the wrapper's external symbol.
        let init_name = init_body_name;
        // Debug: emit puts("INIT: <prefix>") at the top of each module init
        let debug_init_const = if std::env::var("PERRY_DEBUG_INIT").is_ok() {
            let debug_msg = format!("INIT: {}\0", module_prefix);
            let (const_name, _) = llmod.add_string_constant(&debug_msg);
            llmod.declare_function("puts", I32, &[PTR]);
            Some(const_name)
        } else {
            None
        };
        let ic_base = llmod.ic_counter;
        let buffer_alias_base = llmod.buffer_alias_counter;
        let init_fn = llmod.define_function(&init_name, VOID, vec![]);
        // `__init_body` is normally only reached through the guarded `__init`
        // wrapper, so it would be `internal`. But a `worker_threads` Worker
        // target must re-run its module body ONCE PER worker thread (each
        // worker has its own thread-local arena), which the process-global
        // `__perry_init_done_*` guard on `__init` would suppress after the
        // first worker. Exposing the body with external linkage lets the
        // worker-spawn codegen call it directly, bypassing the once-guard, so
        // every spawned worker actually executes its entry. Main-thread import
        // init is unaffected — it still goes through the guarded wrapper.
        init_fn.linkage = "external".to_string();
        if is_dylib {
            init_fn.add_pre_return_void_call("js_typed_feedback_maybe_dump_trace");
        }
        let _ = init_fn.create_block("entry");
        {
            let blk = init_fn.block_mut(0).unwrap();
            if let Some(ref cname) = debug_init_const {
                blk.call_void("puts", &[(PTR, &format!("@{}", cname))]);
            }
            if write_barriers_enabled() {
                blk.call_void("js_gc_write_barriers_emitted", &[(I32, "1")]);
            }
            // Each non-entry module runs its own string pool init at
            // the start of its module init function. The entry main
            // calls each module init in order (after running its own
            // strings init), so by the time user code in any module
            // executes, every module's strings are alive.
            blk.call_void(&strings_init_name, &[]);
        }
        // Same boundary as the entry-module main: hoisted post-init
        // setup must run AFTER the strings init populates module
        // globals like `@perry_class_keys_*`.
        init_fn.mark_entry_init_boundary();
        let flat_const_ids: std::collections::HashSet<u32> =
            cross_module.flat_const_arrays.keys().copied().collect();
        let (init_shadow_slot_map, init_shadow_slot_clears_after_stmt) =
            enable_module_init_shadow_frame(init_fn, &hir.init, &flat_const_ids);

        let init_boxed_vars = module_boxed_vars.clone();
        let clamp_fn_ids: std::collections::HashSet<u32> = cross_module
            .clamp3_functions
            .union(&cross_module.clamp_u8_functions)
            .chain(cross_module.returns_int_functions.iter())
            .copied()
            .collect();
        let init_native_facts = crate::collectors::collect_native_region_fact_graph(
            &hir.init,
            &flat_const_ids,
            &clamp_fn_ids,
            &cross_module.clamp3_functions,
            &init_boxed_vars,
            module_globals,
            classes,
            &cross_module.compile_time_constants,
        );
        let mut ctx = FnCtx {
            func: init_fn,
            module_slug: crate::expr::native_region_slug(strings.module_prefix()),
            source_function: "module_init".to_string(),
            source_function_slug: crate::expr::native_region_slug("module_init"),
            active_region_id: None,
            native_facts: &init_native_facts,
            locals: HashMap::new(),
            local_types: HashMap::new(),
            current_block: 0,
            discard_expr_value: false,
            func_names,
            strings,
            loop_targets: Vec::new(),
            label_targets: HashMap::new(),
            pending_labels: Vec::new(),
            classes,
            this_stack: Vec::new(),
            inline_ctor_return: Vec::new(),
            new_target_stack: Vec::new(),
            class_stack: Vec::new(),
            methods,
            module_globals,
            import_function_prefixes,
            import_function_origin_names: &cross_module.import_function_origin_names,
            import_function_v8_specifiers: &cross_module.import_function_v8_specifiers,
            // Issue #841: node:submodule named-import + namespace registries.
            import_function_node_submodule: &cross_module.import_function_node_submodule,
            namespace_node_submodules: &cross_module.namespace_node_submodules,
            namespace_v8_specifiers: &cross_module.namespace_v8_specifiers,
            closure_captures: HashMap::new(),
            current_closure_ptr: None,
            enums,
            is_async_fn: false,
            is_strict_fn: false,
            static_field_globals,
            class_ids,
            class_keys_globals: &cross_module.class_keys_globals,
            class_field_counts: &cross_module.class_field_counts,
            class_init_chains: &cross_module.class_init_chains,
            imported_class_ctors: &cross_module.imported_class_ctors,
            func_signatures,
            func_synthetic_arguments,
            func_returns_class: &cross_module.func_returns_class,
            boxed_vars: init_boxed_vars,
            prealloc_boxes: std::collections::HashSet::new(),
            compiler_private_async_i32_control_locals: &cross_module
                .compiler_private_async_i32_control_locals,
            compiler_private_async_i1_control_locals: &cross_module
                .compiler_private_async_i1_control_locals,
            closure_rest_params,
            local_closure_func_ids: HashMap::new(),
            local_closure_param_counts: HashMap::new(),
            option_object_locals: HashMap::new(),
            object_literal_locals: HashSet::new(),
            namespace_imports: &cross_module.namespace_imports,
            namespace_reexport_named_imports: &cross_module.namespace_reexport_named_imports,
            namespace_member_prefixes: &cross_module.namespace_member_prefixes,
            imported_async_funcs: &cross_module.imported_async_funcs,
            local_async_funcs: &cross_module.local_async_funcs,
            local_generator_funcs: &cross_module.local_generator_funcs,
            funcs_reading_dynamic_this: &cross_module.funcs_reading_dynamic_this,
            type_aliases: &cross_module.type_aliases,
            imported_func_param_counts: &cross_module.imported_func_param_counts,
            imported_func_has_rest: &cross_module.imported_func_has_rest,
            imported_func_synthetic_arguments: &cross_module.imported_func_synthetic_arguments,
            method_param_counts: &cross_module.method_param_counts,
            method_has_rest: &cross_module.method_has_rest,
            imported_func_return_types: &cross_module.imported_func_return_types,
            ffi_signatures: &cross_module.ffi_signatures,
            ffi_aliases: &cross_module.ffi_aliases,
            imported_class_sources: &cross_module.imported_class_sources,
            imported_class_original_names: &cross_module.imported_class_original_names,
            interfaces: &cross_module.interfaces,
            try_depth: 0,
            pending_declares: Vec::new(),
            integer_locals: init_native_facts.integer_locals(),
            unsigned_i32_locals: init_native_facts.unsigned_i32_locals(),
            shadow_slot_map: init_shadow_slot_map,
            shadow_slot_clears_after_stmt: init_shadow_slot_clears_after_stmt,
            arena_state_slot: None,
            class_keys_slots: HashMap::new(),
            cached_lengths: HashMap::new(),
            bounded_index_pairs: Vec::new(),
            packed_f64_loop_facts: Vec::new(),
            i32_counter_slots: HashMap::new(),
            i1_local_slots: HashMap::new(),
            index_used_locals: init_native_facts.index_used_locals(),
            strictly_i32_bounded_locals: init_native_facts.strictly_i32_bounded_locals(),
            i18n: &cross_module.i18n,
            dynamic_import_path_to_prefix: &cross_module.dynamic_import_path_to_prefix,
            local_class_aliases: HashMap::new(),
            local_class_field_aliases: HashMap::new(),
            local_id_to_name: HashMap::new(),
            local_value_aliases: HashMap::new(),
            imported_vars: &cross_module.imported_vars,
            compile_time_constants: init_native_facts.compile_time_constants(),
            target_triple: &cross_module.target_triple,
            app_metadata: &cross_module.app_metadata,
            scalar_replaced: std::collections::HashMap::new(),
            pod_records: std::collections::HashMap::new(),
            pod_views: std::collections::HashMap::new(),
            scalar_replaced_arrays: std::collections::HashMap::new(),
            scalar_ctor_target: Vec::new(),
            non_escaping_news: init_native_facts.non_escaping_news().clone(),
            non_escaping_new_used_fields: init_native_facts.non_escaping_new_used_fields().clone(),
            non_escaping_arrays: init_native_facts.non_escaping_arrays().clone(),
            non_escaping_array_used_indices: init_native_facts
                .non_escaping_array_used_indices()
                .clone(),
            non_escaping_object_literals: init_native_facts.non_escaping_object_literals().clone(),
            non_escaping_object_literal_used_fields: init_native_facts
                .non_escaping_object_literal_used_fields()
                .clone(),
            flat_const_arrays: &cross_module.flat_const_arrays,
            array_row_aliases: HashMap::new(),
            clamp3_functions: &cross_module.clamp3_functions,
            clamp_u8_functions: &cross_module.clamp_u8_functions,
            integer_returning_functions: &cross_module.returns_int_functions,
            i32_identity_functions: &cross_module.i32_identity_functions,
            typed_f64_functions: &cross_module.typed_f64_functions,
            typed_i32_functions: &cross_module.typed_i32_functions,
            typed_string_functions: &cross_module.typed_string_functions,
            typed_i1_functions: &cross_module.typed_i1_functions,
            typed_i1_function_param_reps: &cross_module.typed_i1_function_param_reps,
            typed_f64_methods: &cross_module.typed_f64_methods,
            typed_i32_methods: &cross_module.typed_i32_methods,
            typed_i1_methods: &cross_module.typed_i1_methods,
            typed_string_methods: &cross_module.typed_string_methods,
            typed_i1_method_param_reps: &cross_module.typed_i1_method_param_reps,
            typed_f64_closures: &cross_module.typed_f64_closures,
            typed_i32_closures: &cross_module.typed_i32_closures,
            typed_i1_closures: &cross_module.typed_i1_closures,
            typed_i1_closure_param_reps: &cross_module.typed_i1_closure_param_reps,
            typed_string_closures: &cross_module.typed_string_closures,
            typed_string_closure_capture_counts: &cross_module.typed_string_closure_capture_counts,
            was_unrolled: hir.init_was_unrolled,
            ic_site_counter: ic_base,
            ic_globals: Vec::new(),
            typed_parse_rodata: Vec::new(),
            typed_parse_counter: 0,
            buffer_data_slots: HashMap::new(),
            buffer_view_slots: HashMap::new(),
            native_arena_owner_aliases: HashMap::new(),
            native_arena_ambiguous_owner_aliases: HashSet::new(),
            disable_buffer_fast_path: cross_module.disable_buffer_fast_path,
            min_length_bounds: HashMap::new(),
            bounded_buffer_index_pairs: Vec::new(),
            guarded_buffer_index_pairs: Vec::new(),
            buffer_hazard_reasons: HashMap::new(),
            native_i32_aliases: HashMap::new(),
            int_range_aliases: HashMap::new(),
            int_range_facts: Vec::new(),
            next_loop_proof_scope_id: 0,
            nonnegative_integer_locals: HashSet::new(),
            native_rep_records: Vec::new(),
            known_noalias_buffer_locals: init_native_facts.known_noalias_buffer_locals(),
            buffer_alias_base,
        };
        // Register every module-level global's ADDRESS as a GC root —
        // same reason as the entry-module branch above (issue #36). For
        // non-entry modules the registration runs inside their __init
        // function, which the entry main calls in topological order
        // right after js_gc_init, so by the time any user code executes
        // every module's globals are already GC-rooted.
        register_module_globals_as_gc_roots(&mut ctx, module_globals);
        // Issue #894: split into early/late around top-level lowering so a
        // computed-Symbol-key static field whose key/init reference
        // top-level module lets (e.g. effect's `make()` factory:
        // `static [TypeId] = variance`) sees populated globals.
        init_static_fields_early(&mut ctx, hir)?;
        stmt::lower_top_level_stmts(&mut ctx, &hir.init).with_context(|| {
            format!(
                "lowering init statements of non-entry module '{}'",
                hir.name
            )
        })?;
        init_static_fields_late(&mut ctx, hir)?;

        // Issue #100: populate `@__perry_ns_<module_prefix>` from the
        // namespace_entries list at the tail of the non-entry __init.
        // The entry main has already called this module's __init AFTER
        // every static-import dependency's __init (topo sort) — so
        // re-export sources have populated their getters. Local
        // exports' bindings are also set because top-level lowering ran
        // above. The dispatcher in `Expr::DynamicImport` loads
        // `@__perry_ns_<prefix>` and wraps it in `js_promise_resolved`.
        // Issue #842: also run the populator for side-effect-only
        // dynamic-import targets (`namespace_entries` empty but module
        // is a target). The populator emits `js_create_namespace(0, ...)`
        // → an empty NaN-boxed object → stored into `@__perry_ns_<prefix>`,
        // satisfying the consumer-side extern reference.
        if (!cross_module.namespace_entries.is_empty() || cross_module.is_dynamic_import_target)
            && !ctx.block().is_terminated()
        {
            emit_namespace_populator(
                &mut ctx,
                &cross_module.namespace_entries,
                namespace_key_globals,
                module_prefix,
            );
        }

        if !ctx.block().is_terminated() {
            ctx.block().ret_void();
        }
        let ic_globals = std::mem::take(&mut ctx.ic_globals);
        let typed_parse_rodata = std::mem::take(&mut ctx.typed_parse_rodata);
        let ic_end = ctx.ic_site_counter;
        let pending = std::mem::take(&mut ctx.pending_declares);
        let buffer_alias_used = ctx.buffer_data_slots.len() as u32;
        let native_rep_records = std::mem::take(&mut ctx.native_rep_records);
        drop(ctx);
        llmod.ic_counter = ic_end;
        llmod.buffer_alias_counter += buffer_alias_used;
        llmod.native_rep_records.extend(native_rep_records);
        for (name, ret, params) in pending {
            llmod.declare_function(&name, ret, &params);
        }
        // NB: the plugin ABI shim (`perry_plugin_abi_version` /
        // `plugin_activate` / `plugin_deactivate`) is emitted from the entry
        // branch above, NOT here — see `emit_plugin_abi_shim` and issue #5273.
        // A dylib's top-level plugin exports live in its entry module, and the
        // three symbols must be defined exactly once per shared library.
        for ic_name in &ic_globals {
            llmod.add_raw_global(format!(
                "@{} = private global [2 x i64] zeroinitializer",
                ic_name
            ));
        }
        for raw in &typed_parse_rodata {
            llmod.add_raw_global(raw.clone());
        }
    }
    Ok(())
}
