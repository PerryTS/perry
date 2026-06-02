//! Assignment expression lowering: `ast::Expr::Assign`.
//!
//! Tier 2.3 round 3 (v0.5.339) — extracts the 312-LOC `Assign` arm
//! from `lower_expr`. Covers `x = v`, `x += v` (and other compound
//! assigns), `obj.prop = v`, `obj[k] = v`, plus destructuring assigns
//! `[a, b] = arr` and `{a, b} = obj` (these last two desugar to a
//! sequence expression of individual assignments).

use anyhow::{anyhow, Result};
use perry_types::Type;
use swc_ecma_ast as ast;

use crate::destructuring::lower_destructuring_assignment;
use crate::ir::{BinaryOp, Expr, LogicalOp};
use crate::lower_patterns::lower_assign_target_to_expr;

use super::{lower_expr, lower_expr_assignment, LoweringContext};

fn throw_type_error_const_assignment(name: &str) -> Expr {
    Expr::Call {
        callee: Box::new(Expr::ExternFuncRef {
            name: "js_throw_type_error_const_assignment".to_string(),
            param_types: vec![Type::String],
            return_type: Type::Any,
        }),
        args: vec![Expr::String(name.to_string())],
        type_args: vec![],
    }
}

fn throw_reference_error_unresolvable_assignment(name: &str) -> Expr {
    Expr::Call {
        callee: Box::new(Expr::ExternFuncRef {
            name: "js_throw_reference_error_unresolvable_assignment".to_string(),
            param_types: vec![Type::String],
            return_type: Type::Any,
        }),
        args: vec![Expr::String(name.to_string())],
        type_args: vec![],
    }
}

pub(super) fn lower_assign(ctx: &mut LoweringContext, assign: &ast::AssignExpr) -> Result<Expr> {
    // Detect assignments from native module calls and register for cross-function tracking.
    // e.g., `mongoClient = await MongoClient.connect(uri)` registers mongoClient as a mongodb instance.
    if assign.op == ast::AssignOp::Assign {
        if let ast::AssignTarget::Simple(ast::SimpleAssignTarget::Ident(target_ident)) =
            &assign.left
        {
            let var_name = target_ident.id.sym.to_string();
            // Unwrap await if present
            let inner_rhs = if let ast::Expr::Await(await_expr) = assign.right.as_ref() {
                await_expr.arg.as_ref()
            } else {
                assign.right.as_ref()
            };
            // Check for NativeModule.method() call (e.g., MongoClient.connect(uri))
            if let ast::Expr::Call(call_expr) = inner_rhs {
                if let ast::Callee::Expr(callee) = &call_expr.callee {
                    if let ast::Expr::Member(member) = callee.as_ref() {
                        if let ast::Expr::Ident(obj_ident) = member.obj.as_ref() {
                            let obj_name = obj_ident.sym.as_ref();
                            if let Some((module_name, _)) = ctx.lookup_native_module(obj_name) {
                                if let ast::MemberProp::Ident(method_ident) = &member.prop {
                                    let class_name = match (module_name, method_ident.sym.as_ref())
                                    {
                                        ("mongodb", "connect") => Some("MongoClient"),
                                        ("pg", "connect") => Some("Client"),
                                        ("readline", "createInterface") => Some("Interface"),
                                        _ => Some("Instance"),
                                    };
                                    if let Some(class_name) = class_name {
                                        ctx.module_native_instances.push((
                                            var_name.clone(),
                                            module_name.to_string(),
                                            class_name.to_string(),
                                        ));
                                    }
                                }
                            }
                        }
                    }
                }
            }
            // Check for `new NativeClass(...)` assignment: `instance = new Database('mango.db')`
            if let ast::Expr::New(new_expr) = inner_rhs {
                if let ast::Expr::Ident(class_ident) = new_expr.callee.as_ref() {
                    let class_name_str = class_ident.sym.as_ref();
                    let native_info = ctx
                        .lookup_native_module(class_name_str)
                        .map(|(m, _)| m.to_string());
                    if let Some(module_name) = native_info {
                        ctx.register_native_instance(
                            var_name.clone(),
                            module_name.clone(),
                            class_name_str.to_string(),
                        );
                        ctx.module_native_instances.push((
                            var_name.clone(),
                            module_name,
                            class_name_str.to_string(),
                        ));
                    }
                }
            }
            // Check for variable-to-variable assignment: `x = y` where y is a known native instance.
            // e.g., `mongoClient = client` where client was tracked from MongoClient.connect().
            if let ast::Expr::Ident(rhs_ident) = inner_rhs {
                let rhs_name = rhs_ident.sym.as_ref();
                if let Some((module, class)) = ctx.lookup_native_instance(rhs_name) {
                    ctx.module_native_instances.push((
                        var_name,
                        module.to_string(),
                        class.to_string(),
                    ));
                }
            }
        }
    }

    let rhs = lower_expr(ctx, &assign.right)?;

    // Handle compound assignment operators (+=, -=, *=, /=, etc.)
    let value = match assign.op {
        ast::AssignOp::Assign => Box::new(rhs),
        ast::AssignOp::AddAssign => {
            // a += b becomes a = a + b
            let left = Box::new(lower_assign_target_to_expr(ctx, &assign.left)?);
            Box::new(Expr::Binary {
                op: BinaryOp::Add,
                left,
                right: Box::new(rhs),
            })
        }
        ast::AssignOp::SubAssign => {
            let left = Box::new(lower_assign_target_to_expr(ctx, &assign.left)?);
            Box::new(Expr::Binary {
                op: BinaryOp::Sub,
                left,
                right: Box::new(rhs),
            })
        }
        ast::AssignOp::MulAssign => {
            let left = Box::new(lower_assign_target_to_expr(ctx, &assign.left)?);
            Box::new(Expr::Binary {
                op: BinaryOp::Mul,
                left,
                right: Box::new(rhs),
            })
        }
        ast::AssignOp::DivAssign => {
            let left = Box::new(lower_assign_target_to_expr(ctx, &assign.left)?);
            Box::new(Expr::Binary {
                op: BinaryOp::Div,
                left,
                right: Box::new(rhs),
            })
        }
        ast::AssignOp::ModAssign => {
            let left = Box::new(lower_assign_target_to_expr(ctx, &assign.left)?);
            Box::new(Expr::Binary {
                op: BinaryOp::Mod,
                left,
                right: Box::new(rhs),
            })
        }
        ast::AssignOp::BitAndAssign => {
            let left = Box::new(lower_assign_target_to_expr(ctx, &assign.left)?);
            Box::new(Expr::Binary {
                op: BinaryOp::BitAnd,
                left,
                right: Box::new(rhs),
            })
        }
        ast::AssignOp::BitOrAssign => {
            let left = Box::new(lower_assign_target_to_expr(ctx, &assign.left)?);
            Box::new(Expr::Binary {
                op: BinaryOp::BitOr,
                left,
                right: Box::new(rhs),
            })
        }
        ast::AssignOp::BitXorAssign => {
            let left = Box::new(lower_assign_target_to_expr(ctx, &assign.left)?);
            Box::new(Expr::Binary {
                op: BinaryOp::BitXor,
                left,
                right: Box::new(rhs),
            })
        }
        ast::AssignOp::LShiftAssign => {
            let left = Box::new(lower_assign_target_to_expr(ctx, &assign.left)?);
            Box::new(Expr::Binary {
                op: BinaryOp::Shl,
                left,
                right: Box::new(rhs),
            })
        }
        ast::AssignOp::RShiftAssign => {
            let left = Box::new(lower_assign_target_to_expr(ctx, &assign.left)?);
            Box::new(Expr::Binary {
                op: BinaryOp::Shr,
                left,
                right: Box::new(rhs),
            })
        }
        ast::AssignOp::ZeroFillRShiftAssign => {
            let left = Box::new(lower_assign_target_to_expr(ctx, &assign.left)?);
            Box::new(Expr::Binary {
                op: BinaryOp::UShr,
                left,
                right: Box::new(rhs),
            })
        }
        ast::AssignOp::ExpAssign => {
            // a **= b becomes a = a ** b
            let left = Box::new(lower_assign_target_to_expr(ctx, &assign.left)?);
            Box::new(Expr::Binary {
                op: BinaryOp::Pow,
                left,
                right: Box::new(rhs),
            })
        }
        ast::AssignOp::AndAssign => {
            // a &&= b becomes a = a && b (short-circuit: only evaluates b if a is truthy)
            let left = Box::new(lower_assign_target_to_expr(ctx, &assign.left)?);
            Box::new(Expr::Logical {
                op: LogicalOp::And,
                left,
                right: Box::new(rhs),
            })
        }
        ast::AssignOp::OrAssign => {
            // a ||= b becomes a = a || b (short-circuit: only evaluates b if a is falsy)
            let left = Box::new(lower_assign_target_to_expr(ctx, &assign.left)?);
            Box::new(Expr::Logical {
                op: LogicalOp::Or,
                left,
                right: Box::new(rhs),
            })
        }
        ast::AssignOp::NullishAssign => {
            // a ??= b becomes a = a ?? b (short-circuit: only evaluates b if a is null/undefined)
            let left = Box::new(lower_assign_target_to_expr(ctx, &assign.left)?);
            Box::new(Expr::Logical {
                op: LogicalOp::Coalesce,
                left,
                right: Box::new(rhs),
            })
        } // #853: the match above exhausts every `ast::AssignOp` variant
          // SWC ships today. If SWC adds a new operator, the build breaks
          // here — preferable to a silent runtime error path. No catch-all.
    };

    match &assign.left {
        ast::AssignTarget::Simple(ast::SimpleAssignTarget::Ident(ident)) => {
            let name = ident.id.sym.to_string();
            if let Some(id) = ctx.lookup_local(&name) {
                if ctx.is_local_immutable(id) {
                    return Ok(throw_type_error_const_assignment(&name));
                }
                Ok(Expr::LocalSet(id, value))
            } else if ctx.lookup_class(&name).is_some() || ctx.lookup_func(&name).is_some() {
                // v0.5.757: don't shadow a class/function binding with an
                // implicit local for `<Name> = X` patterns. Drizzle's
                // sql.js uses `((sql2) => { ... })(sql || (sql = {}))`
                // (and the same for SQL) — since the binding exists
                // (truthy), the OR short-circuits and the assignment is
                // dead. Pre-fix the implicit local hid the original
                // binding from later reads. Just evaluate the RHS for
                // side effects. Refs #420.
                Ok(*value)
            } else {
                if ctx.current_strict {
                    return Ok(Expr::Sequence(vec![
                        *value,
                        throw_reference_error_unresolvable_assignment(&name),
                    ]));
                }
                // Variable not found in scope — likely a closure capture that wasn't
                // properly tracked. Create an implicit local to avoid hard failure.
                eprintln!(
                    "  Warning: Assignment to undeclared variable '{}', creating implicit local",
                    name
                );
                let id = ctx.define_local(name, Type::Any);
                Ok(Expr::LocalSet(id, value))
            }
        }
        ast::AssignTarget::Simple(ast::SimpleAssignTarget::Member(member)) => {
            // Proxy set: `proxy.foo = v` / `proxy[k] = v`
            if let ast::Expr::Ident(obj_ident) = member.obj.as_ref() {
                let obj_name = obj_ident.sym.to_string();
                if ctx.proxy_locals.contains(&obj_name) {
                    let proxy = Box::new(if let Some(id) = ctx.lookup_local(&obj_name) {
                        Expr::LocalGet(id)
                    } else {
                        lower_expr(ctx, &member.obj)?
                    });
                    let key = Box::new(match &member.prop {
                        ast::MemberProp::Ident(i) => Expr::String(i.sym.to_string()),
                        ast::MemberProp::Computed(c) => lower_expr(ctx, &c.expr)?,
                        ast::MemberProp::PrivateName(p) => {
                            Expr::String(format!("#{}", p.name.as_str()))
                        }
                    });
                    return Ok(Expr::ProxySet { proxy, key, value });
                }
            }
            // Check if this is a static field assignment (e.g., Counter.count = 5)
            if let ast::Expr::Ident(obj_ident) = member.obj.as_ref() {
                let obj_name = obj_ident.sym.to_string();
                if ctx.lookup_class(&obj_name).is_some() {
                    if let ast::MemberProp::Ident(prop_ident) = &member.prop {
                        let field_name = prop_ident.sym.to_string();
                        if ctx.has_static_field(&obj_name, &field_name) {
                            return Ok(Expr::StaticFieldSet {
                                class_name: obj_name,
                                field_name,
                                value,
                            });
                        }
                    }
                }
                // #1350: process.exitCode = v. Route directly through
                // the runtime setter so the read side
                // (`process.exitCode` → `js_process_exit_code_get`)
                // sees the assigned value. The setter returns its
                // argument so the assignment expression yields the RHS,
                // matching JS semantics. Bypasses the generic
                // PropertySet → js_object_set_field_by_name path which
                // would silently drop the write — same shape as the
                // ProcessEnv assignment fix (#1344).
                if obj_name == "process" && ctx.lookup_local("process").is_none() {
                    if let ast::MemberProp::Ident(prop_ident) = &member.prop {
                        if prop_ident.sym.as_ref() == "exitCode" {
                            return Ok(Expr::Call {
                                callee: Box::new(Expr::ExternFuncRef {
                                    name: "js_process_exit_code_set".to_string(),
                                    param_types: vec![perry_types::Type::Number],
                                    return_type: perry_types::Type::Number,
                                }),
                                args: vec![*value],
                                type_args: vec![],
                            });
                        }
                    }
                }
            }

            // Issue #838: JS-classic prototype-method assignment.
            // Two recognised shapes:
            //   (a) Direct:  `<ClassName>.prototype.<method> = <fn>`
            //                — `member.obj` is `<ClassName>.prototype`
            //                  (a MemberExpr).
            //   (b) Aliased: `let p = <ClassName>.prototype; p.<method>
            //                = <fn>` — `member.obj` is an Ident that
            //                  resolves to a local recorded in
            //                  `ctx.prototype_aliases`. dayjs's minified
            //                  source uses this shape: `var m =
            //                  M.prototype; m.parse = function(){…};`.
            // Route into Expr::RegisterPrototypeMethod which codegen
            // lowers to `js_register_prototype_method(class_id, name,
            // closure)`. The runtime consults the resulting side-table
            // during dispatch so `(new Class()).method()` reaches the
            // registered closure with `this` bound. The pre-fix path
            // lowered both shapes to a generic PropertySet on an
            // unobserved prototype-object proxy, so the assignment was
            // a silent no-op from the user's perspective.
            //
            // TypeScript wrappers (`(Foo.prototype as any).bar = fn`)
            // surface here as `TsAs(MemberExpr)` / `Paren(MemberExpr)`
            // / `TsNonNull(MemberExpr)` / etc. inside `member.obj`.
            // Unwrap them so the recogniser fires on the underlying
            // shape rather than silently falling through.
            fn unwrap_ts(e: &ast::Expr) -> &ast::Expr {
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
            // Extract the method name from either an Ident prop
            // (`p.method`) or a computed string-literal prop
            // (`p['@@transducer/step']`). ramda's transducer pattern,
            // Symbol.iterator stand-ins, and any "method with a dash or
            // a slash" all reach assignment through the computed form;
            // pre-fix only the Ident shape was recognised so these went
            // to a generic PropertySet on an unobserved prototype proxy.
            let method_name_opt: Option<String> = match &member.prop {
                ast::MemberProp::Ident(prop_ident) => Some(prop_ident.sym.to_string()),
                ast::MemberProp::Computed(c) => match c.expr.as_ref() {
                    ast::Expr::Lit(ast::Lit::Str(s)) => {
                        Some(s.value.as_str().unwrap_or("").to_string())
                    }
                    _ => None,
                },
                _ => None,
            };
            if let Some(method_name) = method_name_opt {
                let obj_unwrapped = unwrap_ts(member.obj.as_ref());
                // Issue #838 followup (b): track whether the recognised
                // shape resolves to a `class C {}` (HIR class name) or a
                // `function M() {}` (callable value at runtime). The
                // two routes diverge in codegen — classes go to
                // `Expr::RegisterPrototypeMethod` (class_id known at
                // compile time), function decls go to
                // `Expr::RegisterFunctionPrototypeMethod` (synthetic id
                // allocated at runtime against the closure's bits).
                enum ProtoOwner {
                    Class(String),
                    Func(Expr),
                }
                let resolved: Option<ProtoOwner> = match obj_unwrapped {
                    // (a) <ClassName>.prototype.<method>
                    //     <funcName>.prototype.<method>
                    ast::Expr::Member(inner) => {
                        let prop_is_prototype = matches!(
                            &inner.prop,
                            ast::MemberProp::Ident(p) if p.sym.as_ref() == "prototype"
                        );
                        if prop_is_prototype {
                            let inner_obj = unwrap_ts(inner.obj.as_ref());
                            if let ast::Expr::Ident(cls_ident) = inner_obj {
                                let cls_name = cls_ident.sym.to_string();
                                // Built-in Date has a real runtime prototype object;
                                // Date.prototype writes must remain ordinary property sets.
                                if cls_name == "Date"
                                    && ctx.lookup_local(&cls_name).is_none()
                                    && ctx.lookup_func(&cls_name).is_none()
                                {
                                    None
                                } else if ctx.lookup_class(&cls_name).is_some() {
                                    Some(ProtoOwner::Class(cls_name))
                                } else if let Some(local_id) = ctx.lookup_local(&cls_name) {
                                    if ctx.function_valued_locals.contains(&local_id) {
                                        // dayjs minified shape (inside IIFE):
                                        // `function M(){…}` hoists to a
                                        // `Let M = Closure{…}` inside the
                                        // function expression body, so `M`
                                        // resolves as a local whose init is
                                        // a Closure. Codegen evaluates
                                        // LocalGet to the same closure
                                        // pointer the matching `new M(args)`
                                        // NewDynamic site reads, keying
                                        // `js_register_function_prototype_method`
                                        // and `js_new_function_construct`
                                        // against identical NaN-boxed bits.
                                        Some(ProtoOwner::Func(Expr::LocalGet(local_id)))
                                    } else {
                                        None
                                    }
                                } else if let Some(func_id) = ctx.lookup_func(&cls_name) {
                                    // Top-level / globally-registered function
                                    // without a corresponding local binding
                                    // (rare; most function decls also get a
                                    // local). FuncRef lowering produces the
                                    // singleton wrapper closure — paired
                                    // with the matching `new` site that
                                    // also lowers `<Ident>` through FuncRef
                                    // (the codegen-side `try_static_class_name`
                                    // path), the bits agree.
                                    Some(ProtoOwner::Func(Expr::FuncRef(func_id)))
                                } else {
                                    None
                                }
                            } else {
                                None
                            }
                        } else {
                            None
                        }
                    }
                    // (b) `<alias>.<method>` where the alias local was
                    // initialised from `<ClassName>.prototype` or
                    // `<funcDecl>.prototype` (#838 followup (b) — Babel's
                    // `function Foo(){} var _proto = Foo.prototype; _proto.x = fn`
                    // emit pattern, and dayjs's identical minified form).
                    ast::Expr::Ident(obj_ident) => {
                        let local_id = ctx.lookup_local(obj_ident.sym.as_ref());
                        if let Some(id) = local_id {
                            if let Some(class_name) = ctx.prototype_aliases.get(&id).cloned() {
                                Some(ProtoOwner::Class(class_name))
                            } else if let Some(func_id) =
                                ctx.prototype_function_aliases.get(&id).copied()
                            {
                                Some(ProtoOwner::Func(Expr::FuncRef(func_id)))
                            } else if let Some(src_local) =
                                ctx.prototype_function_locals.get(&id).copied()
                            {
                                Some(ProtoOwner::Func(Expr::LocalGet(src_local)))
                            } else {
                                None
                            }
                        } else {
                            None
                        }
                    }
                    _ => None,
                };
                match resolved {
                    Some(ProtoOwner::Class(class_name)) => {
                        return Ok(Expr::RegisterPrototypeMethod {
                            class_name,
                            method_name,
                            value,
                        });
                    }
                    Some(ProtoOwner::Func(func_expr)) => {
                        return Ok(Expr::RegisterFunctionPrototypeMethod {
                            func: Box::new(func_expr),
                            method_name,
                            value,
                        });
                    }
                    None => {}
                }
            }

            // Issue #577 — `res.statusCode = 200` / `res.statusMessage = "OK"`
            // on a registered ServerResponse native instance. Rewrite to
            // a `__set_<name>` NativeMethodCall so codegen dispatches
            // through the http NATIVE_MODULE_TABLE entries.
            if let ast::Expr::Ident(obj_ident) = member.obj.as_ref() {
                let obj_name = obj_ident.sym.to_string();
                let native_instance = ctx
                    .lookup_native_instance(&obj_name)
                    .map(|(m, c)| (m.to_string(), c.to_string()));
                if let Some((module_name, class_name)) = native_instance {
                    if matches!(module_name.as_str(), "http" | "https") {
                        if let ast::MemberProp::Ident(prop_ident) = &member.prop {
                            let prop = prop_ident.sym.to_string();
                            let setter_method = match (class_name.as_str(), prop.as_str()) {
                                ("ServerResponse", "statusCode") => Some("__set_statusCode"),
                                ("ServerResponse", "statusMessage") => Some("__set_statusMessage"),
                                // Issue #2210 — `server.headersTimeout = N` etc.
                                // route to the `__set_<name>` FFI variants. Phase
                                // 1 just stores; Phase 2 wires hyper deadlines.
                                ("HttpServer", "headersTimeout") => Some("__set_headersTimeout"),
                                ("HttpServer", "keepAliveTimeout") => {
                                    Some("__set_keepAliveTimeout")
                                }
                                ("HttpServer", "keepAliveTimeoutBuffer") => {
                                    Some("__set_keepAliveTimeoutBuffer")
                                }
                                ("HttpServer", "requestTimeout") => Some("__set_requestTimeout"),
                                ("HttpServer", "timeout") => Some("__set_timeout"),
                                ("HttpServer", "maxHeadersCount") => Some("__set_maxHeadersCount"),
                                ("HttpServer", "maxRequestsPerSocket") => {
                                    Some("__set_maxRequestsPerSocket")
                                }
                                ("HttpsServer", "headersTimeout") => Some("__set_headersTimeout"),
                                ("HttpsServer", "keepAliveTimeout") => {
                                    Some("__set_keepAliveTimeout")
                                }
                                ("HttpsServer", "keepAliveTimeoutBuffer") => {
                                    Some("__set_keepAliveTimeoutBuffer")
                                }
                                ("HttpsServer", "requestTimeout") => Some("__set_requestTimeout"),
                                ("HttpsServer", "timeout") => Some("__set_timeout"),
                                ("HttpsServer", "maxHeadersCount") => Some("__set_maxHeadersCount"),
                                ("HttpsServer", "maxRequestsPerSocket") => {
                                    Some("__set_maxRequestsPerSocket")
                                }
                                // #2154 — `http.Agent` / `https.Agent` tunable
                                // properties + the `createConnection` /
                                // `createSocket` overrides. PR #2264 added the
                                // FFI setters + native-table entries but never
                                // wired the assignment path, so `agent.<prop> =
                                // x` silently no-op'd. Route them to the
                                // `__set_<name>` NativeMethodCall here.
                                ("Agent", "protocol") => Some("__set_protocol"),
                                ("Agent", "maxSockets") => Some("__set_maxSockets"),
                                ("Agent", "maxFreeSockets") => Some("__set_maxFreeSockets"),
                                ("Agent", "maxTotalSockets") => Some("__set_maxTotalSockets"),
                                ("Agent", "keepAlive") => Some("__set_keepAlive"),
                                ("Agent", "keepAliveMsecs") => Some("__set_keepAliveMsecs"),
                                ("Agent", "createConnection") => Some("__set_createConnection"),
                                ("Agent", "createSocket") => Some("__set_createSocket"),
                                _ => None,
                            };
                            if let Some(method) = setter_method {
                                let object_expr = lower_expr(ctx, &member.obj)?;
                                return Ok(Expr::NativeMethodCall {
                                    module: module_name,
                                    class_name: Some(class_name),
                                    object: Some(Box::new(object_expr)),
                                    method: method.to_string(),
                                    args: vec![*value],
                                });
                            }
                        }
                    }
                }
            }

            // Issue #650: URL setters — `u.pathname = X` / `u.search = X` /
            // `u.hash = X` mutate the URL object's stored field AND re-derive
            // `href` so subsequent reads see the new composed string. Pre-fix
            // these fell through to generic PropertySet which only updated
            // the named field — `href` then stayed stale (the issue's exact
            // symptom: `u2.href` reads the original after `u2.pathname = "/x"`).
            if let ast::MemberProp::Ident(prop_ident) = &member.prop {
                let prop_name = prop_ident.sym.as_ref();
                let url_setter = matches!(
                    prop_name,
                    "pathname"
                        | "search"
                        | "hash"
                        | "protocol"
                        | "hostname"
                        | "port"
                        | "username"
                        | "password"
                        | "href"
                );
                if url_setter {
                    let is_url_recv = match member.obj.as_ref() {
                        ast::Expr::New(new_expr) => matches!(
                            new_expr.callee.as_ref(),
                            ast::Expr::Ident(ident) if ident.sym.as_ref() == "URL"
                        ),
                        ast::Expr::Ident(ident) => ctx
                            .lookup_local_type(ident.sym.as_ref())
                            .map(|ty| matches!(ty, Type::Named(n) if n == "URL"))
                            .unwrap_or(false),
                        _ => false,
                    };
                    if is_url_recv {
                        let url_expr = lower_expr(ctx, &member.obj)?;
                        return Ok(match prop_name {
                            "pathname" => Expr::UrlSetPathname {
                                url: Box::new(url_expr),
                                value,
                            },
                            "search" => Expr::UrlSetSearch {
                                url: Box::new(url_expr),
                                value,
                            },
                            "hash" => Expr::UrlSetHash {
                                url: Box::new(url_expr),
                                value,
                            },
                            "protocol" => Expr::UrlSetProtocol {
                                url: Box::new(url_expr),
                                value,
                            },
                            "hostname" => Expr::UrlSetHostname {
                                url: Box::new(url_expr),
                                value,
                            },
                            "port" => Expr::UrlSetPort {
                                url: Box::new(url_expr),
                                value,
                            },
                            "username" => Expr::UrlSetUsername {
                                url: Box::new(url_expr),
                                value,
                            },
                            "password" => Expr::UrlSetPassword {
                                url: Box::new(url_expr),
                                value,
                            },
                            "href" => Expr::UrlSetHref {
                                url: Box::new(url_expr),
                                value,
                            },
                            _ => unreachable!(),
                        });
                    }
                }
            }

            // regex.lastIndex = N → RegExpSetLastIndex
            if let ast::MemberProp::Ident(prop_ident) = &member.prop {
                if prop_ident.sym.as_ref() == "lastIndex" {
                    let is_regex_obj = match member.obj.as_ref() {
                        ast::Expr::Lit(ast::Lit::Regex(_)) => true,
                        ast::Expr::Ident(ident) => ctx
                            .lookup_local_type(ident.sym.as_ref())
                            .map(|ty| matches!(ty, Type::Named(n) if n == "RegExp"))
                            .unwrap_or(false),
                        _ => false,
                    };
                    if is_regex_obj {
                        let regex_expr = lower_expr(ctx, &member.obj)?;
                        if matches!(&regex_expr, Expr::RegExp { .. })
                            || matches!(&regex_expr, Expr::LocalGet(_))
                        {
                            return Ok(Expr::RegExpSetLastIndex {
                                regex: Box::new(regex_expr),
                                value,
                            });
                        }
                    }
                }
            }

            let object = Box::new(lower_expr(ctx, &member.obj)?);
            match &member.prop {
                ast::MemberProp::Ident(ident) => {
                    let property = ident.sym.to_string();
                    // Issue #711 part 2: route `<expr>.prototype =
                    // <value>` through SetFunctionPrototype so the
                    // runtime binds the proto object as the function
                    // value's class-prototype source. Effect's
                    // effectable.ts uses this to declare classes via
                    // prototype assignment on a plain function. The
                    // runtime helper is a no-op when `object` doesn't
                    // resolve to a function at runtime (preserves the
                    // baseline for arbitrary `obj.prototype = X`
                    // writes — those are rare and meaningless on
                    // non-functions in practice).
                    if property == "prototype" {
                        return Ok(Expr::SetFunctionPrototype {
                            func: object,
                            proto: value,
                        });
                    }
                    // #1401: process.title = X — route through a runtime
                    // cell so subsequent reads see the new value. Without
                    // this, the assignment lands on the GlobalGet sentinel
                    // that the title getter never consults, so reads still
                    // return argv[0].
                    if property == "title" {
                        if let ast::Expr::Ident(obj_ident) = member.obj.as_ref() {
                            if obj_ident.sym.as_ref() == "process" {
                                return Ok(Expr::ProcessSetTitle(value));
                            }
                        }
                    }
                    Ok(Expr::PropertySet {
                        object,
                        property,
                        value,
                    })
                }
                ast::MemberProp::Computed(computed) => {
                    let index = Box::new(lower_expr(ctx, &computed.expr)?);
                    // Specialize for Uint8Array/Buffer variables → byte-level access.
                    // See mirrored comment in IndexGet lowering: params
                    // typed `Buffer` must route through the byte-write path.
                    if let Expr::LocalGet(id) = &*object {
                        if let Some((_, _, ty)) = ctx.locals.iter().find(|(_, lid, _)| lid == id) {
                            if matches!(ty, Type::Named(n) if n == "Uint8Array" || n == "Buffer") {
                                return Ok(Expr::Uint8ArraySet {
                                    array: object,
                                    index,
                                    value,
                                });
                            }
                        }
                    }
                    // Issue #529: mirror the IndexGet fold — `obj["key"] = v`
                    // with a static non-numeric string key is semantically a
                    // property assignment, not an indexed-element write.
                    // Numeric-string keys keep IndexSet so `arr["0"] = v`
                    // preserves spec-compliant element-write semantics.
                    if let Expr::String(key) = &*index {
                        let is_numeric_string = !key.is_empty()
                            && key.chars().all(|c| c.is_ascii_digit())
                            && !(key.len() > 1 && key.starts_with('0'));
                        if !is_numeric_string {
                            return Ok(Expr::PropertySet {
                                object,
                                property: key.clone(),
                                value,
                            });
                        }
                    }
                    Ok(Expr::IndexSet {
                        object,
                        index,
                        value,
                    })
                }
                ast::MemberProp::PrivateName(private) => {
                    // Private field assignment: this.#field = value
                    let property = format!("#{}", private.name);
                    Ok(Expr::PropertySet {
                        object,
                        property,
                        value,
                    })
                }
            }
        }
        ast::AssignTarget::Simple(ast::SimpleAssignTarget::SuperProp(super_prop)) => {
            let mut exprs = Vec::new();
            if let ast::SuperProp::Computed(computed) = &super_prop.prop {
                exprs.push(lower_expr(ctx, &computed.expr)?);
            }
            exprs.push(*value);
            exprs.push(throw_type_error_const_assignment(""));
            Ok(Expr::Sequence(exprs))
        }
        ast::AssignTarget::Pat(pat) => {
            // Destructuring assignment: [a, b] = expr or { a, b } = expr
            // We need to lower this to a sequence of assignments
            lower_destructuring_assignment(ctx, pat, value)
        }
        // Unwrap TypeScript type annotations and parentheses for assignment
        ast::AssignTarget::Simple(ast::SimpleAssignTarget::Paren(paren)) => {
            lower_expr_assignment(ctx, &paren.expr, value)
        }
        ast::AssignTarget::Simple(ast::SimpleAssignTarget::TsAs(ts_as)) => {
            lower_expr_assignment(ctx, &ts_as.expr, value)
        }
        ast::AssignTarget::Simple(ast::SimpleAssignTarget::TsNonNull(ts_nn)) => {
            lower_expr_assignment(ctx, &ts_nn.expr, value)
        }
        ast::AssignTarget::Simple(ast::SimpleAssignTarget::TsTypeAssertion(ts_ta)) => {
            lower_expr_assignment(ctx, &ts_ta.expr, value)
        }
        ast::AssignTarget::Simple(ast::SimpleAssignTarget::TsSatisfies(ts_sat)) => {
            lower_expr_assignment(ctx, &ts_sat.expr, value)
        }
        other => Err(anyhow!("Unsupported assignment target: {:?}", other)),
    }
}
