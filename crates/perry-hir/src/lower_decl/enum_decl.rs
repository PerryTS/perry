use anyhow::Result;
use swc_ecma_ast as ast;

use crate::ir::*;
use crate::lower::LoweringContext;
use crate::types::{LocalId, Type};

pub fn lower_enum_decl(
    ctx: &mut LoweringContext,
    enum_decl: &ast::TsEnumDecl,
    is_exported: bool,
) -> Result<Enum> {
    let name = enum_decl.id.sym.to_string();
    let members = compute_enum_members(enum_decl, ctx);

    // #4510: an enum referenced before its textual declaration is
    // pre-registered (see `pre_register_module_enums`) so forward references
    // resolve. If a registration already exists, reuse its id instead of
    // minting a fresh one — that avoids a duplicate `ctx.enums` entry and
    // keeps the EnumMember lookups pointing at the same id.
    let enum_id = if let Some((existing_id, _)) = ctx.lookup_enum(&name) {
        existing_id
    } else {
        let id = ctx.fresh_enum();
        let member_values: Vec<(String, EnumValue)> = members
            .iter()
            .map(|m| (m.name.clone(), m.value.clone()))
            .collect();
        ctx.define_enum(name.clone(), id, member_values);
        id
    };

    Ok(Enum {
        id: enum_id,
        name,
        members,
        is_exported,
    })
}

/// The enum's runtime object as tsc builds it: every member forward
/// (`E.A === 0`) plus a reverse entry for each numeric member (`E[0] === "A"`).
/// String members get no reverse entry (string enums are one-directional).
pub(crate) fn enum_runtime_object(members: &[(String, EnumValue)]) -> Expr {
    let mut fields: Vec<(String, Expr)> = Vec::new();
    for (name, value) in members {
        match value {
            EnumValue::Number(n) => {
                fields.push((name.clone(), Expr::Number(*n as f64)));
                fields.push((n.to_string(), Expr::String(name.clone())));
            }
            EnumValue::String(s) => {
                fields.push((name.clone(), Expr::String(s.clone())));
            }
        }
    }
    Expr::object_literal(fields)
}

/// Bind an enum's own name to its runtime object at the declaration position,
/// as tsc's `var E; (function (E) { ... })(E || (E = {}))` does, so the enum is
/// a value: `const o = E`, `export { E }`, `Debug.formatEnum(v, E)`.
/// `E.Member` keeps folding to the constant. A module-level enum reuses the
/// `var`-like local `pre_register_enum` created for forward references; a
/// function-body enum binds a fresh local.
pub(crate) fn enum_value_binding(
    ctx: &mut LoweringContext,
    name: &str,
    module_level: bool,
) -> Option<(LocalId, Stmt)> {
    let members = ctx.lookup_enum(name)?.1.to_vec();
    let id = if module_level && ctx.pre_registered_module_vars.remove(name) {
        ctx.pre_registered_module_var_decls.remove(name);
        ctx.lookup_local(name)?
    } else {
        ctx.define_local(name.to_string(), Type::Any)
    };
    let stmt = Stmt::Let {
        id,
        name: name.to_string(),
        ty: Type::Any,
        mutable: true,
        init: Some(enum_runtime_object(&members)),
    };
    Some((id, stmt))
}

/// Compute an enum's members (names + values) without touching the lowering
/// context. Pure so it can run in the forward-reference pre-scan
/// (`pre_register_module_enums`) and again at the real declaration site and
/// produce identical results.
pub(crate) fn compute_enum_members(
    enum_decl: &ast::TsEnumDecl,
    ctx: &LoweringContext,
) -> Vec<EnumMember> {
    let enum_name = enum_decl.id.sym.to_string();
    let mut members: Vec<EnumMember> = Vec::new();
    let mut next_value: i64 = 0;

    for member in &enum_decl.members {
        // Get member name
        let member_name = match &member.id {
            ast::TsEnumMemberId::Ident(ident) => ident.sym.to_string(),
            ast::TsEnumMemberId::Str(s) => s.value.as_str().unwrap_or("").to_string(),
        };

        // Get member value: a constant enum expression, as tsc evaluates it
        // (`FirstToken = Unknown`, `Let = 1 << 0`, `BlockScoped = Let | Const`).
        // A non-constant initializer keeps the auto-increment fallback.
        let value = match member
            .init
            .as_deref()
            .and_then(|init| eval_enum_init(init, &enum_name, &members, ctx))
        {
            Some(value) => value,
            None => EnumValue::Number(next_value),
        };
        if let EnumValue::Number(v) = value {
            next_value = v + 1;
        }

        members.push(EnumMember {
            name: member_name,
            value,
        });
    }

    members
}

/// tsc's constant enum expression: literals, no-substitution templates,
/// parentheses, unary `+ - ~`, binary arithmetic / shift / bitwise operators
/// (int32 semantics as in JS) and string `+`, and references to earlier
/// members of this enum (bare, `E.M`, `E["M"]`) or members of another
/// already-registered enum (`Other.M`).
fn eval_enum_init(
    expr: &ast::Expr,
    enum_name: &str,
    members: &[EnumMember],
    ctx: &LoweringContext,
) -> Option<EnumValue> {
    let own = |name: &str| {
        members
            .iter()
            .rev()
            .find(|m| m.name == name)
            .map(|m| m.value.clone())
    };
    let member_of = |object: &str, name: &str| {
        if object == enum_name {
            own(name)
        } else {
            ctx.lookup_enum_member(object, name).cloned()
        }
    };
    let num = |v: f64| (v.is_finite() && v.fract() == 0.0).then(|| EnumValue::Number(v as i64));
    let int32 = |v: i64| v as i32;
    match expr {
        ast::Expr::Lit(ast::Lit::Num(n)) => num(n.value),
        ast::Expr::Lit(ast::Lit::Str(s)) => Some(EnumValue::String(
            s.value.as_str().unwrap_or("").to_string(),
        )),
        ast::Expr::Tpl(tpl) if tpl.exprs.is_empty() && tpl.quasis.len() == 1 => {
            let cooked = tpl.quasis[0].cooked.as_ref()?;
            Some(EnumValue::String(cooked.as_str()?.to_string()))
        }
        ast::Expr::Paren(paren) => eval_enum_init(&paren.expr, enum_name, members, ctx),
        ast::Expr::Ident(ident) => own(ident.sym.as_ref()),
        ast::Expr::Member(member) => {
            let ast::Expr::Ident(object) = member.obj.as_ref() else {
                return None;
            };
            let name = match &member.prop {
                ast::MemberProp::Ident(prop) => prop.sym.to_string(),
                ast::MemberProp::Computed(computed) => match computed.expr.as_ref() {
                    ast::Expr::Lit(ast::Lit::Str(s)) => s.value.as_str()?.to_string(),
                    _ => return None,
                },
                ast::MemberProp::PrivateName(_) => return None,
            };
            member_of(object.sym.as_ref(), &name)
        }
        ast::Expr::Unary(unary) => {
            let EnumValue::Number(v) = eval_enum_init(&unary.arg, enum_name, members, ctx)? else {
                return None;
            };
            match unary.op {
                ast::UnaryOp::Minus => num(-(v as f64)),
                ast::UnaryOp::Plus => Some(EnumValue::Number(v)),
                ast::UnaryOp::Tilde => Some(EnumValue::Number(!int32(v) as i64)),
                _ => None,
            }
        }
        ast::Expr::Bin(bin) => {
            let left = eval_enum_init(&bin.left, enum_name, members, ctx)?;
            let right = eval_enum_init(&bin.right, enum_name, members, ctx)?;
            let (a, b) = match (&left, &right) {
                (EnumValue::Number(a), EnumValue::Number(b)) => (*a, *b),
                _ if bin.op == ast::BinaryOp::Add => {
                    let text = |v: &EnumValue| match v {
                        EnumValue::Number(n) => n.to_string(),
                        EnumValue::String(s) => s.clone(),
                    };
                    return Some(EnumValue::String(text(&left) + &text(&right)));
                }
                _ => return None,
            };
            let shift = (b as u32) & 31;
            match bin.op {
                ast::BinaryOp::Add => a.checked_add(b).map(EnumValue::Number),
                ast::BinaryOp::Sub => a.checked_sub(b).map(EnumValue::Number),
                ast::BinaryOp::Mul => a.checked_mul(b).map(EnumValue::Number),
                ast::BinaryOp::Div => num(a as f64 / b as f64),
                ast::BinaryOp::Mod => num(a as f64 % b as f64),
                ast::BinaryOp::LShift => {
                    Some(EnumValue::Number(int32(a).wrapping_shl(shift) as i64))
                }
                ast::BinaryOp::RShift => Some(EnumValue::Number((int32(a) >> shift) as i64)),
                ast::BinaryOp::ZeroFillRShift => {
                    Some(EnumValue::Number(((int32(a) as u32) >> shift) as i64))
                }
                ast::BinaryOp::BitAnd => Some(EnumValue::Number((int32(a) & int32(b)) as i64)),
                ast::BinaryOp::BitOr => Some(EnumValue::Number((int32(a) | int32(b)) as i64)),
                ast::BinaryOp::BitXor => Some(EnumValue::Number((int32(a) ^ int32(b)) as i64)),
                _ => None,
            }
        }
        _ => None,
    }
}
