//! TextEncoder.encode/TextDecoder.decode on inline expressions.
//!
//! Extracted from `expr_call/mod.rs` as a mechanical move.

use anyhow::{anyhow, Result};
use perry_types::{LocalId, Type};
use swc_ecma_ast as ast;

use crate::ir::*;
use crate::lower_types::extract_ts_type_with_ctx;

use super::super::{
    extract_typed_parse_source_order, is_generator_call_expr, is_widget_modifier_name, lower_expr,
    resolve_typed_parse_ty, LoweringContext,
};

pub(super) fn try_textencoder_decoder(
    ctx: &mut LoweringContext,
    call: &ast::CallExpr,
    args: Vec<Expr>,
) -> Result<Result<Expr, Vec<Expr>>> {
    // TextEncoder.encode() / TextDecoder.decode() on inline expressions
    // e.g., new TextEncoder().encode("hello"), new TextDecoder().decode(buf)
    if let ast::Callee::Expr(expr) = &call.callee {
        if let ast::Expr::Member(member) = expr.as_ref() {
            if let ast::MemberProp::Ident(method_ident) = &member.prop {
                let method_name = method_ident.sym.as_ref();
                // Check if the receiver is new TextEncoder() or new TextDecoder()
                if let ast::Expr::New(new_expr) = member.obj.as_ref() {
                    if let ast::Expr::Ident(class_ident) = new_expr.callee.as_ref() {
                        let class_name = class_ident.sym.as_ref();
                        if class_name == "TextEncoder" && method_name == "encode" {
                            let str_arg = if !args.is_empty() {
                                args.into_iter().next().unwrap()
                            } else {
                                Expr::String(String::new())
                            };
                            return Ok(Ok(Expr::TextEncoderEncode(Box::new(str_arg))));
                        }
                        if class_name == "TextDecoder" && method_name == "decode" {
                            if !args.is_empty() {
                                return Ok(Ok(Expr::TextDecoderDecode(Box::new(
                                    args.into_iter().next().unwrap(),
                                ))));
                            } else {
                                return Ok(Ok(Expr::String(String::new())));
                            }
                        }
                    }
                }
                // Also check for local variable typed as TextEncoder/TextDecoder
                if let ast::Expr::Ident(obj_ident) = member.obj.as_ref() {
                    let obj_name = obj_ident.sym.to_string();
                    let is_text_encoder = ctx
                        .lookup_local_type(&obj_name)
                        .map(|ty| matches!(ty, Type::Named(name) if name == "TextEncoder"))
                        .unwrap_or(false);
                    if is_text_encoder && method_name == "encode" {
                        let str_arg = if !args.is_empty() {
                            args.into_iter().next().unwrap()
                        } else {
                            Expr::String(String::new())
                        };
                        return Ok(Ok(Expr::TextEncoderEncode(Box::new(str_arg))));
                    }
                    let is_text_decoder = ctx
                        .lookup_local_type(&obj_name)
                        .map(|ty| matches!(ty, Type::Named(name) if name == "TextDecoder"))
                        .unwrap_or(false);
                    if is_text_decoder && method_name == "decode" {
                        if !args.is_empty() {
                            return Ok(Ok(Expr::TextDecoderDecode(Box::new(
                                args.into_iter().next().unwrap(),
                            ))));
                        } else {
                            return Ok(Ok(Expr::String(String::new())));
                        }
                    }
                }
            }
        }
    }

    Ok(Err(args))
}
