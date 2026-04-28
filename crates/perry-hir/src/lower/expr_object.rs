//! Object literal lowering: `ast::Expr::Object`.
//!
//! Tier 2.3 follow-up (v0.5.338) — extracts the 477-LOC `Object` arm
//! from `lower_expr` into a focused module. This is the largest single
//! arm extraction so far. The lowered shape depends on whether the
//! literal is a "closed shape" (no spreads, all fixed string keys) —
//! such literals lower to `new __AnonShape_N()` so downstream property
//! access hits the codegen direct-GEP fast path. Open-shape literals
//! (spreads, computed keys, getters/setters) fall through to a generic
//! `Object` / `ObjectSpread` HIR node that the runtime resolves
//! dynamically.
//!
//! Pattern matches `expr_misc.rs` and `expr_function.rs`: free
//! `pub(super) fn` helpers, recursion through `super::lower_expr`.

use anyhow::Result;
use perry_types::{LocalId, Type};
use swc_ecma_ast as ast;

use crate::analysis::{closure_uses_this, collect_assigned_locals_stmt, collect_local_refs_stmt};
use crate::ir::{EnumValue, Expr, Function, Param, Stmt};
use crate::lower_decl::lower_block_stmt;
use crate::lower_patterns::{
    get_param_default, get_pat_name, is_rest_param,
};
use crate::lower_types::{extract_param_type_with_ctx, extract_ts_type_with_ctx};

use super::{lower_expr, LoweringContext};

pub(super) fn lower_object(ctx: &mut LoweringContext, obj: &ast::ObjectLit) -> Result<Expr> {
            // Phase 3: closed-shape object literals lower to `new __AnonShape_N()`
            // so downstream field access hits the direct-GEP fast path. The
            // anon class is synthesized with `init: Some(value_expr)` on each
            // field, and `apply_field_initializers_recursive` at codegen time
            // emits `PropertySet { this, field, init }` — PropertySet's
            // direct-GEP arm at `crates/perry-codegen/src/expr.rs:2277-2293`
            // fires because `this` resolves to the anon class via class_stack.
            //
            // Runtime parity for Object.* introspection APIs on anon-shape
            // classes is handled runtime-side in perry-runtime's object module
            // — see that crate's handling of `class_id`-tagged objects on
            // getOwnPropertyDescriptor / Object.keys / JSON.stringify / etc.
            fn is_closed_shape(obj: &ast::ObjectLit) -> bool {
                if obj.props.is_empty() { return false; }
                for p in &obj.props {
                    match p {
                        ast::PropOrSpread::Spread(_) => return false,
                        ast::PropOrSpread::Prop(prop) => match prop.as_ref() {
                            ast::Prop::KeyValue(kv) => match &kv.key {
                                ast::PropName::Ident(_)
                                | ast::PropName::Str(_)
                                | ast::PropName::Num(_) => {}
                                _ => return false,
                            },
                            ast::Prop::Shorthand(_) => {}
                            _ => return false,
                        },
                    }
                }
                true
            }
            if is_closed_shape(obj) {
                let mut fields: Vec<(String, Type, Expr)> = Vec::new();
                let mut bail = false;
                let mut seen = std::collections::HashSet::new();
                for prop in &obj.props {
                    let ast::PropOrSpread::Prop(p) = prop else { unreachable!() };
                    match p.as_ref() {
                        ast::Prop::KeyValue(kv) => {
                            let key = match &kv.key {
                                ast::PropName::Ident(ident) => ident.sym.to_string(),
                                ast::PropName::Str(s) => s.value.as_str().unwrap_or("").to_string(),
                                ast::PropName::Num(n) => n.value.to_string(),
                                _ => unreachable!(),
                            };
                            if !seen.insert(key.clone()) { bail = true; break; }
                            let ty = crate::lower_types::infer_type_from_expr(&kv.value, ctx);
                            let value = lower_expr(ctx, &kv.value)?;
                            fields.push((key, ty, value));
                        }
                        ast::Prop::Shorthand(ident) => {
                            let name = ident.sym.to_string();
                            if !seen.insert(name.clone()) { bail = true; break; }
                            let (value, ty) = if let Some(func_id) = ctx.lookup_func(&name) {
                                (Expr::FuncRef(func_id), Type::Any)
                            } else if let Some(local_id) = ctx.lookup_local(&name) {
                                let ty = ctx.lookup_local_type(&name).cloned().unwrap_or(Type::Any);
                                (Expr::LocalGet(local_id), ty)
                            } else if ctx.lookup_class(&name).is_some() {
                                (Expr::ClassRef(name.clone()), Type::Any)
                            } else {
                                bail = true; break;
                            };
                            fields.push((name, ty, value));
                        }
                        _ => unreachable!(),
                    }
                }
                if !bail {
                    // Split (name, ty, value) into parallel vecs before the
                    // synthesize call consumes ownership of the shape.
                    let args: Vec<Expr> = fields.iter().map(|(_, _, v)| v.clone()).collect();
                    let class_name = ctx.synthesize_anon_shape_class(&fields);
                    return Ok(Expr::New {
                        class_name,
                        args,
                        type_args: Vec::new(),
                    });
                }
            }
            // Legacy path — spread, methods/getters/setters, computed keys,
            // dup keys, or unresolvable shorthand.
            //
            // Check if any spread elements exist; if so, use ObjectSpread
            let has_spread = obj.props.iter().any(|p| matches!(p, ast::PropOrSpread::Spread(_)));
            if has_spread {
                let mut parts: Vec<(Option<String>, Expr)> = Vec::new();
                for prop in &obj.props {
                    match prop {
                        ast::PropOrSpread::Spread(spread) => {
                            let spread_expr = lower_expr(ctx, &spread.expr)?;
                            parts.push((None, spread_expr));
                        }
                        ast::PropOrSpread::Prop(prop) => {
                            match prop.as_ref() {
                                ast::Prop::KeyValue(kv) => {
                                    let key = match &kv.key {
                                        ast::PropName::Ident(ident) => ident.sym.to_string(),
                                        ast::PropName::Str(s) => s.value.as_str().unwrap_or("").to_string(),
                                        ast::PropName::Num(n) => n.value.to_string(),
                                        _ => continue,
                                    };
                                    let value = lower_expr(ctx, &kv.value)?;
                                    parts.push((Some(key), value));
                                }
                                ast::Prop::Shorthand(ident) => {
                                    let name = ident.sym.to_string();
                                    let value = if let Some(func_id) = ctx.lookup_func(&name) {
                                        Expr::FuncRef(func_id)
                                    } else if let Some(local_id) = ctx.lookup_local(&name) {
                                        Expr::LocalGet(local_id)
                                    } else if ctx.lookup_class(&name).is_some() {
                                        Expr::ClassRef(name.clone())
                                    } else {
                                        continue;
                                    };
                                    parts.push((Some(name), value));
                                }
                                _ => {}
                            }
                        }
                    }
                }
                return Ok(Expr::ObjectSpread { parts });
            }
            let mut props = Vec::new();
            // Computed keys whose value can't be folded to a string at HIR time
            // (typically symbol-typed locals like `{ [symProp]: 42 }`). Deferred
            // and emitted as statements inside an IIFE wrapper after the
            // static-key Object literal is built.
            //
            // For `Prop::Method` with a computed key whose body uses `this`
            // (e.g. `{ [Symbol.toPrimitive](hint) { return this.value; } }`),
            // we emit a dedicated `js_object_set_symbol_method` runtime call
            // that BOTH stores the closure in the symbol side-table AND
            // patches the closure's reserved `this` slot with the object.
            enum PostInit {
                SetValue { key: Expr, value: Expr },
                SetMethodWithThis { key: Expr, closure: Expr },
            }
            let mut computed_post_init: Vec<PostInit> = Vec::new();
            for prop in &obj.props {
                match prop {
                    ast::PropOrSpread::Prop(prop) => {
                        match prop.as_ref() {
                            ast::Prop::KeyValue(kv) => {
                                enum KeyResolution {
                                    Static(String),
                                    Dynamic(Expr),
                                    Skip,
                                }
                                let key_resolution: KeyResolution = match &kv.key {
                                    ast::PropName::Ident(ident) => KeyResolution::Static(ident.sym.to_string()),
                                    ast::PropName::Str(s) => KeyResolution::Static(s.value.as_str().unwrap_or("").to_string()),
                                    ast::PropName::Num(n) => KeyResolution::Static(n.value.to_string()),
                                    ast::PropName::Computed(computed) => {
                                        // Handle computed property keys like [ChainName.ETHEREUM]
                                        // Try to resolve enum member access to string keys first.
                                        match computed.expr.as_ref() {
                                            ast::Expr::Member(member) => {
                                                if let (ast::Expr::Ident(obj), ast::MemberProp::Ident(prop)) = (member.obj.as_ref(), &member.prop) {
                                                    let enum_name = obj.sym.to_string();
                                                    let member_name = prop.sym.to_string();
                                                    if let Some(value) = ctx.lookup_enum_member(&enum_name, &member_name) {
                                                        match value {
                                                            EnumValue::String(s) => KeyResolution::Static(s.clone()),
                                                            EnumValue::Number(n) => KeyResolution::Static(n.to_string()),
                                                        }
                                                    } else {
                                                        // Non-enum member access: lower as a dynamic expression.
                                                        match lower_expr(ctx, computed.expr.as_ref()) {
                                                            Ok(e) => KeyResolution::Dynamic(e),
                                                            Err(_) => KeyResolution::Skip,
                                                        }
                                                    }
                                                } else {
                                                    match lower_expr(ctx, computed.expr.as_ref()) {
                                                        Ok(e) => KeyResolution::Dynamic(e),
                                                        Err(_) => KeyResolution::Skip,
                                                    }
                                                }
                                            }
                                            ast::Expr::Lit(ast::Lit::Str(s)) => KeyResolution::Static(s.value.as_str().unwrap_or("").to_string()),
                                            ast::Expr::Lit(ast::Lit::Num(n)) => KeyResolution::Static(n.value.to_string()),
                                            // Identifier or any other expression — lower it
                                            // and defer to post-init IndexSet so symbol-typed
                                            // locals like `[symProp]` flow through the
                                            // IndexSet symbol dispatch path.
                                            _ => match lower_expr(ctx, computed.expr.as_ref()) {
                                                Ok(e) => KeyResolution::Dynamic(e),
                                                Err(_) => KeyResolution::Skip,
                                            },
                                        }
                                    }
                                    _ => KeyResolution::Skip,
                                };
                                match key_resolution {
                                    KeyResolution::Skip => continue,
                                    KeyResolution::Static(key) => {
                                        let value = lower_expr(ctx, &kv.value)?;
                                        props.push((key, value));
                                    }
                                    KeyResolution::Dynamic(key_expr) => {
                                        let value = lower_expr(ctx, &kv.value)?;
                                        computed_post_init.push(PostInit::SetValue { key: key_expr, value });
                                    }
                                }
                            }
                            ast::Prop::Shorthand(ident) => {
                                // Shorthand property: { help } → { help: help }
                                let name = ident.sym.to_string();
                                let value = if let Some(func_id) = ctx.lookup_func(&name) {
                                    Expr::FuncRef(func_id)
                                } else if let Some(local_id) = ctx.lookup_local(&name) {
                                    Expr::LocalGet(local_id)
                                } else if ctx.lookup_class(&name).is_some() {
                                    Expr::ClassRef(name.clone())
                                } else {
                                    continue;
                                };
                                props.push((name, value));
                            }
                            ast::Prop::Method(method) => {
                                // Inline method: { help(): string { ... } }
                                // Computed keys (e.g. `[Symbol.toPrimitive](hint) {}`)
                                // get routed through the IIFE wrapper's
                                // SetMethodWithThis post-init, which emits a
                                // `js_object_set_symbol_method` call that also
                                // patches the closure's reserved `this` slot.
                                enum MethodKey {
                                    Static(String),
                                    Computed(Expr),
                                }
                                let method_key = match &method.key {
                                    ast::PropName::Ident(ident) => {
                                        MethodKey::Static(ident.sym.to_string())
                                    }
                                    ast::PropName::Str(s) => {
                                        MethodKey::Static(s.value.as_str().unwrap_or("").to_string())
                                    }
                                    ast::PropName::Computed(computed) => {
                                        match lower_expr(ctx, computed.expr.as_ref()) {
                                            Ok(e) => MethodKey::Computed(e),
                                            Err(_) => continue,
                                        }
                                    }
                                    _ => continue,
                                };
                                let key_label: String = match &method_key {
                                    MethodKey::Static(s) => s.clone(),
                                    MethodKey::Computed(_) => format!("computed_{}", ctx.next_func_id),
                                };
                                let key: String = key_label.clone();
                                let func_id = ctx.fresh_func();
                                // Use a unique synthetic name to avoid collisions
                                let func_name = format!("__obj_method_{}_{}", key, func_id);

                                // Snapshot outer locals for capture analysis
                                let outer_locals: Vec<(String, LocalId)> = ctx.locals.iter()
                                    .map(|(name, id, _)| (name.clone(), *id))
                                    .collect();

                                let scope_mark = ctx.enter_scope();
                                let mut params = Vec::new();
                                for param in method.function.params.iter() {
                                    let param_name = get_pat_name(&param.pat)?;
                                    let param_type = extract_param_type_with_ctx(&param.pat, Some(ctx));
                                    let param_default = get_param_default(ctx, &param.pat)?;
                                    let param_id = ctx.define_local(param_name.clone(), param_type.clone());
                                    params.push(Param {
                                        id: param_id,
                                        name: param_name,
                                        ty: param_type,
                                        default: param_default,
                                        is_rest: is_rest_param(&param.pat),
                                    });
                                }
                                let return_type = method.function.return_type.as_ref()
                                    .map(|rt| extract_ts_type_with_ctx(&rt.type_ann, Some(ctx)))
                                    .unwrap_or(Type::Any);
                                let body = if let Some(ref block) = method.function.body {
                                    lower_block_stmt(ctx, block)?
                                } else {
                                    Vec::new()
                                };
                                ctx.exit_scope(scope_mark);

                                // Capture analysis (same pattern as arrow/function expressions)
                                let mut all_refs = Vec::new();
                                let mut visited_closures = std::collections::HashSet::new();
                                for stmt in &body {
                                    collect_local_refs_stmt(stmt, &mut all_refs, &mut visited_closures);
                                }
                                let outer_local_ids: std::collections::HashSet<LocalId> = outer_locals.iter()
                                    .map(|(_, id)| *id)
                                    .collect();
                                let method_param_ids: std::collections::HashSet<LocalId> = params.iter()
                                    .map(|p| p.id)
                                    .collect();
                                let mut captures: Vec<LocalId> = all_refs.into_iter()
                                    .filter(|id| outer_local_ids.contains(id) && !method_param_ids.contains(id))
                                    .collect();
                                captures.sort();
                                captures.dedup();
                                captures = ctx.filter_module_level_captures(captures);

                                // Check if the method body uses `this` — even with no
                                // outer-scope captures we must emit a Closure so the
                                // object-literal creation code can patch capture slot 0
                                // with the object pointer.
                                let uses_this = closure_uses_this(&body);

                                let value_expr: Expr = if captures.is_empty() && !uses_this {
                                    // No captures and no `this`: keep as standalone Function + FuncRef
                                    ctx.register_func(func_name.clone(), func_id);
                                    let defaults: Vec<Option<Expr>> = params.iter().map(|p| p.default.clone()).collect();
                                    let param_ids: Vec<LocalId> = params.iter().map(|p| p.id).collect();
                                    ctx.func_defaults.push((func_id, defaults, param_ids));
                                    ctx.pending_functions.push(Function {
                                        id: func_id,
                                        name: func_name,
                                        type_params: Vec::new(),
                                        params,
                                        return_type,
                                        body,
                                        is_async: method.function.is_async,
                                        is_generator: false,
                                        was_plain_async: false,
                                        is_exported: false,
                                        captures: Vec::new(),
                                        decorators: Vec::new(),
                                    });
                                    Expr::FuncRef(func_id)
                                } else {
                                    // Has captures: emit as Closure
                                    let mut all_assigned = Vec::new();
                                    for stmt in &body {
                                        collect_assigned_locals_stmt(stmt, &mut all_assigned);
                                    }
                                    let assigned_set: std::collections::HashSet<LocalId> = all_assigned.into_iter().collect();
                                    let mutable_captures: Vec<LocalId> = captures.iter()
                                        .filter(|id| assigned_set.contains(id) || ctx.var_hoisted_ids.contains(id))
                                        .copied()
                                        .collect();
                                    let captures_this = uses_this;
                                    let enclosing_class = if captures_this {
                                        ctx.current_class.clone()
                                    } else {
                                        None
                                    };
                                    Expr::Closure {
                                        func_id,
                                        params,
                                        return_type,
                                        body,
                                        captures,
                                        mutable_captures,
                                        captures_this,
                                        enclosing_class,
                                        is_async: method.function.is_async,
                                    }
                                };
                                match method_key {
                                    MethodKey::Static(key_str) => {
                                        props.push((key_str, value_expr));
                                    }
                                    MethodKey::Computed(key_expr) => {
                                        if uses_this {
                                            computed_post_init.push(PostInit::SetMethodWithThis {
                                                key: key_expr,
                                                closure: value_expr,
                                            });
                                        } else {
                                            computed_post_init.push(PostInit::SetValue {
                                                key: key_expr,
                                                value: value_expr,
                                            });
                                        }
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                    _ => {}
                }
            }
            // No computed-key post-init: emit a plain object literal.
            if computed_post_init.is_empty() {
                return Ok(Expr::Object(props));
            }
            // Has computed keys: synthesize an IIFE wrapper that builds the
            // object with static props, then runs IndexSet for each computed
            // key, then returns the object. The IndexSet branch in the LLVM
            // backend already runtime-dispatches to
            // `js_object_set_symbol_property` when the key is a symbol — so
            // `{ [symProp]: 42, x: 1 }` flows through the symbol side-table
            // automatically.
            //
            // Lowered shape:
            //   ((__o) => {
            //       __o[k1] = v1;
            //       __o[k2] = v2;
            //       return __o;
            //   })({ static_props })
            let iife_func_id = ctx.fresh_func();
            let scope_mark = ctx.enter_scope();
            let param_id = ctx.define_local("__perry_obj_iife".to_string(), Type::Any);
            let param = Param {
                id: param_id,
                name: "__perry_obj_iife".to_string(),
                ty: Type::Any,
                default: None,
                is_rest: false,
            };
            let mut body: Vec<Stmt> = Vec::with_capacity(computed_post_init.len() + 1);
            for init in computed_post_init {
                match init {
                    PostInit::SetValue { key, value } => {
                        body.push(Stmt::Expr(Expr::IndexSet {
                            object: Box::new(Expr::LocalGet(param_id)),
                            index: Box::new(key),
                            value: Box::new(value),
                        }));
                    }
                    PostInit::SetMethodWithThis { key, closure } => {
                        // Emit a direct call to the runtime helper that
                        // stores the closure in the symbol side-table AND
                        // patches its reserved `this` slot with __o.
                        body.push(Stmt::Expr(Expr::Call {
                            callee: Box::new(Expr::ExternFuncRef {
                                name: "js_object_set_symbol_method".to_string(),
                                param_types: Vec::new(),
                                return_type: Type::Any,
                            }),
                            args: vec![
                                Expr::LocalGet(param_id),
                                key,
                                closure,
                            ],
                            type_args: Vec::new(),
                        }));
                    }
                }
            }
            body.push(Stmt::Return(Some(Expr::LocalGet(param_id))));
            ctx.exit_scope(scope_mark);
            // Capture analysis: any LocalIds referenced inside the body that
            // weren't defined here (i.e. the symbol locals from the outer scope).
            let mut all_refs = Vec::new();
            let mut visited_closures = std::collections::HashSet::new();
            for stmt in &body {
                collect_local_refs_stmt(stmt, &mut all_refs, &mut visited_closures);
            }
            let mut captures: Vec<LocalId> = all_refs
                .into_iter()
                .filter(|id| *id != param_id)
                .collect();
            captures.sort();
            captures.dedup();
            captures = ctx.filter_module_level_captures(captures);
            let static_obj = Expr::Object(props);
            let closure = Expr::Closure {
                func_id: iife_func_id,
                params: vec![param],
                return_type: Type::Any,
                body,
                captures,
                mutable_captures: Vec::new(),
                captures_this: false,
                enclosing_class: None,
                is_async: false,
            };
            Ok(Expr::Call {
                callee: Box::new(closure),
                args: vec![static_obj],
                type_args: vec![],
            })
}
