//! Entry-module helpers split out of `codegen/entry.rs` for the 2000-line cap:
//! the plugin ABI shim and the `process.env` literal scan. Both are called
//! from `compile_module_entry` and touch nothing else in that file.

use perry_hir::Module as HirModule;

use crate::module::LlModule;
use crate::types::{DOUBLE, I64, VOID};

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
pub(super) fn emit_plugin_abi_shim(llmod: &mut LlModule, hir: &HirModule, module_prefix: &str) {
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
pub(super) fn collect_entry_env_literals(hir: &HirModule) -> Vec<(String, String)> {
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
    for stmt in crate::codegen::entry_outline::logical_entry_stmts(hir) {
        scan(std::slice::from_ref(stmt), &mut out, 0);
    }
    out
}
