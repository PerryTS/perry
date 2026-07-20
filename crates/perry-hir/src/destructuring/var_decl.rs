//! Lowering of variable declarations that may carry destructuring patterns.

use super::*;

use super::var_decl_sources::*;

mod alias_tracking;
mod binding_guards;
mod native_fetch;
mod native_new;
mod type_infer;

use alias_tracking::track_decl_aliases;
use binding_guards::apply_binding_guards;
use native_fetch::register_native_fetch_and_streams;
use native_new::register_native_from_new_and_calls;
use type_infer::infer_decl_type;

/// Lower a variable declaration, handling array destructuring patterns.
/// Returns a vector of statements (multiple for destructuring, single for simple bindings).
pub(crate) fn lower_var_decl_with_destructuring(
    ctx: &mut LoweringContext,
    decl: &ast::VarDeclarator,
    mutable: bool,
    is_var_decl: bool,
) -> Result<Vec<Stmt>> {
    let mut result = Vec::new();

    match &decl.name {
        ast::Pat::Ident(ident) => {
            // Simple binding: let x = expr
            let name = ident.id.sym.to_string();

            // Strict-mode early error + native-instance/module shadow
            // tombstones (extracted to `binding_guards`).
            apply_binding_guards(ctx, decl, &name)?;

            // Plain-object tagging + declared/inferred type computation
            // (extracted to `type_infer`).
            let mut ty = infer_decl_type(ctx, decl, ident, &name);

            // Native-instance registration driven by `new`/`await new`/
            // factory-call/method-chain initializers (extracted to
            // `native_new`).
            register_native_from_new_and_calls(ctx, decl, &name);

            // Require-namespace fast path + fetch / Web-Streams / Blob
            // native-instance registration (extracted to `native_fetch`).
            // Returns true when nothing observable is bound (the `require`
            // of a resolvable native module).
            if register_native_fetch_and_streams(ctx, decl, &name, &mut ty) {
                return Ok(result);
            }

            // Issue #461: when the init is an arrow / function expression
            // (`const f = (x) => …` or `const f = function() {}`), pre-define
            // the local BEFORE lowering the init so self-recursive references
            // inside the closure body resolve to `LocalGet(id)` instead of
            // falling through to `lookup_imported_func` and lowering as
            // `ExternFuncRef { name: "f" }` (which then emits a bare unmangled
            // `_f` symbol at link time). Effect's `internal/stream.ts` hits this:
            // `import * as pull from "./stream/pull.js"` (namespace import) +
            // `const pull = (state) => { … pull(...) … }` (local rebinding) —
            // without pre-registration, the inner closure's `pull` reference
            // resolves to the namespace import. Function declarations
            // (`function f() {}`) already have this pre-registration via
            // `lower_decl.rs`'s `Decl::Fn` arm.
            //
            // Gate on function-expr init only: pre-defining for `const x = x + 1`
            // would silently turn a TDZ violation into a self-reference. For
            // closures, the body doesn't execute until call time, so the slot
            // holds the closure value by then.
            // #593: extend the pre-registration to inits that *contain*
            // an Arrow / Fn anywhere in their tree (e.g.
            // `const off = ev.on(() => off())` — Call wrapping Arrow,
            // `const sub = subject.subscribe({ next: () => sub.unsubscribe() })`
            // — Object wrapping Arrow). The closure body is lowered in
            // its own LoweringContext but reuses the parent's `locals`
            // for outer-scope lookups (see `lower_arrow` /
            // `lower_fn_expr`). Without pre-registration, the inner
            // `off` / `sub` reference resolves to GlobalGet(0) and the
            // self-recursive call no-ops at runtime.
            let is_function_expr_init = matches!(
                decl.init.as_deref(),
                Some(ast::Expr::Arrow(_)) | Some(ast::Expr::Fn(_))
            ) || decl
                .init
                .as_deref()
                .map_or(false, ast_expr_contains_function_expr);
            let pre_id = if is_function_expr_init
                && !ctx.pre_registered_module_vars.contains(&name)
                && ctx.lookup_local(&name).is_none()
            {
                Some(ctx.define_local(name.clone(), ty.clone()))
            } else {
                None
            };

            if let Some(init_ast) = decl.init.as_ref() {
                result.extend(predeclare_implicit_assignment_targets(ctx, init_ast));
            }
            let init = decl.init.as_ref().map(|e| lower_expr(ctx, e)).transpose()?;
            if matches!(ty, Type::Any) {
                match &init {
                    Some(Expr::NativeMethodCall { module, method, .. }) => {
                        if module == "stream" && method == "from" {
                            ty = Type::Named("Readable".to_string());
                        }
                    }
                    Some(Expr::NewDynamic { callee, .. }) => {
                        if let Expr::PropertyGet {
                            object, property, ..
                        } = callee.as_ref()
                        {
                            if matches!(object.as_ref(), Expr::NativeModuleRef(module) if module == "net" || module == "node:net")
                                && matches!(property.as_str(), "BlockList" | "SocketAddress")
                            {
                                ty = Type::Named(property.clone());
                            }
                        }
                    }
                    _ => {}
                }
            }
            // #321: a generator function EXPRESSION bound to a name (`const g =
            // function*(){}`) — register the name so `for (x of g())` / `[...g()]`
            // take the iterator-protocol path, matching named `function* g(){}`
            // declarations. (`.next()`-driving already works via the lifted
            // generator transform; this covers the for-of/spread call sites,
            // whose detection in stmt_loops.rs is name-based.)
            if let Some(Expr::Closure {
                is_generator: true,
                is_async,
                ..
            }) = &init
            {
                ctx.generator_func_names.insert(name.clone());
                if *is_async {
                    ctx.async_generator_func_names.insert(name.clone());
                }
            }
            // Annex B B.3.4: a `var <name> = init` whose name shadows a live
            // `catch (<name>)` parameter assigns to the catch binding, so the
            // function-scoped hoisted `var` keeps its pre-catch value. Target the
            // catch parameter (innermost `lookup_local`) instead of reusing the
            // hoisted id. Only a simple `var x = init` (a plain ident declarator
            // with an initializer); a bare `var x;` re-declaration is inert and a
            // destructuring pattern never names the catch param directly.
            let shadows_catch_param = is_var_decl
                && init.is_some()
                && matches!(&decl.name, ast::Pat::Ident(_))
                && ctx
                    .catch_param_scopes
                    .iter()
                    .any(|scope| scope.contains(&name));
            let id = if shadows_catch_param {
                ctx.lookup_local(&name)
                    .unwrap_or_else(|| ctx.define_local(name.clone(), ty.clone()))
            } else if let Some(pid) = pre_id {
                pid
            } else if ctx.scope_depth == 0
                && ctx.inside_block_scope == 0
                && ctx.pre_registered_module_vars.remove(&name)
            {
                ctx.pre_registered_module_var_decls.remove(&name);
                // Reuse pre-registered LocalId from module-level forward-declaration pass.
                // #1758: gated on MODULE scope — a nested local of the same name
                // (`function helper() { const zipWith = ... }` where the module also
                // declares `const zipWith`) must NOT consume the module var's
                // pre-registered id, or it conflates the two: the nested local and
                // the module binding share one id, the real module value lands on a
                // fresh id, and a sibling closure that referenced the module name
                // resolves to the wrong (uninitialised) slot → `value is not a
                // function`. effect's `layer.merge` (refs module `zipWith` at L1191)
                // broke this way because a local `zipWith` (L1180) precedes it.
                let id = ctx.lookup_local(&name).unwrap();
                // Update the type now that we have full inference
                if let Some(existing_ty) = ctx.locals.lookup_type_mut(&name) {
                    *existing_ty = ty.clone();
                }
                id
            } else if let Some(fid) = match &decl.name {
                // #4973: the function-body hoist pass pre-registered this
                // exact `let`/`const` declarator (span-keyed) so hoisted
                // sibling functions could forward-reference it. Reuse the
                // pre-registered id here so the init lands in the slot/box
                // those references captured.
                ast::Pat::Ident(ident) => ctx.lexical_forward_decls.remove(&ident.id.span.lo.0),
                _ => None,
            } {
                if let Some((_, _, existing_ty)) =
                    ctx.locals.iter_mut().rev().find(|(_, lid, _)| *lid == fid)
                {
                    *existing_ty = ty.clone();
                }
                fid
            } else if let Some((reuse_pos, id)) = is_var_decl
                .then(|| {
                    // Issue #838 followup (b): when the closure-body hoist
                    // in `lower_fn_expr` / `lower_arrow` pre-registered this
                    // `var` (so forward references like `var O = function(){
                    // … _ … }; var _ = …;` resolve before `_`'s let runs),
                    // reuse that pre-hoisted id here. Otherwise the let
                    // defines a fresh id and the pre-hoisted slot stays
                    // uninitialised — closures created before the let see
                    // value-zero through the capture box and dispatch
                    // misses entirely. dayjs's outer IIFE hits this with
                    // `var O = function(t){ return new _(n); }; var _ = ((
                    // function(){ function M(){…}; … return M; })());`.
                    //
                    // Restrict this path to syntactic `var`. A block-scoped
                    // `let`/`const` with the same name must create a fresh
                    // lexical binding, and using `lookup_local(name)` here
                    // would accidentally grab a shadowing catch parameter.
                    ctx.locals
                        .iter_named(&name)
                        .find(|(_, (_, lid, _))| ctx.var_hoisted_ids.contains(lid))
                        .map(|(pos, (_, lid, _))| (pos, *lid))
                })
                .flatten()
            {
                // Patch the reused binding's type in place (O(1) by position)
                // rather than re-finding it with an O(n) scan (#5267).
                *ctx.locals.type_mut_at(reuse_pos) = ty.clone();
                id
            } else {
                ctx.define_local(name.clone(), ty.clone())
            };
            if !mutable {
                ctx.mark_local_immutable(id);
            }
            // Alias / prototype / static-method tracking for the freshly-
            // bound identifier (extracted to `alias_tracking`).
            track_decl_aliases(ctx, decl, &name, id, &init);
            // `with (o) { var foo = v; }` — the binding `foo` is hoisted to
            // the enclosing var scope, but the *initialisation* is a normal
            // PutValue under the with environment: when `o` has a `foo`
            // property, the write goes to `o.foo`, not the hoisted local
            // (test262 with/12.10-0-8). Emit the hoisted Let (no init) plus
            // a WithSet for the assignment.
            if is_var_decl && init.is_some() {
                if let Some(env_id) = ctx.active_with_envs_for_ident(&name).into_iter().next() {
                    result.push(Stmt::Let {
                        id,
                        name: name.clone(),
                        ty,
                        mutable,
                        init: None,
                    });
                    let fallback = crate::lower::with_set_fallback_for_ident(ctx, &name);
                    result.push(Stmt::Expr(Expr::WithSet {
                        object: Box::new(Expr::LocalGet(env_id)),
                        property: name,
                        value: Box::new(init.unwrap()),
                        fallback,
                        strict: ctx.current_strict,
                    }));
                    return Ok(result);
                }
            }
            // Next.js / webpack require pattern: `var i = n[e] = {exports:{}}`.
            // A chained member-assignment whose RHS is an object literal
            // miscompiles in the full-bundle context: the constructed object's
            // own field reads back as 0 when the construction flows directly
            // into both the member store and the binding (the nested webpack
            // bundle's `exports` then reads 0 → `exports.Fragment = …` throws).
            // A directly-bound object literal (`var x = {exports:{}}`) is fine,
            // so hoist the construction to its own `Let` and feed the member-set
            // and the binding from that temp — mirroring the working form.
            let init = match init {
                Some(Expr::PutValueSet {
                    target,
                    key,
                    value,
                    receiver,
                    strict,
                }) if matches!(value.as_ref(), Expr::New { .. } | Expr::Object(_)) => {
                    let tmp_id = ctx.define_local("__nx_member_init".to_string(), Type::Any);
                    result.push(Stmt::Let {
                        id: tmp_id,
                        name: "__nx_member_init".to_string(),
                        ty: Type::Any,
                        mutable: false,
                        init: Some(*value),
                    });
                    result.push(Stmt::Expr(Expr::PutValueSet {
                        target,
                        key,
                        value: Box::new(Expr::LocalGet(tmp_id)),
                        receiver,
                        strict,
                    }));
                    result.push(Stmt::Let {
                        id,
                        name,
                        ty,
                        mutable,
                        init: Some(Expr::LocalGet(tmp_id)),
                    });
                    return Ok(result);
                }
                other => other,
            };
            result.push(Stmt::Let {
                id,
                name,
                ty,
                mutable,
                init,
            });
        }
        ast::Pat::Array(_) | ast::Pat::Object(_) => {
            // #3663 / #4905: tag destructured builtin-module members
            // (stream ctors, net factories) as native-module aliases so
            // call sites route through the static native table. Bindings
            // returned in `skip_local_bindings` must not also bind a
            // runtime local — the local (undefined for `net.connect`)
            // would shadow the alias at call sites; ESM named imports
            // never create one (exact parity).
            let skip_local_bindings = register_destructured_stream_ctors(ctx, decl);
            let filtered_pat;
            let pattern: &ast::Pat = if skip_local_bindings.is_empty() {
                &decl.name
            } else if let ast::Pat::Object(obj) = &decl.name {
                let mut obj = obj.clone();
                obj.props.retain(|prop| match prop {
                    ast::ObjectPatProp::Assign(a) => {
                        !skip_local_bindings.contains(&a.key.sym.to_string())
                    }
                    ast::ObjectPatProp::KeyValue(kv) => match kv.value.as_ref() {
                        ast::Pat::Ident(b) => !skip_local_bindings.contains(&b.id.sym.to_string()),
                        _ => true,
                    },
                    _ => true,
                });
                if obj.props.is_empty() {
                    // Every binding became a native alias; nothing left to
                    // bind at runtime (require of a builtin module has no
                    // observable side effects).
                    return Ok(result);
                }
                filtered_pat = ast::Pat::Object(obj);
                &filtered_pat
            } else {
                &decl.name
            };

            // Delegate to the recursive pattern binding helper so that all
            // destructuring features (nested patterns, defaults, rest, computed
            // keys) work consistently across all call sites.

            // ink-shape useState: `const [v, setV] = useState(0)` (#679 Phase 1).
            // Rewrite RHS to call useStateTuple which returns a real
            // [value, setter_closure] 2-element array. Without this, the
            // regular destructure path indexes a scalar return as if it were
            // an array — both elements come out undefined.
            let init_expr =
                if let (ast::Pat::Array(_), Some(init)) = (&decl.name, decl.init.as_ref()) {
                    if let Some(rewritten) = rewrite_use_state_tuple(ctx, init) {
                        rewritten
                    } else {
                        lower_expr(ctx, init)?
                    }
                } else {
                    decl.init
                        .as_ref()
                        .map(|e| lower_expr(ctx, e))
                        .transpose()?
                        .ok_or_else(|| anyhow!("Destructuring requires an initializer"))?
                };
            let stmts = lower_pattern_binding(ctx, pattern, init_expr, mutable, is_var_decl)?;
            result.extend(stmts);
        }
        _ => {
            // For other patterns, fall back to existing behavior
            let name = get_binding_name(&decl.name)?;
            let ty = extract_binding_type(&decl.name);
            if let Some(init_ast) = decl.init.as_ref() {
                result.extend(predeclare_implicit_assignment_targets(ctx, init_ast));
            }
            let init = decl.init.as_ref().map(|e| lower_expr(ctx, e)).transpose()?;
            // #321: a generator function EXPRESSION bound to a name (`const g =
            // function*(){}`) — register the name so `for (x of g())` / `[...g()]`
            // take the iterator-protocol path, matching named `function* g(){}`
            // declarations. (`.next()`-driving works regardless via the lifted
            // generator transform; this covers the for-of/spread call sites,
            // whose detection in stmt_loops.rs is name-based.)
            if let Some(Expr::Closure {
                is_generator: true,
                is_async,
                ..
            }) = &init
            {
                ctx.generator_func_names.insert(name.clone());
                if *is_async {
                    ctx.async_generator_func_names.insert(name.clone());
                }
            }
            let id = if ctx.scope_depth == 0
                && ctx.inside_block_scope == 0
                && ctx.pre_registered_module_vars.remove(&name)
            {
                ctx.pre_registered_module_var_decls.remove(&name);
                // #1758: module-scope only — see the sibling guard above.
                let id = ctx.lookup_local(&name).unwrap();
                if let Some(existing_ty) = ctx.locals.lookup_type_mut(&name) {
                    *existing_ty = ty.clone();
                }
                id
            } else {
                ctx.define_local(name.clone(), ty.clone())
            };
            if !mutable {
                ctx.mark_local_immutable(id);
            }
            result.push(Stmt::Let {
                id,
                name,
                ty,
                mutable,
                init,
            });
        }
    }

    Ok(result)
}
