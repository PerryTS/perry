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
        // A later declaration merging into the same enum (`enum E { A }
        // enum E { B = 1 }`) adds its members; the pre-registration of this
        // very declaration already holds them.
        ctx.extend_enum(
            &name,
            members
                .iter()
                .map(|m| (m.name.clone(), m.value.clone()))
                .collect(),
        );
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
/// tsc emits a constant -0 as +0, but retains -0 during constant evaluation
/// (e.g. `enum E { Z = -0, R = 1 / Z }` emits `Z = 0, R = -Infinity`).
pub(crate) fn enum_runtime_object(members: &[(String, EnumValue)]) -> Expr {
    let mut fields: Vec<(String, Expr)> = Vec::new();
    for (name, value) in members {
        match value {
            EnumValue::Number(n) => {
                fields.push((name.clone(), Expr::Number(if *n == 0.0 { 0.0 } else { *n })));
                fields.push((crate::js_number_to_string(*n), Expr::String(name.clone())));
            }
            EnumValue::String(s) => {
                fields.push((name.clone(), Expr::String(s.clone())));
            }
            EnumValue::Computed => unreachable!("computed enum reads use the runtime binding"),
        }
    }
    Expr::object_literal(fields)
}

/// Bind an enum's own name to its runtime object at the declaration position,
/// as tsc's `var E; (function (E) { ... })(E || (E = {}))` does, so the enum is
/// a value: `const o = E`, `export { E }`, `Debug.formatEnum(v, E)`.
/// `E.Member` keeps folding to the constant. A module-level enum reuses the
/// `var`-like local `pre_register_enum` created for forward references; a
/// function-body enum binds a fresh local. A later module-level declaration
/// merging into an enum already bound in this scope stores its own members
/// (`own`) and their reverse entries onto that object, as tsc's IIFE over
/// the existing `E` does.
pub(crate) fn enum_value_binding(
    ctx: &mut LoweringContext,
    name: &str,
    module_level: bool,
    own: &[EnumMember],
    decl: &ast::TsEnumDecl,
) -> Result<Option<(LocalId, Stmt)>> {
    let existing = if module_level {
        ctx.enum_object_locals
            .get(name)
            .copied()
            .filter(|id| ctx.lookup_local(name) == Some(*id))
    } else {
        None
    };
    let id = if let Some(id) = existing {
        id
    } else if module_level && ctx.pre_registered_module_vars.remove(name) {
        ctx.pre_registered_module_var_decls.remove(name);
        match ctx.lookup_local(name) {
            Some(id) => id,
            None => return Ok(None),
        }
    } else {
        ctx.define_local(name.to_string(), Type::Any)
    };

    // Constant enums keep their literal birth. Computed members evaluate once,
    // in declaration order, then use the assignment result for their reverse key.
    let computed = own.iter().any(|m| matches!(m.value, EnumValue::Computed));
    let stmt = if existing.is_none() && !computed {
        Stmt::Let {
            id,
            name: name.to_string(),
            ty: Type::Any,
            mutable: true,
            init: Some(enum_runtime_object(
                &own.iter()
                    .map(|m| (m.name.clone(), m.value.clone()))
                    .collect::<Vec<_>>(),
            )),
        }
    } else {
        let mut stores = Vec::new();
        if existing.is_none() {
            stores.push(Expr::LocalSet(
                id,
                Box::new(Expr::object_literal(Vec::new())),
            ));
        }
        for (member, ast_member) in own.iter().zip(&decl.members) {
            let value = match &member.value {
                EnumValue::Number(n) => Expr::Number(if *n == 0.0 { 0.0 } else { *n }),
                EnumValue::String(s) => Expr::String(s.clone()),
                EnumValue::Computed => {
                    // Enum members form an implicit lexical scope for an initializer.
                    let enclosing = ctx
                        .enum_initializer_scope
                        .replace((name.to_string(), ctx.locals.len()));
                    let value = crate::lower::lower_expr(
                        ctx,
                        ast_member
                            .init
                            .as_deref()
                            .expect("computed member has initializer"),
                    );
                    ctx.enum_initializer_scope = enclosing;
                    value?
                }
            };
            let forward = Expr::IndexSet {
                object: Box::new(Expr::LocalGet(id)),
                index: Box::new(Expr::String(member.name.clone())),
                value: Box::new(value),
                byte_offset: 0,
                byte_end: 0,
            };
            if matches!(member.value, EnumValue::Computed) {
                // E[E.A = init] = "A": [[Set]] can call an inherited setter;
                // its assignment result is init, independent of a later E.A read.
                stores.push(Expr::IndexSet {
                    object: Box::new(Expr::LocalGet(id)),
                    index: Box::new(forward),
                    value: Box::new(Expr::String(member.name.clone())),
                    byte_offset: 0,
                    byte_end: 0,
                });
                continue;
            }
            stores.push(forward);
            if !matches!(member.value, EnumValue::String(_)) {
                let key = match &member.value {
                    EnumValue::Number(n) => Expr::String(crate::js_number_to_string(*n)),
                    EnumValue::Computed => unreachable!(),
                    EnumValue::String(_) => unreachable!(),
                };
                stores.push(Expr::IndexSet {
                    object: Box::new(Expr::LocalGet(id)),
                    index: Box::new(key),
                    value: Box::new(Expr::String(member.name.clone())),
                    byte_offset: 0,
                    byte_end: 0,
                });
            }
        }
        if existing.is_some() {
            Stmt::Expr(Expr::Sequence(stores))
        } else {
            stores.push(Expr::LocalGet(id));
            Stmt::Let {
                id,
                name: name.to_string(),
                ty: Type::Any,
                mutable: true,
                init: Some(Expr::Sequence(stores)),
            }
        }
    };
    if module_level {
        ctx.enum_object_locals.insert(name.to_string(), id);
    }
    Ok(Some((id, stmt)))
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
    let mut next_value: f64 = 0.0;

    for member in &enum_decl.members {
        // Get member name
        let member_name = match &member.id {
            ast::TsEnumMemberId::Ident(ident) => ident.sym.to_string(),
            ast::TsEnumMemberId::Str(s) => s.value.as_str().unwrap_or("").to_string(),
        };

        // Get member value: a constant enum expression, as tsc evaluates it
        // (`FirstToken = Unknown`, `Let = 1 << 0`, `BlockScoped = Let | Const`).
        // A non-constant initializer is evaluated at the declaration site.
        let value = match member
            .init
            .as_deref()
            .and_then(|init| eval_enum_init(init, &enum_name, &members, ctx))
        {
            Some(value) => value,
            None if member.init.is_some() => EnumValue::Computed,
            None => EnumValue::Number(next_value),
        };
        if let EnumValue::Number(v) = value {
            next_value = v + 1.0;
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
            own(name).or_else(|| ctx.lookup_enum_member(object, name).cloned())
        } else {
            ctx.lookup_enum_member(object, name).cloned()
        }
    };
    let num = |v: f64| Some(EnumValue::Number(v));
    let int32 = crate::number::js_to_int32;
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
        ast::Expr::Ident(ident) => own(ident.sym.as_ref()).or_else(|| {
            ctx.lookup_enum_member(enum_name, ident.sym.as_ref())
                .cloned()
        }),
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
                ast::UnaryOp::Minus => num(-v),
                ast::UnaryOp::Plus => Some(EnumValue::Number(v)),
                ast::UnaryOp::Tilde => Some(EnumValue::Number(!int32(v) as f64)),
                _ => None,
            }
        }
        ast::Expr::Bin(bin) => {
            let left = eval_enum_init(&bin.left, enum_name, members, ctx)?;
            let right = eval_enum_init(&bin.right, enum_name, members, ctx)?;
            let (a, b) = match (&left, &right) {
                (EnumValue::Number(a), EnumValue::Number(b)) => (*a, *b),
                _ if matches!(left, EnumValue::Computed)
                    || matches!(right, EnumValue::Computed) =>
                {
                    return None
                }
                _ if bin.op == ast::BinaryOp::Add => {
                    let text = |v: &EnumValue| match v {
                        EnumValue::Number(n) => crate::js_number_to_string(*n),
                        EnumValue::String(s) => s.clone(),
                        EnumValue::Computed => unreachable!(),
                    };
                    return Some(EnumValue::String(text(&left) + &text(&right)));
                }
                _ => return None,
            };
            let shift = (int32(b) as u32) & 31;
            match bin.op {
                ast::BinaryOp::Add => num(a + b),
                ast::BinaryOp::Sub => num(a - b),
                ast::BinaryOp::Mul => num(a * b),
                ast::BinaryOp::Exp => num(if a.abs() == 1.0 && b.is_infinite() {
                    f64::NAN
                } else {
                    a.powf(b)
                }),
                ast::BinaryOp::Div => num(a / b),
                ast::BinaryOp::Mod => num(a % b),
                ast::BinaryOp::LShift => {
                    Some(EnumValue::Number(int32(a).wrapping_shl(shift) as f64))
                }
                ast::BinaryOp::RShift => Some(EnumValue::Number((int32(a) >> shift) as f64)),
                ast::BinaryOp::ZeroFillRShift => {
                    Some(EnumValue::Number(((int32(a) as u32) >> shift) as f64))
                }
                ast::BinaryOp::BitAnd => Some(EnumValue::Number((int32(a) & int32(b)) as f64)),
                ast::BinaryOp::BitOr => Some(EnumValue::Number((int32(a) | int32(b)) as f64)),
                ast::BinaryOp::BitXor => Some(EnumValue::Number((int32(a) ^ int32(b)) as f64)),
                _ => None,
            }
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enum_constant_numbers_follow_javascript() {
        let parsed = perry_parser::parse_typescript(
            "enum E { A = 1.5, B = -0.25, C = A * 2, D = 1e21, E = 0.1 + 0.2, Z = -0, I = 1 / 0, N = 0 / 0, U = 4294967297.5 | 0, S = -1 >>> 0, T = 2 ** 3, R = 5.5 % 2, Next }",
            "enum-number.ts",
        ).unwrap();
        let ast::ModuleItem::Stmt(ast::Stmt::Decl(ast::Decl::TsEnum(decl))) = &parsed.body[0]
        else {
            panic!("enum")
        };
        let mut ctx = LoweringContext::new("enum-number.ts");
        let members = compute_enum_members(decl, &mut ctx);
        let expected = [
            1.5,
            -0.25,
            3.0,
            1e21,
            0.1 + 0.2,
            -0.0,
            f64::INFINITY,
            f64::NAN,
            1.0,
            4294967295.0,
            8.0,
            1.5,
            2.5,
        ];
        for (member, expected) in members.iter().zip(expected) {
            let EnumValue::Number(actual) = member.value else {
                panic!("number")
            };
            assert!(
                actual.to_bits() == expected.to_bits() || (actual.is_nan() && expected.is_nan()),
                "{}: {actual} != {expected}",
                member.name
            );
        }
    }
}
