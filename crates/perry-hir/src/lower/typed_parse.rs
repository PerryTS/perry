//! `JSON.parse<T>` typed-parse helpers.
//!
//! Extracted from `lower/mod.rs`. These helpers walk a TypeScript type
//! argument either to extract field names in source order or to resolve
//! a structural type for codegen. Consumed by the call-lowering
//! `JSON.parse` arms (see `lower/expr_call/*`).
//!
//! Visibility note: bumped from `pub(super)` to `pub(crate)` so the
//! mod.rs named re-export can propagate the symbols across the crate.

use crate::types::Type;

use super::*;

pub(crate) fn extract_typed_parse_source_order(
    ts_type: &swc_ecma_ast::TsType,
    ctx: &LoweringContext,
) -> Option<Vec<String>> {
    use swc_ecma_ast as ast;
    match ts_type {
        ast::TsType::TsArrayType(arr) => extract_typed_parse_source_order(&arr.elem_type, ctx),
        ast::TsType::TsTypeLit(lit) => {
            let mut keys = Vec::with_capacity(lit.members.len());
            for member in &lit.members {
                if let ast::TsTypeElement::TsPropertySignature(prop) = member {
                    if let ast::Expr::Ident(ident) = prop.key.as_ref() {
                        keys.push(ident.sym.to_string());
                    } else {
                        return None;
                    }
                }
            }
            if keys.is_empty() {
                None
            } else {
                Some(keys)
            }
        }
        ast::TsType::TsTypeRef(tref) => {
            // `Array<T>` — recurse on the element type argument.
            if let Some(type_params) = &tref.type_params {
                let name = match &tref.type_name {
                    ast::TsEntityName::Ident(i) => i.sym.as_ref(),
                    _ => return None,
                };
                if name == "Array" && type_params.params.len() == 1 {
                    return extract_typed_parse_source_order(&type_params.params[0], ctx);
                }
            }
            // Named interface reference — look up the source-order
            // field list recorded by `lower_interface_decl`.
            let name = match &tref.type_name {
                ast::TsEntityName::Ident(i) => i.sym.to_string(),
                _ => return None,
            };
            ctx.interface_source_keys.get(&name).cloned()
        }
        _ => None,
    }
}

/// Issue #179 typed-parse: fully resolve a `JSON.parse<T>` type argument
/// down to a structural form codegen can use (ObjectType with fields /
/// Array of object). Named/interface references are expanded via the
/// lowering context's type-alias table. Unresolvable references collapse
/// to `Type::Any` so the caller falls through to the generic parser.
pub(crate) fn resolve_typed_parse_ty(ctx: &LoweringContext, ty: Type) -> Type {
    match ty {
        Type::Named(ref name) => {
            // Interface reference? Expand to ObjectType from the
            // typed-parse side table (populated by `lower_interface_decl`).
            if let Some(obj) = ctx.interface_object_types.get(name) {
                return Type::Object(obj.clone());
            }
            // Type alias? Expand and recurse.
            match ctx.resolve_type_alias(name) {
                Some(resolved) => resolve_typed_parse_ty(ctx, resolved),
                None => Type::Any,
            }
        }
        Type::Array(elem) => {
            let resolved = resolve_typed_parse_ty(ctx, *elem);
            Type::Array(Box::new(resolved))
        }
        Type::Generic { base, type_args } if base == "Array" && type_args.len() == 1 => {
            let resolved = resolve_typed_parse_ty(ctx, type_args.into_iter().next().unwrap());
            Type::Array(Box::new(resolved))
        }
        // Object/primitive/tuple types pass through unchanged.
        other => other,
    }
}
