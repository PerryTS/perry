//! Emits the module's exported function-value surface: live getters for
//! re-exported node builtins, the `__perry_wrap_*` closure-ABI wrappers for
//! every top-level user function, and the cross-module name aliases that
//! bridge the producer-side `sanitize()` mangling to the consumer-side
//! verbatim one. Kept separate from the artifact traversal so this wrapper
//! family stays grouped without pushing that orchestration module over the
//! source-size gate.

use std::collections::{HashMap, HashSet};

use perry_hir::Module as HirModule;

use crate::module::LlModule;
use crate::strings::StringPool;
use crate::types::{LlvmType, DOUBLE, I64};

use super::helpers::{function_body_returns_generator_object, sanitize, scoped_fn_name};

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
            if !import.is_native {
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
        let getter_name = format!("perry_fn_{}__{}", module_prefix, sanitize(exported));
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
    // a thin wrapper `__perry_wrap_<name>` whose signature matches the
    // closure-call ABI: `double(i64 this_closure, double arg0, double
    // arg1, ...)`. Most wrappers discard the closure pointer and forward the
    // args to the underlying function; generator wrappers reuse it to link the
    // returned iterator to the closure-cached `prototype`.
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
        let mut wrap_params: Vec<(LlvmType, String)> = vec![(I64, "%this_closure".to_string())];
        for name in &arg_names {
            wrap_params.push((DOUBLE, name.clone()));
        }
        let wrap_name = format!("__perry_wrap_{}", original_name);
        let wf = llmod.define_function(&wrap_name, DOUBLE, wrap_params);
        let _ = wf.create_block("entry");
        let blk = wf.block_mut(0).unwrap();
        // Call the underlying function with just the arg doubles.
        let call_args: Vec<(LlvmType, &str)> =
            arg_names.iter().map(|n| (DOUBLE, n.as_str())).collect();
        let mut result = blk.call(DOUBLE, &original_name, &call_args);
        if function_body_returns_generator_object(&f.body) {
            result = blk.call(
                DOUBLE,
                "js_generator_attach_closure_prototype",
                &[(DOUBLE, &result), (I64, "%this_closure")],
            );
        }
        blk.ret(DOUBLE, &result);
    }

    // Exported function names use a stable, plain-sanitized cross-module ABI,
    // while local function bodies use the injective `sanitize_member` mangling
    // so hostile-but-valid identifiers cannot collide inside one module. The
    // direct-call aliases bridge those schemes above; closure values need the
    // identical bridge for their `__perry_wrap_*` symbols.
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
            let alias_wrap = format!(
                "__perry_wrap_perry_fn_{}__{}",
                module_prefix,
                sanitize(exported_name)
            );
            if alias_wrap == target_wrap
                || llmod.has_function(&alias_wrap)
                || !emitted_aliases.insert(alias_wrap.clone())
            {
                continue;
            }

            let arity = f.params.len();
            let mut params: Vec<(LlvmType, String)> = vec![(I64, "%this_closure".to_string())];
            params.extend((0..arity).map(|i| (DOUBLE, format!("%a{}", i))));
            let alias = llmod.define_function(&alias_wrap, DOUBLE, params);
            let _ = alias.create_block("entry");
            let blk = alias.block_mut(0).unwrap();
            let mut args: Vec<(LlvmType, String)> = vec![(I64, "%this_closure".to_string())];
            args.extend((0..arity).map(|i| (DOUBLE, format!("%a{}", i))));
            let arg_refs: Vec<(LlvmType, &str)> = args
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
            let exported_wrap = format!(
                "__perry_wrap_perry_fn_{}__{}",
                module_prefix,
                sanitize(exported)
            );
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
                let mut wrap_params: Vec<(LlvmType, String)> =
                    vec![(I64, "%this_closure".to_string())];
                for i in 0..arity {
                    wrap_params.push((DOUBLE, format!("%a{}", i)));
                }
                let wf = llmod.define_function(&exported_wrap, DOUBLE, wrap_params);
                let _ = wf.create_block("entry");
                let blk = wf.block_mut(0).unwrap();
                let target = func_names
                    .get(&f.id)
                    .cloned()
                    .unwrap_or_else(|| scoped_fn_name(module_prefix, &f.name));
                let call_args: Vec<(LlvmType, String)> =
                    (0..arity).map(|i| (DOUBLE, format!("%a{}", i))).collect();
                let call_args_ref: Vec<(LlvmType, &str)> =
                    call_args.iter().map(|(t, s)| (*t, s.as_str())).collect();
                let result = blk.call(DOUBLE, &target, &call_args_ref);
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
                    vec![
                        (I64, "%this_closure".to_string()),
                        (DOUBLE, "%a0".to_string()),
                        (DOUBLE, "%a1".to_string()),
                        (DOUBLE, "%a2".to_string()),
                        (DOUBLE, "%a3".to_string()),
                        (DOUBLE, "%a4".to_string()),
                    ],
                );
                let _ = wf.create_block("entry");
                let blk = wf.block_mut(0).unwrap();
                blk.ret(DOUBLE, "0x7FFC000000000001");
            }
        }
    }

    // Closes #836: two additional producer-side emissions for cross-module
    // exports that the regular wrapper/getter/stub loops above leave behind.
    //
    // Sub-bug A — sanitize-mismatch raw aliases. The producer side hashes
    // every export through `sanitize()` (replaces every non-alphanumeric +
    // non-underscore char with `_`), so `export const $ZodCheck` lands at
    // `perry_fn_<src>___ZodCheck` (one `_` from the `__` separator, two
    // from the sanitized `$`). The consumer side, in contrast, builds the
    // callee symbol with `import_origin_suffix()`, which returns the
    // exported name VERBATIM — so consumer references read
    // `perry_fn_<src>__$ZodCheck` and link-fail. The mismatch hits every
    // export whose name contains `$` (zod's `$ZodCheck`/`$ZodCheckString
    // Format`/`$constructor` family — ~80 symbols on the zod surface), or
    // any other character `sanitize()` rewrites. Fix: for every named
    // export where `sanitize(name) != name`, emit raw-name aliases
    // forwarding to the canonical definition (not a colliding sanitized name).
    //
    // Sub-bug B — missing `__perry_wrap_perry_fn_<src>__<exported>` for
    // `local == exported` non-function exports. Concrete shape:
    //   import * as z from "./external.js";
    //   export { z };
    //   export default z;
    // The `Export::Named { local: "z", exported: "z" }` entry is skipped
    // by every loop above because `local == exported` and `z` is not a
    // HIR function (it's a namespace import alias). A consumer that
    // imports `{ z }` from this module references the wrapper symbol
    // when reading `z` as a value (e.g. `console.log(z)` or passing it
    // as a callback), and link-fails on the missing wrapper. Fix: emit
    // a no-op wrapper for every named export whose `local==exported`
    // name is not the body of a HIR function (and where no wrapper has
    // already been emitted by the regular `__perry_wrap_<fn>` loop).
    //
    // Note: anonymous `export default function() {...}` produces zero
    // exports and zero functions in the HIR today (lower.rs drops it on
    // the floor — separate bug, will need its own PR to wire up the
    // synthetic `default` name). That path is OUT OF SCOPE here; the
    // test below uses a named default to sidestep it.
    {
        use std::collections::HashSet;
        let mut emitted_aliases: HashSet<String> = HashSet::new();
        let func_by_local_name: HashMap<&str, &perry_hir::Function> =
            hir.functions.iter().map(|f| (f.name.as_str(), f)).collect();
        let func_by_id: HashMap<_, _> = hir.functions.iter().map(|f| (f.id, f)).collect();
        let func_by_export_name: HashMap<_, _> = hir
            .exported_functions
            .iter()
            .filter_map(|(name, id)| func_by_id.get(id).map(|f| (name.as_str(), *f)))
            .collect();
        for export in &hir.exports {
            let perry_hir::Export::Named { local, exported } = export else {
                continue;
            };
            let sanitized = sanitize(exported);

            // Sub-bug A: emit raw-name aliases when the exported name
            // sanitizes to a different symbol. Two aliases per mismatch:
            //   * `perry_fn_<src>__<raw_exported>` — value/getter form,
            //     forwards to the exact function body, or a variable getter.
            //   * `__perry_wrap_perry_fn_<src>__<raw_exported>` — closure-
            //     wrapper form, forwards to the matching canonical wrapper if it
            //     exists, otherwise emits a no-op (matches the variable/
            //     class branch in the #837 loop above).
            if sanitized != *exported {
                // `$n` and `_n` share sanitize() output but are distinct bodies.
                // The export's function ID is authoritative, including aliases
                // whose local name no longer identifies the original function.
                let function = func_by_export_name
                    .get(exported.as_str())
                    .or_else(|| func_by_local_name.get(local.as_str()));
                let target = function
                    .and_then(|f| func_names.get(&f.id))
                    .cloned()
                    .unwrap_or_else(|| format!("perry_fn_{}__{}", module_prefix, sanitized));
                let raw_target = format!("perry_fn_{}__{}", module_prefix, exported);
                if !llmod.has_function(&raw_target) && emitted_aliases.insert(raw_target.clone()) {
                    // Variable getters take no arguments; function aliases must
                    // match the exact target's arity, not its sanitized sibling.
                    let param_count = function.map(|f| f.params.len()).unwrap_or(0);
                    let wrap_params: Vec<(LlvmType, String)> = (0..param_count)
                        .map(|i| (DOUBLE, format!("%a{}", i)))
                        .collect();
                    let wf = llmod.define_function(&raw_target, DOUBLE, wrap_params);
                    let _ = wf.create_block("entry");
                    let blk = wf.block_mut(0).unwrap();
                    let arg_names: Vec<String> =
                        (0..param_count).map(|i| format!("%a{}", i)).collect();
                    let call_args: Vec<(LlvmType, &str)> =
                        arg_names.iter().map(|s| (DOUBLE, s.as_str())).collect();
                    let result = blk.call(DOUBLE, &target, &call_args);
                    blk.ret(DOUBLE, &result);
                }
                let raw_wrap = format!("__perry_wrap_perry_fn_{}__{}", module_prefix, exported);
                let target_wrap = format!("__perry_wrap_{}", target);
                if !llmod.has_function(&raw_wrap) && emitted_aliases.insert(raw_wrap.clone()) {
                    if llmod.has_function(&target_wrap) {
                        // Match the canonical wrapper's closure-call ABI (one
                        // double per declared param), including renamed exports.
                        let arity = function.map(|f| f.params.len()).unwrap_or(5);
                        let mut params = vec![(I64, "%this_closure".to_string())];
                        params.extend((0..arity).map(|i| (DOUBLE, format!("%a{}", i))));
                        let wf = llmod.define_function(&raw_wrap, DOUBLE, params.clone());
                        let _ = wf.create_block("entry");
                        let blk = wf.block_mut(0).unwrap();
                        let args: Vec<_> = params
                            .iter()
                            .map(|(ty, name)| (*ty, name.as_str()))
                            .collect();
                        let result = blk.call(DOUBLE, &target_wrap, &args);
                        blk.ret(DOUBLE, &result);
                    } else {
                        // No sanitized wrapper either (variable/class/
                        // namespace re-export with sanitize-mismatch
                        // name). Emit a no-op returning undefined.
                        let wf = llmod.define_function(
                            &raw_wrap,
                            DOUBLE,
                            vec![
                                (I64, "%this_closure".to_string()),
                                (DOUBLE, "%a0".to_string()),
                                (DOUBLE, "%a1".to_string()),
                                (DOUBLE, "%a2".to_string()),
                                (DOUBLE, "%a3".to_string()),
                                (DOUBLE, "%a4".to_string()),
                            ],
                        );
                        let _ = wf.create_block("entry");
                        let blk = wf.block_mut(0).unwrap();
                        blk.ret(DOUBLE, "0x7FFC000000000001");
                    }
                }
            }

            // Namespace initialization reads renamed values through the local
            // getter name. Forward that name to the public getter when this is
            // not a real function alias.
            if local != exported && !func_by_local_name.contains_key(local.as_str()) {
                let local_target = format!("perry_fn_{}__{}", module_prefix, sanitize(local));
                let exported_target = format!("perry_fn_{}__{}", module_prefix, sanitize(exported));
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
                let exported_wrap = format!(
                    "__perry_wrap_perry_fn_{}__{}",
                    module_prefix,
                    sanitize(exported)
                );
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
                        let mut wrap_params: Vec<(LlvmType, String)> =
                            vec![(I64, "%this_closure".to_string())];
                        for i in 0..arity {
                            wrap_params.push((DOUBLE, format!("%a{}", i)));
                        }
                        let wf = llmod.define_function(&exported_wrap, DOUBLE, wrap_params);
                        let _ = wf.create_block("entry");
                        let blk = wf.block_mut(0).unwrap();
                        let target = func_names
                            .get(&f.id)
                            .cloned()
                            .unwrap_or_else(|| scoped_fn_name(module_prefix, &f.name));
                        let call_args: Vec<(LlvmType, String)> =
                            (0..arity).map(|i| (DOUBLE, format!("%a{}", i))).collect();
                        let call_args_ref: Vec<(LlvmType, &str)> =
                            call_args.iter().map(|(t, s)| (*t, s.as_str())).collect();
                        let result = blk.call(DOUBLE, &target, &call_args_ref);
                        blk.ret(DOUBLE, &result);
                    } else {
                        let wf = llmod.define_function(
                            &exported_wrap,
                            DOUBLE,
                            vec![
                                (I64, "%this_closure".to_string()),
                                (DOUBLE, "%a0".to_string()),
                                (DOUBLE, "%a1".to_string()),
                                (DOUBLE, "%a2".to_string()),
                                (DOUBLE, "%a3".to_string()),
                                (DOUBLE, "%a4".to_string()),
                            ],
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
