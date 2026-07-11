//! The `ast::Expr::Bin` arm of `lower_expr_impl`, extracted to a helper.
//! Pure code move — no behavior change.

use super::*;
use crate::lower::*;
use anyhow::{anyhow, Result};
use perry_types::{LocalId, Type};
use swc_common::Spanned;
use swc_ecma_ast as ast;

use crate::ir::*;
use crate::lower_types::extract_ts_type_with_ctx;

pub(crate) fn lower_bin_expr(ctx: &mut LoweringContext, bin: &ast::BinExpr) -> Result<Expr> {
    // Handle 'in' operator: property in object
    if matches!(bin.op, ast::BinaryOp::In) {
        if let ast::Expr::PrivateName(private) = bin.left.as_ref() {
            let class_name = ctx.current_class.clone().ok_or_else(|| {
                anyhow!("Private name brand check is only supported inside a class")
            })?;
            let field_name = format!("#{}", private.name);
            let object = Box::new(lower_expr(ctx, &bin.right)?);
            return Ok(Expr::PrivateBrandCheck {
                class_name,
                field_name,
                object,
            });
        }
        // Proxy fast path: `key in proxy` routes through js_proxy_has.
        if let ast::Expr::Ident(obj_ident) = bin.right.as_ref() {
            let obj_name = obj_ident.sym.to_string();
            if ctx.proxy_locals.contains(&obj_name) {
                let key = Box::new(lower_expr(ctx, &bin.left)?);
                let proxy = Box::new(lower_expr(ctx, &bin.right)?);
                return Ok(Expr::ProxyHas { proxy, key });
            }
        }
        let property = Box::new(lower_expr(ctx, &bin.left)?);
        let object = Box::new(lower_expr(ctx, &bin.right)?);
        return Ok(Expr::In { property, object });
    }

    // Handle instanceof specially - needs to extract class name
    if matches!(bin.op, ast::BinaryOp::InstanceOf) {
        // WeakRef / FinalizationRegistry: pre-scan tracks local
        // constructor results explicitly, so common `local instanceof
        // WeakRef|FinalizationRegistry` checks can be folded at
        // lowering time when we recognise the receiver.
        if let ast::Expr::Ident(class_ident) = bin.right.as_ref() {
            let class_name = class_ident.sym.as_ref();
            // #6233: only fold when the RHS really is the GLOBAL WeakRef /
            // FinalizationRegistry — a user `class WeakRef {}` (or a local/
            // function/import of that name) shadows the global, and its
            // instances must take the generic instanceof path below.
            if (class_name == "WeakRef" || class_name == "FinalizationRegistry")
                && !ctx.shadows_unqualified_global(class_name)
            {
                if let ast::Expr::Ident(left_ident) = bin.left.as_ref() {
                    let local_name = left_ident.sym.to_string();
                    let is_match = (class_name == "WeakRef"
                        && ctx.weakref_locals.contains(&local_name))
                        || (class_name == "FinalizationRegistry"
                            && ctx.finreg_locals.contains(&local_name));
                    return Ok(Expr::Bool(is_match));
                }
            }
        }
        let expr = Box::new(lower_expr(ctx, &bin.left)?);
        // Right side can be an identifier (ClassName) or member expression (Module.ClassName)
        let ty = match bin.right.as_ref() {
            ast::Expr::Ident(ident) => ident.sym.to_string(),
            ast::Expr::Member(member) => {
                // Handle Module.ClassName - extract the full qualified name
                let obj_name = if let ast::Expr::Ident(obj_ident) = member.obj.as_ref() {
                    obj_ident.sym.to_string()
                } else {
                    "Unknown".to_string()
                };
                let prop_name = match &member.prop {
                    ast::MemberProp::Ident(prop_ident) => prop_ident.sym.to_string(),
                    _ => "Unknown".to_string(),
                };
                format!("{}.{}", obj_name, prop_name)
            }
            _ => {
                // For complex expressions, use a generic type name
                "Object".to_string()
            }
        };
        // v0.5.749: when the right side resolves to a local
        // variable holding a class ref (e.g. `function is(value,
        // type) { return value instanceof type; }`), emit a
        // dynamic-dispatch path that evaluates the class ref at
        // runtime. Without this, the codegen sees `ty = "type"`
        // (the param name), can't resolve it as a class, and
        // falls through to `class_id = 0` — every dynamic
        // instanceof returns false. Drizzle's `is(value, type)`
        // chain depends on this. Refs #420 / #618 followup.
        let ty_expr = match bin.right.as_ref() {
            ast::Expr::Ident(ident) => {
                let name = ident.sym.as_ref();
                // `x instanceof undefined`: `undefined` is the primitive
                // value, never a class name. Codegen would resolve `ty =
                // "undefined"` to class_id 0 and silently return `false`;
                // ECMAScript requires evaluating the RHS and throwing a
                // TypeError because it is not an object (test262
                // instanceof/S11.8.6_A3 #4). Lower it to the undefined
                // value so it routes through `js_instanceof_dynamic`.
                if name == "undefined" {
                    Some(Box::new(Expr::Undefined))
                } else
                // A local holding a class ref (drizzle's `is(value, type)`),
                // OR a top-level ES5 function constructor (`function Foo(){…}`
                // used as `x instanceof Foo`). The latter has no class entry,
                // so without a dynamic value codegen resolves `ty = "Foo"` to
                // class_id 0 and instanceof always returns false — which makes
                // the ubiquitous `if (!(this instanceof Foo)) return new Foo()`
                // guard recurse forever. Lower the function to its value and
                // route through `js_instanceof_dynamic`, which derives the same
                // `synthetic_class_id_for_function` that `new Foo()` stamps onto
                // the instance (see js_new_function_construct).
                if ctx.lookup_local(name).is_some()
                    || ctx.lookup_func(name).is_some()
                    || ctx.lookup_native_module(name).is_some()
                {
                    match lower_expr(ctx, &bin.right) {
                        Ok(e) => Some(Box::new(e)),
                        Err(_) => None,
                    }
                } else {
                    None
                }
            }
            ast::Expr::Member(_member) => {
                // Lower the member RHS to its value and route through
                // `js_instanceof_dynamic`. The pre-fix code only did this
                // for native modules (`Temporal.X`, builtin aliases) and
                // otherwise left codegen with the static `ty = "obj.prop"`
                // string, which it can't resolve to a class id for a
                // user-module member (`x instanceof sv.SemVer` where `sv`
                // is a default/namespace import) → class_id 0 → instanceof
                // always false (semver's `new SemVer(semVerObj)` clone path
                // hit this: `version instanceof SemVer` was false, so the
                // ctor mis-parsed the object as a string). `sv.SemVer`
                // lowers to the same class-ref value `const C = sv.SemVer`
                // produces, which the dynamic path resolves correctly; for
                // native modules it still derives the brand/synthetic id.
                match lower_expr(ctx, &bin.right) {
                    Ok(e) => Some(Box::new(e)),
                    Err(_) => None,
                }
            }
            // Any other right-hand side (a primitive literal like
            // `x instanceof true`, `this`, a call `x instanceof f()`,
            // a parenthesized/conditional class ref, …) is NOT a
            // statically-resolvable class name. The old `_ => "Object"`
            // `ty` substitution silently treated these as
            // `instanceof Object` and returned `false`; ECMAScript
            // requires evaluating the operand and throwing a TypeError
            // when it is not a constructor (`true instanceof true`,
            // `({}) instanceof this`). Lower the operand to a value and
            // route through `js_instanceof_dynamic`, which both resolves
            // every constructor shape and throws on a non-callable RHS.
            _ => match lower_expr(ctx, &bin.right) {
                Ok(e) => Some(Box::new(e)),
                Err(_) => None,
            },
        };
        return Ok(Expr::InstanceOf { expr, ty, ty_expr });
    }

    // `lowering_call_callee` flags the IMMEDIATE callee member so a direct
    // `JSON.parse(x)` / `Date.now()` takes the intrinsic fast path (and the
    // member-tail reroute-undo collapses the receiver to bare `GlobalGet(0)`).
    // A binary/logical expression in callee position — `(K || JSON.parse)(x)`,
    // `(a ?? B.method)(x)`, `(a && f)(x)` — is ITSELF the callee; its operands
    // are values, never the immediate callee member. The flag must not leak
    // into them: left set, the reroute-undo (#4596/#4627) would collapse a
    // nested builtin-namespace member (`JSON.parse`) to the value-less
    // intrinsic form `PropertyGet { GlobalGet(0), <method> }`, which has no
    // value materialization (the namespace name is gone) and lowers to
    // `undefined` — so the short-circuit result throws "value is not a
    // function" when called. A nested *call* operand (`(f() + 1)`) re-sets the
    // flag for its own callee, so direct intrinsic calls are unaffected.
    let prev_call_callee = ctx.lowering_call_callee;
    ctx.lowering_call_callee = false;
    let left = lower_expr(ctx, &bin.left);
    let right = lower_expr(ctx, &bin.right);
    ctx.lowering_call_callee = prev_call_callee;
    let left = Box::new(left?);
    let right = Box::new(right?);

    match bin.op {
        // Arithmetic
        ast::BinaryOp::Add => Ok(Expr::Binary {
            op: BinaryOp::Add,
            left,
            right,
        }),
        ast::BinaryOp::Sub => Ok(Expr::Binary {
            op: BinaryOp::Sub,
            left,
            right,
        }),
        ast::BinaryOp::Mul => Ok(Expr::Binary {
            op: BinaryOp::Mul,
            left,
            right,
        }),
        ast::BinaryOp::Div => Ok(Expr::Binary {
            op: BinaryOp::Div,
            left,
            right,
        }),
        ast::BinaryOp::Mod => Ok(Expr::Binary {
            op: BinaryOp::Mod,
            left,
            right,
        }),
        ast::BinaryOp::Exp => Ok(Expr::Binary {
            op: BinaryOp::Pow,
            left,
            right,
        }),

        // Comparison (treat == same as === for typed code)
        ast::BinaryOp::EqEq => {
            // Proxy/Reflect fold: `Reflect.getPrototypeOf(x) === <Class>.prototype`
            // always true in our model (we don't maintain real prototypes).
            // Same fold for `Object.getPrototypeOf(x) === <Class>.prototype`.
            if matches!(
                &*left,
                Expr::ReflectGetPrototypeOf(_) | Expr::ObjectGetPrototypeOf(_)
            ) && matches!(&*right, Expr::PropertyGet { property, .. } if property == "prototype")
            {
                return Ok(Expr::Bool(true));
            }
            Ok(Expr::Compare {
                op: CompareOp::LooseEq,
                left,
                right,
            })
        }
        ast::BinaryOp::EqEqEq => {
            if matches!(
                &*left,
                Expr::ReflectGetPrototypeOf(_) | Expr::ObjectGetPrototypeOf(_)
            ) && matches!(&*right, Expr::PropertyGet { property, .. } if property == "prototype")
            {
                return Ok(Expr::Bool(true));
            }
            Ok(Expr::Compare {
                op: CompareOp::Eq,
                left,
                right,
            })
        }
        ast::BinaryOp::NotEq => Ok(Expr::Compare {
            op: CompareOp::LooseNe,
            left,
            right,
        }),
        ast::BinaryOp::NotEqEq => Ok(Expr::Compare {
            op: CompareOp::Ne,
            left,
            right,
        }),
        ast::BinaryOp::Lt => Ok(Expr::Compare {
            op: CompareOp::Lt,
            left,
            right,
        }),
        ast::BinaryOp::LtEq => Ok(Expr::Compare {
            op: CompareOp::Le,
            left,
            right,
        }),
        ast::BinaryOp::Gt => Ok(Expr::Compare {
            op: CompareOp::Gt,
            left,
            right,
        }),
        ast::BinaryOp::GtEq => Ok(Expr::Compare {
            op: CompareOp::Ge,
            left,
            right,
        }),

        // Logical
        ast::BinaryOp::LogicalAnd => Ok(Expr::Logical {
            op: LogicalOp::And,
            left,
            right,
        }),
        ast::BinaryOp::LogicalOr => Ok(Expr::Logical {
            op: LogicalOp::Or,
            left,
            right,
        }),
        ast::BinaryOp::NullishCoalescing => Ok(Expr::Logical {
            op: LogicalOp::Coalesce,
            left,
            right,
        }),

        // Bitwise
        ast::BinaryOp::BitAnd => Ok(Expr::Binary {
            op: BinaryOp::BitAnd,
            left,
            right,
        }),
        ast::BinaryOp::BitOr => Ok(Expr::Binary {
            op: BinaryOp::BitOr,
            left,
            right,
        }),
        ast::BinaryOp::BitXor => Ok(Expr::Binary {
            op: BinaryOp::BitXor,
            left,
            right,
        }),
        ast::BinaryOp::LShift => Ok(Expr::Binary {
            op: BinaryOp::Shl,
            left,
            right,
        }),
        ast::BinaryOp::RShift => Ok(Expr::Binary {
            op: BinaryOp::Shr,
            left,
            right,
        }),
        ast::BinaryOp::ZeroFillRShift => Ok(Expr::Binary {
            op: BinaryOp::UShr,
            left,
            right,
        }),

        _ => Err(anyhow!("Unsupported binary operator: {:?}", bin.op)),
    }
}
