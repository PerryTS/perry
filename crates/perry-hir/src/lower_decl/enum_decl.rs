use anyhow::Result;
use swc_ecma_ast as ast;

use crate::ir::*;
use crate::lower::LoweringContext;

pub fn lower_enum_decl(
    ctx: &mut LoweringContext,
    enum_decl: &ast::TsEnumDecl,
    is_exported: bool,
) -> Result<Enum> {
    let name = enum_decl.id.sym.to_string();
    let members = compute_enum_members(enum_decl);

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

pub(crate) fn enum_runtime_let(ctx: &mut LoweringContext, en: &Enum) -> Stmt {
    let id = ctx
        .lookup_local(&en.name)
        .unwrap_or_else(|| ctx.define_local(en.name.clone(), perry_types::Type::Any));
    let mut properties = Vec::new();
    for member in &en.members {
        match &member.value {
            EnumValue::Number(value) => {
                properties.push((member.name.clone(), Expr::Number(*value as f64)));
                properties.push((value.to_string(), Expr::String(member.name.clone())));
            }
            EnumValue::String(value) => {
                properties.push((member.name.clone(), Expr::String(value.clone())));
            }
        }
    }
    Stmt::Let {
        id,
        name: en.name.clone(),
        ty: perry_types::Type::Any,
        mutable: true,
        init: Some(Expr::Object(properties)),
    }
}

/// Compute an enum's members (names + values) without touching the lowering
/// context. Pure so it can run in the forward-reference pre-scan
/// (`pre_register_module_enums`) and again at the real declaration site and
/// produce identical results.
pub(crate) fn compute_enum_members(enum_decl: &ast::TsEnumDecl) -> Vec<EnumMember> {
    let mut members = Vec::new();
    let mut next_value: i64 = 0;

    for member in &enum_decl.members {
        // Get member name
        let member_name = match &member.id {
            ast::TsEnumMemberId::Ident(ident) => ident.sym.to_string(),
            ast::TsEnumMemberId::Str(s) => s.value.as_str().unwrap_or("").to_string(),
        };

        // Get member value
        let value = if let Some(ref init) = member.init {
            match init.as_ref() {
                ast::Expr::Lit(ast::Lit::Num(n)) => {
                    let v = n.value as i64;
                    next_value = v + 1;
                    EnumValue::Number(v)
                }
                ast::Expr::Lit(ast::Lit::Str(s)) => {
                    EnumValue::String(s.value.as_str().unwrap_or("").to_string())
                }
                ast::Expr::Unary(unary) if unary.op == ast::UnaryOp::Minus => {
                    // Handle negative numbers like -1
                    if let ast::Expr::Lit(ast::Lit::Num(n)) = unary.arg.as_ref() {
                        let v = -(n.value as i64);
                        next_value = v + 1;
                        EnumValue::Number(v)
                    } else {
                        // Default to auto-increment
                        let v = next_value;
                        next_value += 1;
                        EnumValue::Number(v)
                    }
                }
                _ => {
                    // For complex expressions, default to auto-increment
                    let v = next_value;
                    next_value += 1;
                    EnumValue::Number(v)
                }
            }
        } else {
            // Auto-increment
            let v = next_value;
            next_value += 1;
            EnumValue::Number(v)
        };

        members.push(EnumMember {
            name: member_name,
            value,
        });
    }

    members
}
