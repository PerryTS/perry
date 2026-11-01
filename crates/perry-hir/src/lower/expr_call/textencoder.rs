//! TextEncoder.encode/TextDecoder.decode on inline expressions.
//!
//! Extracted from `expr_call/mod.rs` as a mechanical move.

use crate::types::Type;
use anyhow::Result;
use swc_ecma_ast as ast;

use crate::ir::*;

use super::super::{lower_expr, LoweringContext};

/// Keep the receiver and property lookup: encode/encodeInto are ordinary
/// writable methods. A receiverless intrinsic cannot represent replacement
/// and also discards receiver-construction effects on inline calls.
pub(super) fn encoder_call(
    receiver: Expr,
    method: &str,
    args: Vec<Expr>,
    call: &ast::CallExpr,
) -> Expr {
    Expr::Call {
        callee: Box::new(Expr::PropertyGet {
            object: Box::new(receiver),
            property: method.into(),
            byte_offset: call.span.lo.0,
            byte_end: call.span.hi.0,
        }),
        args,
        type_args: vec![],
        byte_offset: call.span.lo.0,
        byte_end: call.span.hi.0,
    }
}

fn new_callee_name<'a>(ctx: &LoweringContext, new_expr: &'a ast::NewExpr) -> Option<&'a str> {
    match new_expr.callee.as_ref() {
        ast::Expr::Ident(class_ident) => Some(class_ident.sym.as_ref()),
        ast::Expr::Member(member)
            if matches!(member.obj.as_ref(), ast::Expr::Ident(obj) if obj.sym.as_ref() == "globalThis")
                && ctx.lookup_local("globalThis").is_none() =>
        {
            match &member.prop {
                ast::MemberProp::Ident(prop_ident) => Some(prop_ident.sym.as_ref()),
                _ => None,
            }
        }
        _ => None,
    }
}

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
                    if let Some(class_name) = new_callee_name(ctx, new_expr) {
                        if class_name == "TextEncoder"
                            && matches!(method_name, "encode" | "encodeInto")
                        {
                            let receiver = lower_expr(ctx, &member.obj)?;
                            return Ok(Ok(encoder_call(receiver, method_name, args, call)));
                        }
                        if class_name == "TextDecoder" && method_name == "decode" {
                            let decoder = super::super::expr_new::lower_text_decoder_new(
                                ctx,
                                new_expr.args.as_deref(),
                            )?;
                            let input = if !args.is_empty() {
                                args.into_iter().next().unwrap()
                            } else {
                                Expr::Undefined
                            };
                            return Ok(Ok(Expr::TextDecoderDecode {
                                decoder: Box::new(decoder),
                                input: Box::new(input),
                            }));
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
                    if is_text_encoder && matches!(method_name, "encode" | "encodeInto") {
                        let receiver = lower_expr(ctx, &member.obj)?;
                        return Ok(Ok(encoder_call(receiver, method_name, args, call)));
                    }
                    let is_text_decoder = ctx
                        .lookup_local_type(&obj_name)
                        .map(|ty| matches!(ty, Type::Named(name) if name == "TextDecoder"))
                        .unwrap_or(false);
                    if is_text_decoder && method_name == "decode" {
                        let decoder = lower_expr(ctx, &member.obj)?;
                        let input = if !args.is_empty() {
                            args.into_iter().next().unwrap()
                        } else {
                            Expr::Undefined
                        };
                        return Ok(Ok(Expr::TextDecoderDecode {
                            decoder: Box::new(decoder),
                            input: Box::new(input),
                        }));
                    }
                }
            }
        }
    }

    Ok(Err(args))
}
