//! Assignment expression lowering: `ast::Expr::Assign`.
//!
//! Tier 2.3 round 3 (v0.5.339) — extracts the 312-LOC `Assign` arm
//! from `lower_expr`. Covers `x = v`, `x += v` (and other compound
//! assigns), `obj.prop = v`, `obj[k] = v`, plus destructuring assigns
//! `[a, b] = arr` and `{a, b} = obj` (these last two desugar to a
//! sequence expression of individual assignments).

use anyhow::{anyhow, Result};
use perry_types::{LocalId, Type};
use swc_ecma_ast as ast;

use crate::destructuring::lower_destructuring_assignment;
use crate::ir::{BinaryOp, Expr, LogicalOp, Stmt};
use crate::lower_patterns::lower_assign_target_to_expr;

use super::{
    lower_expr, lower_expr_assignment, strict_global_assign_existing_or_throw,
    with_set_fallback_for_ident, LoweringContext,
};

fn assignment_target_inferred_name(target: &ast::AssignTarget) -> Option<String> {
    match target {
        ast::AssignTarget::Simple(ast::SimpleAssignTarget::Ident(ident)) => {
            let name = ident.id.sym.to_string();
            (!name.is_empty()).then_some(name)
        }
        _ => None,
    }
}

fn anonymous_class_without_own_static_name(class: &ast::ClassExpr) -> bool {
    if class.ident.is_some() {
        return false;
    }
    !class.class.body.iter().any(|member| match member {
        ast::ClassMember::Method(method) if method.is_static => {
            matches!(&method.key, ast::PropName::Ident(ident) if ident.sym.as_ref() == "name")
                || matches!(&method.key, ast::PropName::Str(s) if s.value.as_str() == Some("name"))
        }
        ast::ClassMember::ClassProp(prop) if prop.is_static => {
            matches!(&prop.key, ast::PropName::Ident(ident) if ident.sym.as_ref() == "name")
                || matches!(&prop.key, ast::PropName::Str(s) if s.value.as_str() == Some("name"))
        }
        _ => false,
    })
}

pub(crate) fn rhs_accepts_assignment_name(expr: &ast::Expr) -> bool {
    match expr {
        ast::Expr::Arrow(_) => true,
        ast::Expr::Fn(fn_expr) => fn_expr.ident.is_none(),
        ast::Expr::Class(class_expr) => anonymous_class_without_own_static_name(class_expr),
        ast::Expr::Paren(paren) => rhs_accepts_assignment_name(&paren.expr),
        _ => false,
    }
}

fn lower_rhs_with_assignment_name(
    ctx: &mut LoweringContext,
    rhs: &ast::Expr,
    name: Option<String>,
) -> Result<Expr> {
    let Some(name) = name.filter(|_| rhs_accepts_assignment_name(rhs)) else {
        return lower_expr(ctx, rhs);
    };
    let old_name = ctx.assignment_inferred_name.replace(name);
    let result = lower_expr(ctx, rhs);
    ctx.assignment_inferred_name = old_name;
    result
}

pub(crate) fn throw_type_error_const_assignment(name: &str) -> Expr {
    Expr::Call {
        callee: Box::new(Expr::ExternFuncRef {
            name: "js_throw_type_error_const_assignment".to_string(),
            param_types: vec![Type::String],
            return_type: Type::Any,
        }),
        args: vec![Expr::String(name.to_string())],
        type_args: vec![],
        byte_offset: 0,
    }
}

fn throw_restricted_function_property_assignment() -> Expr {
    Expr::Call {
        callee: Box::new(Expr::ExternFuncRef {
            name: "js_throw_restricted_function_property_assignment".to_string(),
            param_types: vec![],
            return_type: Type::Any,
        }),
        args: vec![],
        type_args: vec![],
        byte_offset: 0,
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
        byte_offset: 0,
    }
}

fn simple_ident_target_name(target: &ast::AssignTarget) -> Option<&str> {
    match target {
        ast::AssignTarget::Simple(ast::SimpleAssignTarget::Ident(ident)) => {
            Some(ident.id.sym.as_ref())
        }
        ast::AssignTarget::Simple(ast::SimpleAssignTarget::Paren(paren)) => {
            expr_ident_name(paren.expr.as_ref())
        }
        ast::AssignTarget::Simple(ast::SimpleAssignTarget::TsAs(ts_as)) => {
            expr_ident_name(ts_as.expr.as_ref())
        }
        ast::AssignTarget::Simple(ast::SimpleAssignTarget::TsNonNull(ts_nn)) => {
            expr_ident_name(ts_nn.expr.as_ref())
        }
        ast::AssignTarget::Simple(ast::SimpleAssignTarget::TsTypeAssertion(ts_ta)) => {
            expr_ident_name(ts_ta.expr.as_ref())
        }
        ast::AssignTarget::Simple(ast::SimpleAssignTarget::TsSatisfies(ts_sat)) => {
            expr_ident_name(ts_sat.expr.as_ref())
        }
        _ => None,
    }
}

fn expr_ident_name(expr: &ast::Expr) -> Option<&str> {
    match expr {
        ast::Expr::Ident(ident) => Some(ident.sym.as_ref()),
        ast::Expr::Paren(paren) => expr_ident_name(paren.expr.as_ref()),
        ast::Expr::TsAs(ts_as) => expr_ident_name(ts_as.expr.as_ref()),
        ast::Expr::TsNonNull(ts_nn) => expr_ident_name(ts_nn.expr.as_ref()),
        ast::Expr::TsTypeAssertion(ts_ta) => expr_ident_name(ts_ta.expr.as_ref()),
        ast::Expr::TsSatisfies(ts_sat) => expr_ident_name(ts_sat.expr.as_ref()),
        _ => None,
    }
}

fn logical_assignment_op(op: ast::AssignOp) -> Option<LogicalOp> {
    match op {
        ast::AssignOp::AndAssign => Some(LogicalOp::And),
        ast::AssignOp::OrAssign => Some(LogicalOp::Or),
        ast::AssignOp::NullishAssign => Some(LogicalOp::Coalesce),
        _ => None,
    }
}

fn lower_logical_assignment(
    ctx: &mut LoweringContext,
    assign: &ast::AssignExpr,
    rhs: Expr,
    op: LogicalOp,
) -> Result<Expr> {
    let left = Box::new(lower_assign_target_to_expr(ctx, &assign.left)?);
    let right = Box::new(lower_assignment_target(ctx, &assign.left, Box::new(rhs))?);
    Ok(Expr::Logical { op, left, right })
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
                                        ctx.push_module_native_instance((
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
                        ctx.push_module_native_instance((
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
                    ctx.push_module_native_instance((
                        var_name,
                        module.to_string(),
                        class.to_string(),
                    ));
                }
            }
        }
    }

    if let Some(name) = simple_ident_target_name(&assign.left)
        .zip(expr_ident_name(assign.right.as_ref()))
        .and_then(|(left, right)| (left == right).then_some(left))
    {
        // `x = x` with no binding anywhere → ReferenceError (the RHS read of
        // an unresolvable reference throws before the sloppy-global create).
        // A pre-registered module `var` declared *later* in the source is
        // still a declared binding (var hoisting) — self-assignment before
        // the declaration statement is fine and yields undefined.
        if ctx.lookup_local(name).is_none() {
            return Ok(throw_reference_error_unresolvable_assignment(name));
        }
    }

    // NamedEvaluation also applies to logical assignment (`x ||= function(){}`,
    // `x &&= () => {}`, `x ??= class {}`): when the LHS is a plain identifier and
    // the RHS is an anonymous function/class, the function's `.name` becomes the
    // identifier (ES2024 §13.15.2). Plain compound assignments (`+=`, `*=`, …)
    // are NOT NamedEvaluation contexts, so they stay name-less.
    let inferred_name_op = matches!(
        assign.op,
        ast::AssignOp::Assign
            | ast::AssignOp::AndAssign
            | ast::AssignOp::OrAssign
            | ast::AssignOp::NullishAssign
    );
    let rhs = lower_rhs_with_assignment_name(
        ctx,
        &assign.right,
        inferred_name_op
            .then(|| assignment_target_inferred_name(&assign.left))
            .flatten(),
    )?;

    // #4586 / #4594: logical assignments (`&&=`, `||=`, `??=`) must not store
    // unconditionally. Desugaring to `a = (a OP rhs)` (the generic
    // compound-assign shape below) always runs PutValue, which for a property
    // target fires setters spuriously and throws `TypeError: Cannot assign to
    // read only property` on non-writable `Object.defineProperty` data props —
    // e.g. Zod v4's `inst._zod ??= {}` where `_zod` is already non-nullish and
    // read-only, breaking every check/refinement-based schema under
    // `perry.compilePackages`.
    //
    // Per ECMAScript LogicalAssignment, the store (PutValue) must be skipped
    // entirely when the short-circuit holds. `lower_logical_assignment`
    // desugars to `read(target) OP (target = rhs)` so the assignment lives on
    // the RHS of the logical operator and is therefore only evaluated on the
    // branch that actually needs to write. `rhs` is consumed exactly once.
    //
    // This covers both `Ident` and `Member` targets via the shared
    // `lower_assignment_target` helper: while plain `Ident` locals have no
    // setters, routing them through the short-circuit keeps the const-reassign
    // path spec-correct (the RHS is still evaluated before the
    // `TypeError: Assignment to constant variable` is thrown) and avoids a
    // dead, Member-only special case.
    if let Some(op) = logical_assignment_op(assign.op) {
        return lower_logical_assignment(ctx, assign, rhs, op);
    }

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
        ast::AssignOp::AndAssign | ast::AssignOp::OrAssign | ast::AssignOp::NullishAssign => {
            unreachable!("logical assignment is lowered before compound assignment")
        } // #853: the match above exhausts every `ast::AssignOp` variant
          // SWC ships today. If SWC adds a new operator, the build breaks
          // here — preferable to a silent runtime error path. No catch-all.
    };

    lower_assignment_target(ctx, &assign.left, value)
}

/// Lower `<name> = value` where `<name>` is an identifier assignment target.
///
/// #6300: this is the ONE place identifier stores are resolved, so the
/// `const`-immutability (and class-inner-name) checks can't be routed around.
/// It is shared by the bare-`Ident` `AssignTarget` arm below and by
/// `lower_expr_assignment`'s `Ident` arm, which is what the parenthesized /
/// TS-cast targets (`(c) = 9`, `(c as any) = 9`, `(c satisfies T) = 9`,
/// `(c!) = 9`) unwrap into. Before the extraction, only the bare-`Ident` arm
/// checked immutability, so any wrapper on the LHS silently mutated a `const`.
pub(crate) fn lower_ident_assignment(
    ctx: &mut LoweringContext,
    name: String,
    value: Box<Expr>,
) -> Result<Expr> {
    if let Some(env_id) = ctx.active_with_envs_for_ident(&name).into_iter().next() {
        let fallback = with_set_fallback_for_ident(ctx, &name);
        return Ok(Expr::WithSet {
            object: Box::new(Expr::LocalGet(env_id)),
            property: name,
            value,
            fallback,
            strict: ctx.current_strict,
        });
    }
    if let Some(id) = ctx.lookup_local(&name) {
        if ctx.is_local_immutable(id) {
            // `const c = 1; c = 9` (and every wrapped spelling of the same
            // target) evaluates the RHS for side effects, then throws
            // `TypeError: Assignment to constant variable.`
            return Ok(Expr::Sequence(vec![
                *value,
                throw_type_error_const_assignment(&name),
            ]));
        }
        Ok(Expr::LocalSet(id, value))
    } else if ctx.current_class_inner_name.as_deref() == Some(name.as_str()) {
        // Assigning to the class own-name binding from inside the class
        // body targets the immutable inner `const` binding -> TypeError
        // (test262 language/statements/class/name-binding/const). Evaluate
        // the RHS for side effects first, then throw. A local/param that
        // shadows the name was already handled by the `lookup_local` arm
        // above, so this only fires for the genuine class binding.
        Ok(Expr::Sequence(vec![
            *value,
            throw_type_error_const_assignment(&name),
        ]))
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
            // #5989: strict-mode assignment to an existing global
            // builtin is a property write, not a ReferenceError. See
            // `strict_global_assign_existing_or_throw` for the full
            // rationale.
            return Ok(strict_global_assign_existing_or_throw(name, value));
        }
        eprintln!(
            "  Warning: Assignment to undeclared variable '{}', creating sloppy global",
            name
        );
        // Sloppy implicit global: the binding IS a property of globalThis
        // (spec CreateGlobalVarBinding on the global object), so `foo = 1`
        // must be visible as `globalThis.foo`, write through to a
        // pre-existing global property, and observe a later
        // `delete globalThis.foo`. Reads of the name resolve through the
        // `js_global_get_or_throw_unresolved` fallback, so no module-local
        // shadow may be created here (a stale local would keep serving
        // deleted/overwritten values).
        // NOTE: `GlobalGet(0)` alone is a by-name routing SENTINEL in
        // codegen (bare reads lower to 0.0) — the write must target
        // the VALUE globalThis, which the `PropertyGet { GlobalGet(0),
        // "globalThis" }` shape resolves to the real global object.
        Ok(Expr::PropertySet {
            object: Box::new(Expr::PropertyGet {
                object: Box::new(Expr::GlobalGet(0)),
                property: "globalThis".to_string(),
            }),
            property: name,
            value,
        })
    }
}

fn lower_assignment_target(
    ctx: &mut LoweringContext,
    target: &ast::AssignTarget,
    value: Box<Expr>,
) -> Result<Expr> {
    match target {
        ast::AssignTarget::Simple(ast::SimpleAssignTarget::Ident(ident)) => {
            lower_ident_assignment(ctx, ident.id.sym.to_string(), value)
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
                    return Ok(Expr::PutValueSet {
                        target: proxy.clone(),
                        key,
                        value,
                        receiver: proxy,
                        strict: ctx.current_strict,
                    });
                }
            }
            // Check if this is a static field assignment (e.g., Counter.count = 5)
            if let ast::Expr::Ident(obj_ident) = member.obj.as_ref() {
                let obj_name = obj_ident.sym.to_string();
                // `f.caller = v` / `f.arguments = v` on a declared function —
                // the poisoned setter-less accessor on Function.prototype
                // throws (strict semantics; Perry-compiled code is strict).
                // The runtime closure-receiver path covers function VALUES;
                // this covers `function f(){}` declarations whose property
                // writes lower before reaching it. Refs test262 13.2-*-s.
                if ctx.lookup_local(&obj_name).is_none() && ctx.lookup_func(&obj_name).is_some() {
                    if let ast::MemberProp::Ident(prop_ident) = &member.prop {
                        if matches!(prop_ident.sym.as_ref(), "caller" | "arguments") {
                            return Ok(Expr::Sequence(vec![
                                *value,
                                throw_restricted_function_property_assignment(),
                            ]));
                        }
                    }
                }
                // #5938 follow-up: resolve scope-local class renames so a
                // colliding body-local `class X`'s static write targets the
                // renamed registrant, not the first same-named one.
                let resolved_class = ctx.resolve_class_name(&obj_name);
                if ctx.lookup_class(&resolved_class).is_some() {
                    if let ast::MemberProp::Ident(prop_ident) = &member.prop {
                        let field_name = prop_ident.sym.to_string();
                        if ctx.has_static_field(&resolved_class, &field_name) {
                            return Ok(Expr::StaticFieldSet {
                                class_name: resolved_class,
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
                                byte_offset: 0,
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
                fn class_has_accessor(
                    ctx: &LoweringContext,
                    class_name: &str,
                    method_name: &str,
                ) -> bool {
                    ctx.lookup_class_accessor_names(class_name)
                        .is_some_and(|names| names.contains_any(method_name) || names.has_computed)
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
                                } else if ctx.lookup_local(&cls_name).is_some()
                                    && !ctx.inferred_class_bindings.contains(cls_name.as_str())
                                {
                                    // A lexical local shadows any same-named
                                    // module-scope class for this write too
                                    // (wall 7's disease, 4th surface): the
                                    // vendored eventemitter3 `function s(){}`
                                    // + `s.prototype.emit = fn` inside a
                                    // turbopack chunk that ALSO has minified
                                    // `class s {…}` declarations registered
                                    // emit onto the unrelated class — the ES5
                                    // constructor's prototype stayed EMPTY and
                                    // every subclass (p-queue's PQueue) lost
                                    // the inherited surface. A function-valued
                                    // local keys the registration by the
                                    // closure VALUE; any other local falls to
                                    // the ordinary property-set path.
                                    let local_id = ctx.lookup_local(&cls_name).unwrap();
                                    if ctx.function_valued_locals.contains(&local_id) {
                                        Some(ProtoOwner::Func(Expr::LocalGet(local_id)))
                                    } else {
                                        None
                                    }
                                } else if ctx.lookup_class(&cls_name).is_some()
                                    && class_has_accessor(ctx, &cls_name, &method_name)
                                {
                                    // `C.prototype.<accessor> = v` where `<accessor>`
                                    // is a `set`/`get` declared on the class is an
                                    // ordinary write that must INVOKE the setter — not
                                    // a prototype-method monkey-patch. Fall through to
                                    // the generic PropertySet path (which reaches the
                                    // runtime prototype-ref setter dispatch). Test262
                                    // accessor-name-inst setters.
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
                                } else {
                                    ctx.lookup_func(&cls_name)
                                        .map(|func_id| ProtoOwner::Func(Expr::FuncRef(func_id)))
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
                                if class_has_accessor(ctx, &class_name, &method_name) {
                                    None
                                } else {
                                    Some(ProtoOwner::Class(class_name))
                                }
                            } else if let Some(func_id) =
                                ctx.prototype_function_aliases.get(&id).copied()
                            {
                                Some(ProtoOwner::Func(Expr::FuncRef(func_id)))
                            } else {
                                ctx.prototype_function_locals
                                    .get(&id)
                                    .copied()
                                    .map(|src_local| ProtoOwner::Func(Expr::LocalGet(src_local)))
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
                                ("ServerResponse", "sendDate") => Some("__set_sendDate"),
                                ("ServerResponse", "strictContentLength") => {
                                    Some("__set_strictContentLength")
                                }
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

            let object_expr = lower_expr(ctx, &member.obj)?;
            // #5437: `PutValueSet` / `PropertySet` carry the object expression
            // in BOTH `target` and `receiver`, and codegen evaluates both. When
            // the object is itself an assignment to a local — Next.js' React
            // renderer does `(r = n2(t = new nX(t, ...), ...)).parentFlushed =
            // !0` — duplicating it re-runs the assignment (and the nested `new
            // nX`), so the Request was constructed twice with the already-
            // reassigned `t` and its `resumableState` became another Request
            // (dynamic-SSR 500). Evaluate the assignment ONCE as a prelude and
            // read the just-assigned local back from both slots. Reusing the
            // assignment's own already-slotted local avoids a fresh temp (which
            // gets no codegen stack slot in expression position). Pure / non-
            // assignment objects keep the long-standing duplicate-in-place
            // shape so codegen fast paths and IR are unchanged.
            let reuse_id = if let Expr::LocalSet(set_id, _) = &object_expr {
                Some(*set_id)
            } else {
                None
            };
            let (mut prelude, object): (Option<Expr>, Box<Expr>) = match reuse_id {
                Some(id) => (Some(object_expr), Box::new(Expr::LocalGet(id))),
                None => (None, Box::new(object_expr.clone())),
            };
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
                        return Ok(wrap_assign_object_prelude(
                            prelude.take(),
                            Expr::SetFunctionPrototype {
                                func: object,
                                proto: value,
                            },
                        ));
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
                    Ok(wrap_assign_object_prelude(
                        prelude.take(),
                        Expr::PutValueSet {
                            target: object.clone(),
                            key: Box::new(Expr::String(property)),
                            value,
                            receiver: object,
                            strict: ctx.current_strict,
                        },
                    ))
                }
                ast::MemberProp::Computed(computed) => {
                    let index = Box::new(lower_expr(ctx, &computed.expr)?);
                    // Specialize for Uint8Array/Buffer variables → byte-level access.
                    // See mirrored comment in IndexGet lowering: params
                    // typed `Buffer` must route through the byte-write path.
                    if let Expr::LocalGet(id) = &*object {
                        if let Some((_, _, ty)) = ctx.locals.iter().find(|(_, lid, _)| lid == id) {
                            if matches!(ty, Type::Named(n) if n == "Uint8Array" || n == "Buffer") {
                                return Ok(wrap_assign_object_prelude(
                                    prelude.take(),
                                    Expr::Uint8ArraySet {
                                        array: object,
                                        index,
                                        value,
                                    },
                                ));
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
                            return Ok(wrap_assign_object_prelude(
                                prelude.take(),
                                Expr::PutValueSet {
                                    target: object.clone(),
                                    key: Box::new(Expr::String(key.clone())),
                                    value,
                                    receiver: object,
                                    strict: ctx.current_strict,
                                },
                            ));
                        }
                    }
                    Ok(wrap_assign_object_prelude(
                        prelude.take(),
                        Expr::PutValueSet {
                            target: object.clone(),
                            key: index,
                            value,
                            receiver: object,
                            strict: ctx.current_strict,
                        },
                    ))
                }
                ast::MemberProp::PrivateName(private) => {
                    // Private field assignment: this.#field = value. Guard the
                    // receiver so a write to a wrong receiver — or to a
                    // getter-only accessor / a private method — throws.
                    let property = format!("#{}", private.name);
                    let object = super::expr_member::wrap_private_guard(
                        ctx,
                        object,
                        &property,
                        super::expr_member::PRIV_OP_WRITE,
                    );
                    Ok(wrap_assign_object_prelude(
                        prelude.take(),
                        Expr::PropertySet {
                            object,
                            property,
                            value,
                        },
                    ))
                }
            }
        }
        ast::AssignTarget::Simple(ast::SimpleAssignTarget::SuperProp(super_prop)) => {
            if ctx.current_class_member_is_static {
                let mut exprs = Vec::new();
                if let ast::SuperProp::Computed(computed) = &super_prop.prop {
                    exprs.push(lower_expr(ctx, &computed.expr)?);
                }
                exprs.push(*value);
                exprs.push(throw_type_error_const_assignment(""));
                return Ok(Expr::Sequence(exprs));
            }
            let key = match &super_prop.prop {
                ast::SuperProp::Ident(ident) => Box::new(Expr::String(ident.sym.to_string())),
                ast::SuperProp::Computed(computed) => Box::new(lower_expr(ctx, &computed.expr)?),
            };
            if let Some(home_id) = ctx.object_super_home_stack.last().copied() {
                Ok(Expr::ObjectSuperPropertySet {
                    home: Box::new(Expr::LocalGet(home_id)),
                    key,
                    value,
                    receiver: Box::new(Expr::This),
                })
            } else {
                let parent_class_name = ctx.current_class_super_ident.clone();
                let parent_class_id = parent_class_name
                    .as_deref()
                    .and_then(|parent| ctx.lookup_class(parent))
                    .unwrap_or(0);
                Ok(Expr::SuperPropertySet {
                    parent_class_id,
                    parent_class_name,
                    key,
                    value,
                })
            }
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

/// #5437: prepend a once-evaluated object prelude (`LocalSet(tmp, object)`)
/// in front of an assignment expression that references the temp from both
/// `target` and `receiver`. `None` ⇒ the object was pure and used in place.
fn wrap_assign_object_prelude(prelude: Option<Expr>, e: Expr) -> Expr {
    match prelude {
        Some(p) => Expr::Sequence(vec![p, e]),
        None => e,
    }
}

/// The `BinaryOp` a plain compound assignment (`+=`, `*=`, `<<=`, …) reads-and-
/// writes with. Returns `None` for `=` and the logical assignments
/// (`&&=`/`||=`/`??=`), which are not simple read-op-write.
fn compound_binary_op(op: ast::AssignOp) -> Option<BinaryOp> {
    use ast::AssignOp::*;
    Some(match op {
        AddAssign => BinaryOp::Add,
        SubAssign => BinaryOp::Sub,
        MulAssign => BinaryOp::Mul,
        DivAssign => BinaryOp::Div,
        ModAssign => BinaryOp::Mod,
        BitAndAssign => BinaryOp::BitAnd,
        BitOrAssign => BinaryOp::BitOr,
        BitXorAssign => BinaryOp::BitXor,
        LShiftAssign => BinaryOp::Shl,
        RShiftAssign => BinaryOp::Shr,
        ZeroFillRShiftAssign => BinaryOp::UShr,
        ExpAssign => BinaryOp::Pow,
        Assign | AndAssign | OrAssign | NullishAssign => return None,
    })
}

/// #6071: statement-level compound assignment to a member/index target
/// (`a.b op= v;`, `a[k] op= v;`). Spill the base and (computed) key into
/// `Stmt::Let` temps so each is evaluated EXACTLY ONCE, then build the read and
/// the write from those temps. Without this, `lower_assign` lowers the target
/// twice (once as the read operand, once as the write target), double-evaluating
/// the base and a side-effecting computed key (`arr[i++] += 1` was wrong).
///
/// Returns `None` — so the caller falls back to the ordinary expression
/// lowering — for anything that isn't a plain member/index compound assign, or
/// that routes through the proxy / `with` / private-brand paths (those have
/// their own semantics and are left unchanged).
pub(crate) fn hoist_compound_member_assign(
    ctx: &mut LoweringContext,
    assign: &ast::AssignExpr,
) -> Result<Option<Vec<Stmt>>> {
    // A plain compound (`+=`, …) or a logical (`&&=`/`||=`/`??=`) assignment;
    // `=` is not one of these and keeps the ordinary path.
    let bin_op = compound_binary_op(assign.op);
    let logical_op = logical_assignment_op(assign.op);
    if bin_op.is_none() && logical_op.is_none() {
        return Ok(None);
    }
    let ast::AssignTarget::Simple(ast::SimpleAssignTarget::Member(member)) = &assign.left else {
        return Ok(None);
    };
    // Private fields and proxy / `with`-scoped receivers keep the existing path.
    if matches!(member.prop, ast::MemberProp::PrivateName(_)) {
        return Ok(None);
    }
    if let ast::Expr::Ident(obj_ident) = member.obj.as_ref() {
        let n = obj_ident.sym.as_ref();
        if ctx.proxy_locals.contains(n) || !ctx.active_with_envs_for_ident(n).is_empty() {
            return Ok(None);
        }
    }

    let mut stmts: Vec<Stmt> = Vec::new();
    let spill =
        |ctx: &mut LoweringContext, stmts: &mut Vec<Stmt>, tag: &str, init: Expr| -> LocalId {
            let id = ctx.fresh_local();
            stmts.push(Stmt::Let {
                id,
                name: format!("__cmpd_{}_{}", tag, id),
                ty: Type::Any,
                mutable: false,
                init: Some(init),
            });
            id
        };

    // Base — always spilled (evaluated once).
    let base = lower_expr(ctx, &member.obj)?;
    let base_id = spill(ctx, &mut stmts, "base", base);

    // Property name (static) or computed key spilled to its own temp.
    let prop: Option<String>;
    let key_id: Option<LocalId>;
    match &member.prop {
        ast::MemberProp::Ident(i) => {
            prop = Some(i.sym.to_string());
            key_id = None;
        }
        ast::MemberProp::Computed(c) => {
            let key = lower_expr(ctx, &c.expr)?;
            key_id = Some(spill(ctx, &mut stmts, "key", key));
            prop = None;
        }
        ast::MemberProp::PrivateName(_) => unreachable!("guarded above"),
    }

    let read = match (&prop, key_id) {
        (Some(p), _) => Expr::PropertyGet {
            object: Box::new(Expr::LocalGet(base_id)),
            property: p.clone(),
        },
        (None, Some(k)) => Expr::IndexGet {
            object: Box::new(Expr::LocalGet(base_id)),
            index: Box::new(Expr::LocalGet(k)),
        },
        _ => unreachable!(),
    };

    // A write of `value` back to the (spilled) target.
    let write_of = |value: Expr| -> Expr {
        match (&prop, key_id) {
            (Some(p), _) => Expr::PropertySet {
                object: Box::new(Expr::LocalGet(base_id)),
                property: p.clone(),
                value: Box::new(value),
            },
            (None, Some(k)) => Expr::IndexSet {
                object: Box::new(Expr::LocalGet(base_id)),
                index: Box::new(Expr::LocalGet(k)),
                value: Box::new(value),
            },
            _ => unreachable!(),
        }
    };

    // RHS is evaluated once. Compound: unconditionally, after the read (spec
    // order). Logical: only on the branch that writes, so short-circuit
    // semantics are preserved (`a[k] ||= v` doesn't write when `a[k]` is truthy).
    let rhs = lower_expr(ctx, &assign.right)?;
    let final_expr = if let Some(op) = bin_op {
        write_of(Expr::Binary {
            op,
            left: Box::new(read),
            right: Box::new(rhs),
        })
    } else {
        // `read OP (target = rhs)` — mirrors `lower_logical_assignment`.
        Expr::Logical {
            op: logical_op.unwrap(),
            left: Box::new(read),
            right: Box::new(write_of(rhs)),
        }
    };
    stmts.push(Stmt::Expr(final_expr));
    Ok(Some(stmts))
}
