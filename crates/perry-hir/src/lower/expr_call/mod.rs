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

use anyhow::Result;
use swc_ecma_ast as ast;

use crate::ir::*;
use crate::lower_types::extract_ts_type_with_ctx;

use super::{lower_expr, LoweringContext};

mod array_only_methods;
mod crypto;
mod globals;
mod imported_array_methods;
mod inline_array_methods;
pub(crate) mod intrinsics;
mod local_array_methods;
mod module_class_static;
mod module_static;
mod name_fold;
mod native_module;
mod nested_namespace;
mod object_static;
mod os;
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
    check_eval_function_call, try_bare_regexp_call, try_builtin_prototype_method_apply_call,
    try_dynamic_require, try_embed_wasm, try_function_return_this, try_iife_call_rewrite,
    try_iterator_from, try_namespace_static_method_apply_call_bind, try_native_arena_intrinsics,
    try_native_arena_public_api, try_native_memory_public_api, try_native_module_method_apply_call,
    try_pod_layout_constants, try_precompile, try_require_literal,
    try_strict_eval_arguments_assignment,
};
use local_array_methods::try_local_array_methods;
use module_class_static::try_module_class_static;
use module_static::try_module_static_methods;
use native_module::try_native_module_methods;
use nested_namespace::{
    try_dns_promises_namespace, try_path_subnamespace, try_process_hrtime_bigint,
    try_process_memory_usage_rss, try_punycode_ucs2_namespace, try_util_types_namespace,
    try_web_crypto_subtle,
};
use post_args_dispatch::{
    try_array_static_alias_call, try_direct_has_own_call, try_direct_property_is_enumerable_call,
    try_object_has_own_call, try_object_prototype_call, try_object_static_alias_call,
    try_proxy_call,
};
use prescans::run_call_prescans;
use regex_string::try_regex_string_methods;
use static_and_instance::try_static_method_and_instance;
use textencoder::try_textencoder_decoder;
use url_date_instance::try_url_date_weakref_instance;
use wasm_exports::try_wasm_instance_exports;

fn unwrap_call_callee_ts_wrappers(e: &ast::Expr) -> &ast::Expr {
    let mut cur = e;
    loop {
        match cur {
            ast::Expr::TsAs(x) => cur = &x.expr,
            ast::Expr::TsNonNull(x) => cur = &x.expr,
            ast::Expr::TsSatisfies(x) => cur = &x.expr,
            ast::Expr::TsTypeAssertion(x) => cur = &x.expr,
            ast::Expr::TsConstAssertion(x) => cur = &x.expr,
            ast::Expr::Paren(x) => cur = &x.expr,
            _ => return cur,
        }
    }
}

/// #6677 — the method name of a namespace-builtin call callee, for EITHER the
/// dot form (`Math.max(...)`) or the string-literal computed form
/// (`Math["max"](...)`). Per spec `obj.m()` and `obj["m"]()` are equivalent for
/// a string key, so the call-dispatch arms that statically devirtualize
/// `Math`/`JSON`/`Object`/… methods must fire for both forms — otherwise a
/// string-literal computed key never matches the `MemberProp::Ident`-only gate,
/// falls through to generic dispatch (which drops the namespace receiver and
/// lowers the callee to an undefined global), and throws
/// `TypeError: value is not a function` at runtime. The property-READ side
/// already normalizes both forms via `expr_member::static_member_prop_name`;
/// this is the borrowing (`&str`, no allocation) call-side twin, mirroring the
/// `Symbol['for']`/`Symbol['keyFor']` precedent in `native_module.rs`. Returns
/// `None` for a dynamic (non-string-literal) computed key, which must keep the
/// runtime index path.
pub(super) fn static_call_prop_name(prop: &ast::MemberProp) -> Option<&str> {
    match prop {
        ast::MemberProp::Ident(p) => Some(p.sym.as_ref()),
        ast::MemberProp::Computed(c) => match c.expr.as_ref() {
            ast::Expr::Lit(ast::Lit::Str(s)) => s.value.as_str(),
            _ => None,
        },
        ast::MemberProp::PrivateName(_) => None,
    }
}

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
    // #1723: snapshot/restore the one-shot #503 suppression flag around the
    // whole call. `lower_call_inner` may set it (for a `ns[dynamicKey].method()`
    // receiver) but a dispatch arm could `return` before the receiver is
    // lowered, leaving the flag set. Restoring here keeps it from leaking into
    // a sibling expression.
    let prev_suppress = ctx.suppress_stdlib_dispatch_guard_once;
    let result = lower_call_inner(ctx, call);
    ctx.suppress_stdlib_dispatch_guard_once = prev_suppress;
    // Restore: drop any native-instance tags this call's pre-scans
    // added (and anything nested calls left above the mark — those
    // are likewise out of scope once we unwind past their owning
    // call). `truncate` is a no-op when nothing was added.
    if ctx.native_instances.len() > ni_mark {
        ctx.truncate_native_instances(ni_mark);
    }
    result
}

fn lower_call_inner(ctx: &mut LoweringContext, call: &ast::CallExpr) -> Result<Expr> {
    // Safety net for the receiver-lowering memo (see
    // `LoweringContext::prelowered_member_receiver`): the memo is set by
    // `try_static_method_and_instance` only to be consumed by the immediately
    // following fall-through tail's `lower_member_inner`. It is single-shot and
    // span-keyed, but in case a code path sets it without the tail consuming it,
    // drop any stale entry here so it can never leak across calls. This runs
    // before the callee tail (which lowers a Member, not a Call), so it never
    // clobbers an in-flight memo for the call currently being lowered.
    ctx.prelowered_member_receiver = None;
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
    if let Some(expr) = try_require_literal(ctx, call)? {
        return Ok(expr);
    }
    // #5389 Tier 2: a computed `require(expr)` in a compiled external module
    // lowers to a synchronous dynamic-require node (resolved + dispatched like
    // dynamic `import()`, but returning the namespace value directly).
    if let Some(expr) = try_dynamic_require(ctx, call)? {
        return Ok(expr);
    }
    if let Some(expr) = try_embed_wasm(ctx, call)? {
        return Ok(expr);
    }
    // #1681: `precompile(EXPR)` build-time codegen — capture stage emits the
    // build-time-evaluated source; main compile substitutes the compiled fn.
    if let Some(expr) = try_precompile(ctx, call)? {
        return Ok(expr);
    }
    if let Some(expr) = try_pod_layout_constants(ctx, call, has_spread)? {
        return Ok(expr);
    }
    if let Some(expr) = try_native_arena_public_api(ctx, call, has_spread)? {
        return Ok(expr);
    }
    if let Some(expr) = try_native_memory_public_api(ctx, call, has_spread)? {
        return Ok(expr);
    }
    if let Some(expr) = try_native_arena_intrinsics(ctx, call, has_spread)? {
        return Ok(expr);
    }
    if let Some(expr) = try_iife_call_rewrite(ctx, call, has_spread)? {
        return Ok(expr);
    }
    // #1722: `path.join.apply(null, [a, b])` / `.call(null, a, b)` and the
    // same shape on any stdlib namespace — rewrite to the direct call so
    // the dedicated per-method lowering runs (indirect invocation
    // otherwise reads the method value as `undefined`).
    if let Some(expr) = try_native_module_method_apply_call(ctx, call, has_spread)? {
        return Ok(expr);
    }
    // #1777: `Array.prototype.slice.call(arguments, 1)` / `[].slice.call(…)` and
    // the same on any builtin prototype — rewrite `Proto.method.{call,apply}(
    // thisArg, …)` to `thisArg.method(…)` so the indirect prototype-borrow
    // dispatches (the bare method value otherwise reads `undefined`).
    if let Some(expr) = try_builtin_prototype_method_apply_call(ctx, call, has_spread)? {
        return Ok(expr);
    }
    // #2143: `Promise.resolve.call(t, x)` / `Math.min.apply(t, [a,b])` /
    // `(JSON.parse.bind(t, x))(y)` — drop the (unused) `thisArg` and call the
    // namespace static directly. Built-in function values otherwise lower to a
    // numeric fallback (no reified `Function.prototype`).
    if let Some(expr) = try_namespace_static_method_apply_call_bind(ctx, call, has_spread)? {
        return Ok(expr);
    }
    if let Some(expr) = try_function_return_this(ctx, call, has_spread) {
        return Ok(expr);
    }
    // Strict-mode early errors in a literal eval body must throw the
    // SyntaxError at the eval() call — checked BEFORE the const-fold so a
    // foldable body carrying a violation doesn't compile through.
    if let Some(expr) = try_strict_eval_arguments_assignment(ctx, call) {
        return Ok(expr);
    }
    // #1679 (Phase 1): const-fold a literal `Function(...)` body into a
    // native function, and fold the `(0, eval)('this')` globalThis idiom.
    // Runs after the `Function('return this')()` fold; before the Phase 0
    // refusal so const-foldable sites compile instead of being classified.
    if let Some(expr) = super::const_fold_fn::try_eval_function_member_call_fold(ctx, call)? {
        return Ok(expr);
    }
    if let Some(expr) = super::const_fold_fn::try_eval_function_call_fold(ctx, call)? {
        return Ok(expr);
    }
    // #1678/#5206: classify `Function(...)` / `eval(...)`. In strict-eval
    // mode bails on the runtime-unknown bucket; by default compiles that
    // bucket to a throw-on-reach value (returned here); otherwise falls
    // through.
    if let Some(expr) = check_eval_function_call(ctx, call)? {
        return Ok(expr);
    }
    if let Some(expr) = try_bare_regexp_call(ctx, call, has_spread)? {
        return Ok(expr);
    }
    // #2874: `Iterator.from(x)` — wrap an iterable in a lazy iterator-helper.
    if let Some(expr) = try_iterator_from(ctx, call, has_spread)? {
        return Ok(expr);
    }

    let mut args = call
        .args
        .iter()
        .map(|arg| lower_expr(ctx, &arg.expr))
        .collect::<Result<Vec<_>>>()?;

    if !has_spread {
        if let ast::Callee::Expr(callee_expr) = &call.callee {
            if let ast::Expr::Ident(ident) = unwrap_call_callee_ts_wrappers(callee_expr.as_ref()) {
                if let Some((module_name, Some(method_name))) =
                    ctx.lookup_native_module(ident.sym.as_ref())
                {
                    if module_name.strip_prefix("node:").unwrap_or(module_name) == "sqlite"
                        && method_name == "DatabaseSync"
                    {
                        return Ok(Expr::NativeMethodCall {
                            module: "sqlite".to_string(),
                            class_name: None,
                            object: None,
                            method: "DatabaseSync".to_string(),
                            args,
                        });
                    }
                }
            }
        }
    }

    // #1723: `<stdlib-ns>[dynamicKey].method(args)` — a method call whose
    // receiver is a dynamic stdlib SUB-namespace selection (`path.win32` /
    // `path.posix`) and whose method is a source-visible static name. The
    // method-call dispatch below lowers the receiver `ns[dynamicKey]` directly
    // — it never routes the `.method` part through `lower_member` — so the #503
    // guard would fire on the receiver. Mark the *next* `ns[dynamicKey]`
    // lowering auditable (same carve-out as the member-access form: the method
    // name is in plaintext, not the `ns[runtimeVar]()` obfuscation). The flag
    // is set AFTER the arguments above are lowered, so a dynamic stdlib key in
    // an ARGUMENT (`ns[dyn].m(fs[evil])`) is still refused; it is consumed by
    // the first guarded access (the receiver) and restored by `lower_call`.
    if let ast::Callee::Expr(callee_expr) = &call.callee {
        let mut callee = callee_expr.as_ref();
        while let ast::Expr::Paren(p) = callee {
            callee = p.expr.as_ref();
        }
        if let ast::Expr::Member(callee_member) = callee {
            if super::expr_member::stdlib_ns_subnamespace_static_access(ctx, callee_member) {
                ctx.suppress_stdlib_dispatch_guard_once = true;
            }
        }
    }

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
    args = match try_array_static_alias_call(ctx, call, args, has_spread) {
        Ok(expr) => return Ok(expr),
        Err(args) => args,
    };
    args = match try_object_has_own_call(call, args, has_spread) {
        Ok(expr) => return Ok(expr),
        Err(args) => args,
    };
    args = match try_direct_has_own_call(ctx, call, args, has_spread) {
        Ok(expr) => return Ok(expr),
        Err(args) => args,
    };
    args = match try_direct_property_is_enumerable_call(ctx, call, args, has_spread) {
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
            // super() call in constructor. With spread args
            // (`super(...arguments)` — tsc's pass-through-ctor emit) the
            // parent ctor is invoked at runtime via the
            // CLASS_CONSTRUCTORS registry with the materialized args
            // array; the flat lowering would pass the spread operand as
            // ONE positional arg (zod's ZodNumber stored the whole
            // `arguments` object into `this._def`).
            if let Some(spread_args) = spread_args {
                return Ok(Expr::SuperCallSpread(spread_args));
            }
            Ok(Expr::SuperCall(args))
        }
        ast::Callee::Expr(expr) => {
            // Check for super.method() call
            if let ast::Expr::SuperProp(super_prop) = expr.as_ref() {
                match &super_prop.prop {
                    ast::SuperProp::Ident(ident) => {
                        if let Some(home_id) = ctx.object_super_home_stack.last().copied() {
                            return Ok(Expr::ObjectSuperMethodCall {
                                home: Box::new(Expr::LocalGet(home_id)),
                                key: Box::new(Expr::String(ident.sym.to_string())),
                                receiver: Box::new(Expr::This),
                                args,
                            });
                        }
                        if let Some(spread_args) = spread_args.clone() {
                            return Ok(Expr::SuperMethodCallSpread {
                                method: ident.sym.to_string(),
                                args: spread_args,
                            });
                        }
                        return Ok(Expr::SuperMethodCall {
                            method: ident.sym.to_string(),
                            args,
                        });
                    }
                    ast::SuperProp::Computed(computed) => {
                        if let Some(home_id) = ctx.object_super_home_stack.last().copied() {
                            return Ok(Expr::ObjectSuperMethodCall {
                                home: Box::new(Expr::LocalGet(home_id)),
                                key: Box::new(lower_expr(ctx, computed.expr.as_ref())?),
                                receiver: Box::new(Expr::This),
                                args,
                            });
                        }
                        // `super['getThis']()` in a CLASS method with a string-
                        // literal key is a super METHOD CALL — route it through
                        // SuperMethodCall (the ident-form path) so it binds the
                        // current `this` as receiver. Without this it fell through
                        // to a generic property-get-then-call that lost the
                        // receiver binding (test262 super/prop-expr-cls-ref-this).
                        if let ast::Expr::Lit(ast::Lit::Str(s)) = computed.expr.as_ref() {
                            if let Some(method) = s.value.as_str() {
                                if let Some(spread_args) = spread_args.clone() {
                                    return Ok(Expr::SuperMethodCallSpread {
                                        method: method.to_string(),
                                        args: spread_args,
                                    });
                                }
                                return Ok(Expr::SuperMethodCall {
                                    method: method.to_string(),
                                    args,
                                });
                            }
                        }
                    }
                }
            }

            // Nested 3-level Member dispatch: process.hrtime.bigint(),
            // crypto.subtle.<method>(), util.types.<method>(), and
            // path.posix/win32.<method>().
            args = match try_process_hrtime_bigint(expr, args) {
                Ok(e) => return Ok(e),
                Err(a) => a,
            };
            args = match try_process_memory_usage_rss(expr, args) {
                Ok(e) => return Ok(e),
                Err(a) => a,
            };
            args = match try_web_crypto_subtle(ctx, expr, args)? {
                Ok(e) => return Ok(e),
                Err(a) => a,
            };
            args = match try_util_types_namespace(ctx, expr, args)? {
                Ok(e) => return Ok(e),
                Err(a) => a,
            };
            args = match try_dns_promises_namespace(ctx, expr, args)? {
                Ok(e) => return Ok(e),
                Err(a) => a,
            };
            args = match try_punycode_ucs2_namespace(ctx, expr, args)? {
                Ok(e) => return Ok(e),
                Err(a) => a,
            };
            args = match try_path_subnamespace(ctx, expr, args) {
                Ok(e) => return Ok(e),
                Err(a) => a,
            };

            // TextEncoder/TextDecoder direct methods need to win before
            // native-instance dispatch, because node:util named constructors
            // can leave native tags on the local binding.
            args = match try_textencoder_decoder(ctx, call, args)? {
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
            // #3896: mark the callee position so the expr_member read-gate
            // keeps rejecting `ns.foo()` (absent member call) while relaxing a
            // bare value read `ns.foo` to undefined for Node-core namespaces.
            let prev_callee = ctx.lowering_call_callee;
            ctx.lowering_call_callee = true;
            let callee_expr = lower_expr(ctx, expr);
            ctx.lowering_call_callee = prev_callee;
            let callee_expr = callee_expr?;

            // Fill in default arguments if callee is a known function
            let mut args = args;
            if let Expr::FuncRef(func_id) = &callee_expr {
                if let Some((defaults, _param_ids, rest_idx, has_synth_args)) =
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
                        let num_provided = args.len();
                        let fill_end = match rest_idx {
                            Some(i) => i,
                            None => defaults.len(),
                        };
                        // Fill missing fixed-parameter slots with `undefined`.
                        // The callee body already starts with default-param
                        // checks, so default expressions must execute there,
                        // in parameter order and inside the function's abrupt
                        // completion boundary. This matters for async
                        // functions: `async function f(x = throws()) {}` must
                        // return a rejected Promise, not throw while building
                        // the call arguments.
                        //
                        // Refs #645 deeper followup / #488 drizzle-sqlite: push
                        // something for EVERY slot from num_provided..defaults.len(),
                        // even when defaults[i] is None — otherwise a later Some
                        // default lands at the wrong positional slot. Drizzle's
                        // tableBase(name, columns, extraConfig, schema?,
                        // baseName=name) is the load-bearing repro: 3-arg
                        // call, slot 3 (schema) has None default and slot 4
                        // (baseName) has Some(name). If slot 3 is skipped,
                        // baseName receives the wrong positional value.
                        for _ in num_provided..fill_end {
                            args.push(Expr::Undefined);
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
                    // #5247: capture the call's source byte offset (same source as
                    // DynamicImport's `byte_offset` below). Codegen resolves it to a
                    // `file:line` for the runtime "X is not a function" TypeError.
                    byte_offset: call.span.lo.0,
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
                byte_offset: call.span.lo.0,
                deferred_error: None,
                synchronous: false,
            })
        }
    }
}
