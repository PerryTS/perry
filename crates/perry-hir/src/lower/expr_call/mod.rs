//! Function call expression lowering: `ast::Expr::Call`.
//!
//! Tier 2.3 round 4 (v0.5.339) — extracts the 3,986-LOC `Call` arm
//! from `lower_expr`. By far the largest single arm in the entire
//! codebase. This is a giant dispatcher: figure out what's being
//! called (built-in like Math.floor, native module method like
//! `mysql.query()`, user function, closure, etc.) and emit the right
//! HIR variant.
//!
//! Pattern matches the prior expr_*.rs extractions: free
//! `pub(super) fn` entry, recursion through `super::lower_expr`.
//! Module is intentionally one big function; further sub-extraction
//! by call category (Math / JSON / fetch / native / class-static /
//! …) is a follow-up — splitting them all in a single PR would
//! balloon the diff and the borrow-checker dance is non-trivial.

use anyhow::{anyhow, Result};
use perry_types::{LocalId, Type};
use swc_ecma_ast as ast;

use crate::ir::*;
use crate::lower_patterns::detect_native_instance_expr;
use crate::lower_types::extract_ts_type_with_ctx;

use super::{
    extract_typed_parse_source_order, is_generator_call_expr, is_widget_modifier_name, lower_expr,
    resolve_typed_parse_ty, LoweringContext,
};

mod array_only_methods;
mod globals;
mod imported_array_methods;
mod inline_array_methods;
mod intrinsics;
mod local_array_methods;
mod module_class_static;
mod module_static;
mod native_module;
mod nested_namespace;
mod object_static;
mod post_args_dispatch;
mod prescans;
mod reflect_args;
mod regex_string;
mod static_and_instance;
mod static_receiver;
mod stream;
mod textencoder;
mod url_date_instance;
mod url_search_params;
mod wasm_exports;

use array_only_methods::try_array_only_methods;
use globals::try_global_builtins;
use imported_array_methods::try_imported_array_methods;
use inline_array_methods::try_inline_array_methods;
use intrinsics::{
    try_bare_regexp_call, try_embed_wasm, try_function_return_this, try_iife_call_rewrite,
    try_require_literal_bail,
};
use local_array_methods::try_local_array_methods;
use module_class_static::try_module_class_static;
use module_static::try_module_static_methods;
use native_module::try_native_module_methods;
use nested_namespace::{try_path_subnamespace, try_process_hrtime_bigint, try_web_crypto_subtle};
use post_args_dispatch::{
    try_object_has_own_call, try_object_prototype_call, try_object_static_alias_call,
    try_proxy_call,
};
use prescans::run_call_prescans;
use regex_string::try_regex_string_methods;
use static_and_instance::try_static_method_and_instance;
use textencoder::try_textencoder_decoder;
use url_date_instance::try_url_date_weakref_instance;
use wasm_exports::try_wasm_instance_exports;

/// Issue #1132 — scope the native-instance param tags that
/// `lower_call_inner`'s pre-scans register (createServer's
/// `(req, res)`, http.get's `(res)`, fastify's `(req, reply)`, the
/// `'upgrade'` `wsId`, …) to the call expression that owns the
/// handler arrow.
///
/// Those pre-scans `register_native_instance(...)` BEFORE the handler
/// arrow's `lower_arrow` runs (so the arrow body can see the tag),
/// which means the tag is pushed at the *enclosing* scope position —
/// before the arrow's own `enter_scope` mark — so the arrow's
/// `exit_scope` does NOT truncate it. Pre-#1132 the tag therefore
/// leaked into the whole enclosing function/module scope. Combined
/// with the old first-match `lookup_native_instance`, an inner
/// callback re-binding the same name (`createServer((req, res) =>
/// httpGet(url, (res) => …))`) resolved its `res` to the outer
/// `("http", "ServerResponse")` tag.
///
/// Fix (option 2 from the issue — "register inner, restore on scope
/// exit"): snapshot `native_instances.len()` here, run the real
/// lowering (which registers + uses the pre-scan tags while the
/// handler arrow body is lowered, all within this call), then
/// truncate back to the snapshot on the way out. The pre-scan tags
/// only ever matter while their handler arrow's body is being lowered
/// — which happens inside this `lower_call` invocation — so dropping
/// them when the call returns restores any outer binding of the same
/// name without affecting correctness. Nested calls each get their
/// own snapshot, so an inner http.get's `(res)` tag is dropped when
/// the inner call returns, re-exposing the outer createServer's
/// `(res)` for any later use in the outer body.
pub(super) fn lower_call(ctx: &mut LoweringContext, call: &ast::CallExpr) -> Result<Expr> {
    let ni_mark = ctx.native_instances.len();
    let result = lower_call_inner(ctx, call);
    // Restore: drop any native-instance tags this call's pre-scans
    // added (and anything nested calls left above the mark — those
    // are likewise out of scope once we unwind past their owning
    // call). `truncate` is a no-op when nothing was added.
    if ctx.native_instances.len() > ni_mark {
        ctx.native_instances.truncate(ni_mark);
    }
    result
}

fn lower_call_inner(ctx: &mut LoweringContext, call: &ast::CallExpr) -> Result<Expr> {
    // Check if any argument has spread
    let has_spread = call.args.iter().any(|arg| arg.spread.is_some());

    // Pre-scans: register native-instance tags for handler params
    // (fastify/http/ws/streams) BEFORE the arrow bodies are lowered,
    // and run the perry/ui reactive Text/animate desugars. If a
    // reactive desugar fires, return it directly.
    if let Some(desugared) = run_call_prescans(ctx, call)? {
        return Ok(desugared);
    }

    // Compile-time intrinsics + legacy CJS/UMD bare-callee shapes
    // (require/embedWasm/IIFE.call/Function('return this')/RegExp).
    try_require_literal_bail(ctx, call)?;
    if let Some(expr) = try_embed_wasm(ctx, call)? {
        return Ok(expr);
    }
    if let Some(expr) = try_iife_call_rewrite(ctx, call, has_spread)? {
        return Ok(expr);
    }
    if let Some(expr) = try_function_return_this(ctx, call, has_spread) {
        return Ok(expr);
    }
    if let Some(expr) = try_bare_regexp_call(ctx, call, has_spread)? {
        return Ok(expr);
    }

    let mut args = call
        .args
        .iter()
        .map(|arg| lower_expr(ctx, &arg.expr))
        .collect::<Result<Vec<_>>>()?;

    // Post-args dispatch hooks: proxy apply/revoke, `Object.<static>`
    // aliased calls, and `Object.prototype.<method>.call(...)` rewrites.
    args = match try_proxy_call(ctx, call, args, has_spread) {
        Ok(expr) => return Ok(expr),
        Err(args) => args,
    };
    args = match try_object_static_alias_call(ctx, call, args, has_spread) {
        Ok(expr) => return Ok(expr),
        Err(args) => args,
    };
    args = match try_object_has_own_call(call, args, has_spread) {
        Ok(expr) => return Ok(expr),
        Err(args) => args,
    };
    let mut args = match try_object_prototype_call(call, args, has_spread) {
        Ok(expr) => return Ok(expr),
        Err(args) => args,
    };

    // If spread is present, create CallSpread instead of Call
    let spread_args: Option<Vec<CallArg>> = if has_spread {
        Some(
            call.args
                .iter()
                .zip(args.iter())
                .map(|(ast_arg, lowered)| {
                    if ast_arg.spread.is_some() {
                        CallArg::Spread(lowered.clone())
                    } else {
                        CallArg::Expr(lowered.clone())
                    }
                })
                .collect(),
        )
    } else {
        None
    };

    match &call.callee {
        ast::Callee::Super(_) => {
            // super() call in constructor
            Ok(Expr::SuperCall(args))
        }
        ast::Callee::Expr(expr) => {
            // Check for super.method() call
            if let ast::Expr::SuperProp(super_prop) = expr.as_ref() {
                if let ast::SuperProp::Ident(ident) = &super_prop.prop {
                    return Ok(Expr::SuperMethodCall {
                        method: ident.sym.to_string(),
                        args,
                    });
                }
            }

            // Nested 3-level Member dispatch: process.hrtime.bigint(),
            // crypto.subtle.<method>(), path.posix/win32.<method>().
            args = match try_process_hrtime_bigint(expr, args) {
                Ok(e) => return Ok(e),
                Err(a) => a,
            };
            args = match try_web_crypto_subtle(ctx, expr, args)? {
                Ok(e) => return Ok(e),
                Err(a) => a,
            };
            args = match try_path_subnamespace(ctx, expr, args) {
                Ok(e) => return Ok(e),
                Err(a) => a,
            };

            // module.Class.staticMethod() and process.std{in,out} dispatch.
            args = match try_module_class_static(ctx, call, expr, args)? {
                Ok(e) => return Ok(e),
                Err(a) => a,
            };

            // Native module method calls (process/tty/os/Buffer/Uint8Array/Object/Symbol/Array/net).
            args = match try_native_module_methods(ctx, call, expr, args)? {
                Ok(e) => return Ok(e),
                Err(a) => a,
            };

            // Static class method + native-instance method dispatch.
            args = match try_static_method_and_instance(ctx, call, expr, args)? {
                Ok(e) => return Ok(e),
                Err(a) => a,
            };

            // `<inst>.exports.<method>(...)` for WebAssembly JS API.
            args = match try_wasm_instance_exports(ctx, call, expr, args)? {
                Ok(e) => return Ok(e),
                Err(a) => a,
            };

            // fs/path/JSON/Math/Number/String/crypto/os/Buffer/cp/net/AbortSignal/Date/URL static methods.
            args = match try_module_static_methods(ctx, call, expr, args, has_spread)? {
                Ok(e) => return Ok(e),
                Err(a) => a,
            };

            // URL/URLSearchParams/Date instance methods + WeakRef/FinalizationRegistry.
            args = match try_url_date_weakref_instance(ctx, call, expr, args)? {
                Ok(e) => return Ok(e),
                Err(a) => a,
            };

            // Array method calls on local-variable receivers.
            args = match try_local_array_methods(ctx, call, expr, args)? {
                Ok(e) => return Ok(e),
                Err(a) => a,
            };

            // Array methods on imported variables (ExternFuncRef receivers).
            args = match try_imported_array_methods(ctx, call, args)? {
                Ok(e) => return Ok(e),
                Err(a) => a,
            };

            // Array methods on inline array literals.
            args = match try_inline_array_methods(ctx, call, args)? {
                Ok(e) => return Ok(e),
                Err(a) => a,
            };

            // TextEncoder.encode / TextDecoder.decode on inline expressions.
            args = match try_textencoder_decoder(ctx, call, args)? {
                Ok(e) => return Ok(e),
                Err(a) => a,
            };

            // Array-only methods on arbitrary expressions.
            args = match try_array_only_methods(ctx, call, args)? {
                Ok(e) => return Ok(e),
                Err(a) => a,
            };

            // Regex .test()/.exec() + String .match(regex).
            args = match try_regex_string_methods(ctx, call, args)? {
                Ok(e) => return Ok(e),
                Err(a) => a,
            };

            // Global builtins (parseInt/parseFloat/Number/String/isNaN/isFinite/...).
            args = match try_global_builtins(ctx, call, expr, args)? {
                Ok(e) => return Ok(e),
                Err(a) => a,
            };

            // ---------------------------------------------------------------
            // Fall-through tail: lower the callee, fill in default arguments
            // for known callees, fold namespace-function calls into
            // StaticMethodCall, and emit Call / CallSpread.
            // ---------------------------------------------------------------
            let callee_expr = lower_expr(ctx, expr)?;

            // Fill in default arguments if callee is a known function
            let mut args = args;
            if let Expr::FuncRef(func_id) = &callee_expr {
                if let Some((defaults, param_ids, rest_idx, has_synth_args)) =
                    ctx.lookup_func_defaults(*func_id)
                {
                    // Refs #653 followup to v0.5.789's #645 fix: stop the
                    // default-fill loop BEFORE the rest param's slot. Pushing
                    // `Expr::Undefined` for a rest param turns
                    // `function f(name, ...args)` called as `f('a')` into
                    // `f('a', undefined)`, which the runtime then spreads into
                    // `args[0] = undefined`. Real semantics: trailing positional
                    // args get bundled into the rest array at runtime; missing
                    // ones produce an empty array, NOT a single-undefined array.
                    //
                    // Issue #1069: synthetic-`arguments` rest param. For
                    // `function f(a, b) { arguments }`, the HIR appends a
                    // hidden `arguments`-named rest param so the body can
                    // reference it. The codegen call-site synth-args path
                    // (`crates/perry-codegen/src/lower_call.rs:974`) uses
                    // `args.len()` to size the runtime `arguments` array
                    // AND pads missing fixed-param slots with `undefined`
                    // itself, so the body's `if (param === undefined) {
                    // param = default; }` prefix applies user defaults
                    // there. If we let the loop below push Undefined into
                    // the fixed-param slots, `args.len()` inflates to the
                    // declared fixed-param count and `arguments.length`
                    // reads as the declared count regardless of the
                    // caller's actual arg count — `f()` reports
                    // `arguments.length === 2` instead of `0`. Skip the
                    // fill loop entirely for synth-args callees.
                    if has_synth_args {
                        // Skip the default-fill loop. Codegen handles the
                        // fixed-param padding and the body's default-fill
                        // statements substitute user defaults.
                    } else {
                        let defaults = defaults.to_vec();
                        let param_ids = param_ids.to_vec();
                        let num_provided = args.len();
                        let fill_end = match rest_idx {
                            Some(i) => i,
                            None => defaults.len(),
                        };
                        // Build substitution map: callee param LocalId -> actual arg expression
                        // For provided args, map to the caller's arg expression
                        // For defaulted args, map to the expanded default (built incrementally)
                        let mut param_map: Vec<(LocalId, Expr)> = Vec::new();
                        for i in 0..param_ids.len().min(num_provided) {
                            param_map.push((param_ids[i], args[i].clone()));
                        }
                        // Fill in missing arguments with their defaults, substituting
                        // any parameter references to use the caller's scope.
                        //
                        // Refs #645 deeper followup / #488 drizzle-sqlite: push
                        // something for EVERY slot from num_provided..defaults.len(),
                        // even when defaults[i] is None — otherwise a later Some
                        // default lands at the wrong positional slot. Drizzle's
                        // tableBase(name, columns, extraConfig, schema?, baseName=name)
                        // is the load-bearing repro: 3-arg call, slot 3 (schema)
                        // has None default and slot 4 (baseName) has Some(name).
                        // The pre-fix loop skipped slot 3 and pushed baseName's
                        // default into slot 3 — so schema got the table name and
                        // rendered SQL became `"users"."users"` instead of `"users"`.
                        for i in num_provided..fill_end {
                            let substituted = if let Some(default_expr) = &defaults[i] {
                                LoweringContext::substitute_param_refs_in_default(
                                    default_expr,
                                    &param_map,
                                )
                            } else {
                                Expr::Undefined
                            };
                            // Add this expanded default to the map so later defaults
                            // can reference it (e.g., c = b where b was also defaulted)
                            if i < param_ids.len() {
                                param_map.push((param_ids[i], substituted.clone()));
                            }
                            args.push(substituted);
                        }
                    } // end of !has_synth_args branch
                }
            }

            // If inside a namespace, convert calls to namespace functions into StaticMethodCall
            if let Expr::FuncRef(func_id) = &callee_expr {
                if let Some(ref ns_name) = ctx.current_namespace {
                    if let Some(func_name) = ctx.lookup_func_name(*func_id) {
                        if ctx.has_static_method(ns_name, func_name) {
                            let method_name = func_name.to_string();
                            let class_name = ns_name.clone();
                            return Ok(Expr::StaticMethodCall {
                                class_name,
                                method_name,
                                args,
                            });
                        }
                    }
                }
            }

            let callee = Box::new(callee_expr);
            // Extract explicit type arguments if present (e.g., identity<number>(x))
            let type_args = call
                .type_args
                .as_ref()
                .map(|ta| {
                    ta.params
                        .iter()
                        .map(|t| extract_ts_type_with_ctx(t, Some(ctx)))
                        .collect()
                })
                .unwrap_or_default();

            // Use CallSpread if any argument has spread
            if let Some(spread_args) = spread_args {
                Ok(Expr::CallSpread {
                    callee,
                    args: spread_args,
                    type_args,
                })
            } else {
                Ok(Expr::Call {
                    callee,
                    args,
                    type_args,
                })
            }
        }
        ast::Callee::Import(_) => {
            // Issue #100: dynamic import() — lower to Expr::DynamicImport with
            // an empty `paths` list. `collect_modules` runs the path-const
            // folder and populates `paths`. If the argument can't be folded
            // to a finite set, that pass raises a compile error before codegen
            // sees the empty node.
            let arg = args
                .into_iter()
                .next()
                .ok_or_else(|| anyhow::anyhow!("dynamic import() requires a path argument"))?;
            Ok(Expr::DynamicImport {
                paths: Vec::new(),
                arg: Box::new(arg),
            })
        }
    }
}
