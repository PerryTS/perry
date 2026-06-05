use anyhow::{anyhow, bail, Result};
use perry_types::{LocalId, Type};
use swc_ecma_ast as ast;

use crate::analysis::*;
use crate::destructuring::*;
use crate::ir::*;
use crate::lower::{
    collect_for_of_pattern_leaves, emit_for_of_pattern_binding, lower_expr, LoweringContext,
};
use crate::lower_patterns::*;
use crate::lower_types::*;

use super::*;

fn collect_enum_members(enum_decl: &ast::TsEnumDecl) -> (String, Vec<EnumMember>) {
    let name = enum_decl.id.sym.to_string();
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

    (name, members)
}

pub(crate) fn pre_register_enum_decl(
    ctx: &mut LoweringContext,
    enum_decl: &ast::TsEnumDecl,
) -> Result<()> {
    let (name, members) = collect_enum_members(enum_decl);
    if ctx.lookup_enum(&name).is_some() {
        return Ok(());
    }

    let enum_id = ctx.fresh_enum();
    let member_values: Vec<(String, EnumValue)> = members
        .iter()
        .map(|m| (m.name.clone(), m.value.clone()))
        .collect();
    ctx.define_enum(name.clone(), enum_id, member_values);
    if ctx.lookup_local(&name).is_none() {
        ctx.define_local(name, Type::Any);
    }
    Ok(())
}

fn push_or_replace_prop(props: &mut Vec<(String, Expr)>, key: String, value: Expr) {
    if let Some((_, existing)) = props
        .iter_mut()
        .find(|(existing_key, _)| existing_key == &key)
    {
        *existing = value;
    } else {
        props.push((key, value));
    }
}

pub(crate) fn enum_runtime_object_expr(en: &Enum) -> Expr {
    let mut props = Vec::new();
    for member in &en.members {
        match &member.value {
            EnumValue::Number(n) => {
                push_or_replace_prop(&mut props, member.name.clone(), Expr::Number(*n as f64));
                push_or_replace_prop(&mut props, n.to_string(), Expr::String(member.name.clone()));
            }
            EnumValue::String(s) => {
                push_or_replace_prop(&mut props, member.name.clone(), Expr::String(s.clone()));
            }
        }
    }
    Expr::Object(props)
}

pub fn lower_enum_decl(
    ctx: &mut LoweringContext,
    enum_decl: &ast::TsEnumDecl,
    is_exported: bool,
) -> Result<Enum> {
    pre_register_enum_decl(ctx, enum_decl)?;
    let name = enum_decl.id.sym.to_string();
    let (enum_id, member_values) = ctx
        .lookup_enum(&name)
        .ok_or_else(|| anyhow!("enum '{}' was not registered", name))?;
    let members = member_values
        .iter()
        .map(|(name, value)| EnumMember {
            name: name.clone(),
            value: value.clone(),
        })
        .collect();

    Ok(Enum {
        id: enum_id,
        name,
        members,
        is_exported,
    })
}
