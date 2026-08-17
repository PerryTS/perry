use anyhow::Result;
use swc_ecma_ast as ast;

use crate::lower::LoweringContext;

fn native_scalar_conversion_name<'a>(
    ctx: &'a LoweringContext,
    call: &'a ast::CallExpr,
) -> Option<&'a str> {
    let ast::Callee::Expr(callee) = &call.callee else {
        return None;
    };
    let (module, method) = match callee.as_ref() {
        ast::Expr::Ident(ident) => {
            let (module, method) = ctx.lookup_native_module(ident.sym.as_ref())?;
            (module, method?)
        }
        ast::Expr::Member(member) => {
            let ast::Expr::Ident(namespace) = member.obj.as_ref() else {
                return None;
            };
            let ast::MemberProp::Ident(method) = &member.prop else {
                return None;
            };
            let (module, imported_method) = ctx.lookup_native_module(namespace.sym.as_ref())?;
            if imported_method.is_some() {
                return None;
            }
            (module, method.sym.as_ref())
        }
        _ => return None,
    };
    (module == "perry/native"
        && matches!(
            method,
            "u8" | "i32" | "i64" | "u32" | "u64" | "usize" | "f32" | "f64"
        ))
    .then_some(method)
}

/// Enforce the source-level signature before the call reaches the generic
/// native-module dispatcher. That dispatcher derives its LLVM signature from
/// supplied arguments, so accepting missing/spread arguments here would be
/// both a contract violation and an ABI mismatch with the runtime helper.
pub(crate) fn validate_native_scalar_conversion_call(
    ctx: &LoweringContext,
    call: &ast::CallExpr,
    has_spread: bool,
) -> Result<()> {
    let Some(name) = native_scalar_conversion_name(ctx, call) else {
        return Ok(());
    };
    if has_spread {
        crate::lower_bail!(
            call.span,
            "{}(value) does not accept spread arguments",
            name
        );
    }
    if call.args.len() != 1 {
        crate::lower_bail!(call.span, "{}(value) expects exactly one argument", name);
    }
    if call.type_args.is_some() {
        crate::lower_bail!(call.span, "{}(value) does not accept type arguments", name);
    }
    Ok(())
}
