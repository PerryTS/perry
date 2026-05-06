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
        }
        _ => return Err(anyhow!("Unsupported assignment operator: {:?}", assign.op)),
    };

    match &assign.left {
        ast::AssignTarget::Simple(ast::SimpleAssignTarget::Ident(ident)) => {
            let name = ident.id.sym.to_string();
            if let Some(id) = ctx.lookup_local(&name) {
                Ok(Expr::LocalSet(id, value))
            } else {
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
