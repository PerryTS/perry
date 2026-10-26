//! Emits the module's exported function-value surface: live getters for
//! re-exported node builtins, the `__perry_wrap_*` closure-ABI wrappers for
//! every top-level user function, and the cross-module name aliases that
//! use the same canonical symbols as the consumer-side
//! verbatim one. Kept separate from the artifact traversal so this wrapper
//! family stays grouped without pushing that orchestration module over the
//! source-size gate.

use std::collections::{HashMap, HashSet};

use perry_hir::Module as HirModule;

use crate::module::LlModule;
use crate::strings::StringPool;
use crate::types::{LlvmType, DOUBLE, I64};

use super::helpers::{function_body_returns_generator_object, scoped_fn_name};

/// Borrowed inputs for the export/function-value wrapper emission phase.
pub(super) struct ExportValueWrapperCtx<'a> {
    pub llmod: &'a mut LlModule,
    pub strings: &'a mut StringPool,
    pub hir: &'a HirModule,
    pub func_names: &'a HashMap<u32, String>,
    pub module_prefix: &'a String,
}

/// Emit the exported-value getters, `__perry_wrap_*` wrappers, and the
/// raw/renamed cross-module aliases for this module.
pub(super) fn emit_export_value_wrappers(c: ExportValueWrapperCtx<'_>) {
    let ExportValueWrapperCtx {
        llmod,
        strings,
        hir,
        func_names,
        module_prefix,
    } = c;

    // Native named imports are runtime callable/property values rather
    // than functions compiled into this module. When one is exported (either
    // `import { x } from "node:m"; export { x }` or the synthetic Import +
    // Named pair used for a direct native re-export), publish a zero-arg
    // getter that reads the native ESM export cell. Importers classify
    // this as a variable export and invoke the returned callable value, rather
    // than linking a nonexistent `perry_fn_<module>__x` body.
    for export in &hir.exports {
        let perry_hir::Export::Named { local, exported } = export else {
            continue;
        };
        let native_origin = hir.imports.iter().find_map(|import| {
            if !import.has_native_value_bindings() {
                return None;
            }
            import
                .specifiers
                .iter()
                .find_map(|specifier| match specifier {
                    perry_hir::ImportSpecifier::Named {
                        imported,
                        local: import_local,
                    } if import_local == local => Some((import.source.as_str(), imported.as_str())),
                    _ => None,
                })
        });
        let Some((source, imported)) = native_origin else {
            continue;
        };
        let getter_name = crate::symbol_names::function(&module_prefix, &exported);
        if llmod.has_function(&getter_name) {
            continue;
        }
        let source_idx = strings.intern(source);
        let imported_idx = strings.intern(imported);
        let source_handle = format!("@{}", strings.entry(source_idx).handle_global);
        let imported_handle = format!("@{}", strings.entry(imported_idx).handle_global);
        let getter = llmod.define_function(&getter_name, DOUBLE, vec![]);
        let _ = getter.create_block("entry");
        let blk = getter.block_mut(0).unwrap();
        // An object-valued native named export can be a builtin submodule
        // (`fs.promises`, `util.types`, ...). Its namespace getter must arm
        // that precise submodule before reading the ESM export cell, just as
        // a direct import of the same subpath does.
        let subpath = format!("{source}/{imported}");
        if perry_hir::is_node_builtin_module(&subpath) {
            let install = crate::nm_install::native_namespace_submodule_key(&subpath)
                .and_then(crate::nm_install::nm_submod_install_symbol)
                .or_else(|| crate::nm_install::nm_install_symbol(&subpath));
            if let Some(install) = install {
                blk.call_void(install, &[]);
            }
        }
        if let Some(install) = crate::nm_install::nm_install_symbol(source) {
            blk.call_void(install, &[]);
        }
        let source_value = blk.load(DOUBLE, &source_handle);
        let imported_value = blk.load(DOUBLE, &imported_handle);
        let value = blk.call(
            DOUBLE,
            "js_native_module_named_esm_export_value",
            &[(DOUBLE, &source_value), (DOUBLE, &imported_value)],
        );
        blk.ret(DOUBLE, &value);
    }

    // Emit FuncRef-as-value wrappers. For each user function, generate
    // a thin wrapper `__perry_wrap_<name>` whose signature is the JS body
    // ABI: `double(i64 this_closure, i64 js_this, double arg0, double arg1,
    // ...)`. Most wrappers discard the closure pointer and forward the args
    // to the underlying function; generator wrappers reuse it to link the
    // returned iterator to the closure-cached `prototype`. The underlying
    // function still reads its receiver from the implicit-`this` cell.
    //
    // The wrapper exists so that `apply(add, 3, 4)` can pass `add` as
    // a value and have `apply` call it via `js_closure_call2`. Without
    // a wrapper, the closure call would invoke `add(closure, 3, 4)`
    // (wrong calling convention) instead of `add(3, 4)`.
    //
    // Wrappers are emitted unconditionally for every user function;
    // dead-code elimination at link time will remove unused ones.
    for f in &hir.functions {
        let original_name = func_names.get(&f.id).cloned().unwrap();
        // Wrapper signature: i64 closure_ptr + one double per declared param —
        // the full ABI arity `user_fn_wrapper_arity` registers for dynamic
        // dispatch. Any cap here silently drops arguments: at 5, test262's
        // `TemporalHelpers.assertDuration(d, y, mo, w, d, h, …)` (11 args) read
        // `hours` onward as 0; at 16 (#10420), every `apply`/`call`/spread or
        // object-literal-method call of an 18-param function passed `0` for
        // params 17 and 18, because the runtime dispatched the registered
        // 18-slot signature into a 16-param wrapper. Calls wider than the
        // `js_closure_call0..16` family reach the wrapper through
        // `js_closure_call_array`.
        let arity = f.params.len();
        let arg_names: Vec<String> = (0..arity).map(|i| format!("%a{}", i)).collect();
        let wrap_params = crate::expr::body_call::js_body_params(arg_names.iter().cloned());
        let wrap_name = format!("__perry_wrap_{}", original_name);
        let wf = llmod.define_function(&wrap_name, DOUBLE, wrap_params);
        let _ = wf.create_block("entry");
        let blk = wf.block_mut(0).unwrap();
        // Call the underlying function with the arg doubles — a this-reading
        // function's receiver-taking body with the receiver first.
        let mut result = call_function_body(blk, f, &original_name, &arg_names);
        if function_body_returns_generator_object(&f.body) {
            result = blk.call(
                DOUBLE,
                "js_generator_attach_closure_prototype",
                &[(DOUBLE, &result), (I64, "%this_closure")],
            );
        }
        blk.ret(DOUBLE, &result);
    }

    // Renamed exports need closure-ABI wrappers under their public spelling,
    // forwarding to the registry-owned local body. Both use canonical names.
    {
        let func_by_id: HashMap<u32, &perry_hir::Function> =
            hir.functions.iter().map(|f| (f.id, f)).collect();
        let mut emitted_aliases: HashSet<String> = HashSet::new();
        for (exported_name, func_id) in &hir.exported_functions {
            let Some(f) = func_by_id.get(func_id) else {
                continue;
            };
            let Some(target_name) = func_names.get(func_id) else {
                continue;
            };
            let target_wrap = format!("__perry_wrap_{}", target_name);
            let alias_wrap = crate::symbol_names::function_wrapper(&module_prefix, &exported_name);
            if alias_wrap == target_wrap
                || llmod.has_function(&alias_wrap)
                || !emitted_aliases.insert(alias_wrap.clone())
            {
                continue;
            }

            let arity = f.params.len();
            let params =
                crate::expr::body_call::js_body_params((0..arity).map(|i| format!("%a{}", i)));
            let alias = llmod.define_function(&alias_wrap, DOUBLE, params.clone());
            let _ = alias.create_block("entry");
            let blk = alias.block_mut(0).unwrap();
            // Forward every parameter, the receiver included.
            let arg_refs: Vec<(LlvmType, &str)> = params
                .iter()
                .map(|(ty, value)| (*ty, value.as_str()))
                .collect();
            let result = blk.call(DOUBLE, &target_wrap, &arg_refs);
            blk.ret(DOUBLE, &result);
        }
    }

    // Closes #837 / refs #836: emit `__perry_wrap_perry_fn_<src>__<exported>`
    // closure wrappers for every `Export::Named { local, exported }` rename
    // where `exported != local`. The regular wrapper loop above keys
    // wrappers on the local HIR name (`__perry_wrap_perry_fn_<src>__<local>`),
    // and the `perry_fn_<src>__<exported>` alias loop further down emits
    // the direct-call alias. The MISSING piece was the closure-wrapper
    // alias: consumer-side `expr.rs:~3819` builds
    // `__perry_wrap_perry_fn_<src>__<exported>` whenever an imported name
    // is referenced as a function VALUE (passed as a callback, stored in
    // a variable, etc.) — for renamed exports that symbol had no definition
    // and the link failed.
    //
    // Concrete failures pre-fix:
    //   * uuid's `v35.js`: `export default function v35(...)` — the local
    //     is `v35`, the exported name is `default`. Consumers
    //     (`v3.js`, `v5.js`) pass `v35` as a closure value to their own
    //     default wrappers, which transitively pulls
    //     `__perry_wrap_perry_fn_<v35.js>__default` into the link.
    //   * zod's `regexes.ts`: `const _null = ...; export { _null as null }`
    //     (same for `undefined`). A consumer that reads `regexes.null`
    //     /`regexes.undefined` as a value falls through to the
    //     function-shape branch (no `imported_vars` entry under the
    //     renamed name) and references the wrapper symbol.
    //
    // Function-renames forward to the local function body (same shape as
    // the regular wrapper above). Variable-renames have no callable body —
    // emit a no-op that returns undefined, matching the
    // `__perry_wrap_perry_unknown_func` fallback pattern below.
    {
        use std::collections::HashSet;
        let mut emitted_export_wrappers: HashSet<String> = HashSet::new();
        // Pre-compute the local-name → Function lookup. The renamed
        // export may resolve to a real HIR function (forward to it) or
        // to something else (variable / class / type / `export default
        // function NAME` whose body the HIR lowerer never recorded) —
        // for the latter we emit a no-op so the link still succeeds.
        let func_by_local_name: HashMap<&str, &perry_hir::Function> =
            hir.functions.iter().map(|f| (f.name.as_str(), f)).collect();
        for export in &hir.exports {
            let perry_hir::Export::Named { local, exported } = export else {
                continue;
            };
            if local == exported {
                continue;
            }
            let exported_wrap = crate::symbol_names::function_wrapper(&module_prefix, &exported);
            // Skip if a wrapper with this exact name already exists — the
            // regular wrapper loop above may have produced it when the
            // *local* HIR name happens to sanitize to the same symbol.
            if llmod.has_function(&exported_wrap) {
                continue;
            }
            if !emitted_export_wrappers.insert(exported_wrap.clone()) {
                continue;
            }
            // Determine the local target. Prefer a real HIR function;
            // fall back to a no-op (variable/class/type rename).
            if let Some(f) = func_by_local_name.get(local.as_str()) {
                let arity = f.params.len().min(32);
                let wrap_params =
                    crate::expr::body_call::js_body_params((0..arity).map(|i| format!("%a{}", i)));
                let wf = llmod.define_function(&exported_wrap, DOUBLE, wrap_params);
                let _ = wf.create_block("entry");
                let blk = wf.block_mut(0).unwrap();
                let target = func_names
                    .get(&f.id)
                    .cloned()
                    .unwrap_or_else(|| scoped_fn_name(module_prefix, &f.name));
                let arg_names: Vec<String> = (0..arity).map(|i| format!("%a{}", i)).collect();
                let result = call_function_body(blk, f, &target, &arg_names);
                blk.ret(DOUBLE, &result);
            } else {
                // Variable / class / type rename — no callable function
                // body to forward to. Emit a no-op returning undefined,
                // mirroring `__perry_wrap_perry_unknown_func`.
                //
                // External linkage (the default): consumers in OTHER
                // translation units take this symbol's address through
                // `js_closure_alloc_singleton(@__perry_wrap_<sym>)`, so
                // the wrapper must be visible across TUs. Internal
                // linkage gets DCE'd because the defining TU never
                // references the wrapper itself — `expr.rs:~3819`
                // imports it via `pending_declares`, which generates a
                // `declare` extern in the consumer TU.
                let wf = llmod.define_function(
                    &exported_wrap,
                    DOUBLE,
                    crate::expr::body_call::js_body_params((0..5).map(|i| format!("%a{i}"))),
                );
                let _ = wf.create_block("entry");
                let blk = wf.block_mut(0).unwrap();
                blk.ret(DOUBLE, "0x7FFC000000000001");
            }
        }
    }

    // Publish renamed value getters and wrappers for namespace exports.
    {
        use std::collections::HashSet;
        let mut emitted_aliases: HashSet<String> = HashSet::new();
        let func_by_local_name: HashMap<&str, &perry_hir::Function> =
            hir.functions.iter().map(|f| (f.name.as_str(), f)).collect();
        for export in &hir.exports {
            let perry_hir::Export::Named { local, exported } = export else {
                continue;
            };
            // Namespace initialization reads renamed values through the local
            // getter name. Forward that name to the public getter when this is
            // not a real function alias.
            if local != exported && !func_by_local_name.contains_key(local.as_str()) {
                let local_target = crate::symbol_names::function(&module_prefix, &local);
                let exported_target = crate::symbol_names::function(&module_prefix, &exported);
                if local_target != exported_target
                    && !llmod.has_function(&local_target)
                    && llmod.has_function(&exported_target)
                    && emitted_aliases.insert(local_target.clone())
                {
                    let getter = llmod.define_function(&local_target, DOUBLE, vec![]);
                    let _ = getter.create_block("entry");
                    let blk = getter.block_mut(0).unwrap();
                    let value = blk.call(DOUBLE, &exported_target, &[]);
                    blk.ret(DOUBLE, &value);
                }
            }

            // Sub-bug B: emit no-op wrapper for `local==exported` named
            // exports where local isn't a HIR function and no wrapper
            // is yet defined. Catches `import * as z; export { z };`
            // and any `export { ClassName }` or `export { someConst }`
            // where the consumer reads the value as a closure.
            //
            // Issue #967: when the `local==exported` name IS registered
            // in `hir.exported_functions` as an alias for a real function
            // body (the canonical shape: `function add(a,b){…}; export
            // default add;` lowers to `Export::Named { local: "default",
            // exported: "default" }` *and* pushes `("default", add_id)`
            // into `exported_functions`), the no-op wrapper short-circuits
            // any consumer-side closure dispatch through this default
            // import. The consumer's
            // `__perry_wrap_perry_fn_<src>__default` then transmutes a
            // function pointer that returns `undefined` no matter the args
            // — `const fn = add; fn(2,3)` evaluates to `undefined` instead
            // of `5`. Ramda/date-fns trip this on every `var sum =
            // reduce(add, 0)` style barrel where `add`/`reduce` are
            // default-exports of locally-named functions. Fix: when
            // `exported_functions` points the name at a real HIR function,
            // emit a forwarding wrapper to that function's body (mirrors
            // the `local != exported` branch at L2792 above).
            if local == exported && !func_by_local_name.contains_key(local.as_str()) {
                let exported_wrap =
                    crate::symbol_names::function_wrapper(&module_prefix, &exported);
                if !llmod.has_function(&exported_wrap)
                    && emitted_aliases.insert(exported_wrap.clone())
                {
                    // Check if this name is registered as an alias
                    // pointing at a real HIR function (`export default
                    // <namedFn>` shape).
                    let aliased_func: Option<&perry_hir::Function> = hir
                        .exported_functions
                        .iter()
                        .find(|(n, _)| n == exported)
                        .and_then(|(_, fid)| hir.functions.iter().find(|f| f.id == *fid));
                    if let Some(f) = aliased_func {
                        let arity = f.params.len().min(32);
                        let wrap_params = crate::expr::body_call::js_body_params(
                            (0..arity).map(|i| format!("%a{}", i)),
                        );
                        let wf = llmod.define_function(&exported_wrap, DOUBLE, wrap_params);
                        let _ = wf.create_block("entry");
                        let blk = wf.block_mut(0).unwrap();
                        let target = func_names
                            .get(&f.id)
                            .cloned()
                            .unwrap_or_else(|| scoped_fn_name(module_prefix, &f.name));
                        let arg_names: Vec<String> =
                            (0..arity).map(|i| format!("%a{}", i)).collect();
                        let result = call_function_body(blk, f, &target, &arg_names);
                        blk.ret(DOUBLE, &result);
                    } else {
                        let wf = llmod.define_function(
                            &exported_wrap,
                            DOUBLE,
                            crate::expr::body_call::js_body_params(
                                (0..5).map(|i| format!("%a{i}")),
                            ),
                        );
                        let _ = wf.create_block("entry");
                        let blk = wf.block_mut(0).unwrap();
                        blk.ret(DOUBLE, "0x7FFC000000000001");
                    }
                }
            }
        }
    }
}

/// Call top-level function `f` (public symbol `target`) from one of its
/// value wrappers with the wrapper's JS arguments. A function whose body
/// reads its dynamic `this` is entered through its receiver-taking body
/// (`codegen/function.rs`) with the wrapper's receiver parameter; any other
/// through its public symbol.
fn call_function_body(
    blk: &mut crate::block::LlBlock,
    f: &perry_hir::Function,
    target: &str,
    arg_names: &[String],
) -> String {
    let mut call_args: Vec<(LlvmType, &str)> = Vec::with_capacity(arg_names.len() + 1);
    let receiver_body;
    let callee = if perry_hir::analysis::body_reads_dynamic_this(&f.body) {
        call_args.push((I64, crate::expr::body_call::JS_BODY_THIS));
        receiver_body = crate::expr::body_call::receiver_body_name(target);
        receiver_body.as_str()
    } else {
        target
    };
    call_args.extend(arg_names.iter().map(|n| (DOUBLE, n.as_str())));
    blk.call(DOUBLE, callee, &call_args)
}
