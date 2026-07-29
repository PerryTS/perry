use super::*;

use crate::types::Type;
use anyhow::Result;
use swc_ecma_ast as ast;

use crate::lower_types::extract_ts_type_with_ctx;

use super::super::super::{lower_expr, LoweringContext};

fn pod_layout_intrinsic_is_shadowed(ctx: &LoweringContext, name: &str) -> bool {
    ctx.lookup_local(name).is_some()
        || ctx.lookup_func(name).is_some()
        || ctx.lookup_imported_func(name).is_some()
}

fn explicit_single_type_arg(
    ctx: &LoweringContext,
    call: &ast::CallExpr,
    name: &str,
) -> Result<Type> {
    let Some(type_args) = call.type_args.as_ref() else {
        crate::lower_bail!(
            call.span,
            "{}<T>() requires exactly one explicit PerryPod type argument",
            name
        );
    };
    if type_args.params.len() != 1 {
        crate::lower_bail!(
            call.span,
            "{}<T>() requires exactly one explicit PerryPod type argument",
            name
        );
    }
    let type_arg = &type_args.params[0];
    if let Some(ty) = bare_type_param_type_arg(ctx, type_arg) {
        return Ok(ty);
    }
    Ok(extract_ts_type_with_ctx(type_arg, Some(ctx)))
}

fn bare_type_param_type_arg(ctx: &LoweringContext, type_arg: &ast::TsType) -> Option<Type> {
    let ast::TsType::TsTypeRef(type_ref) = type_arg else {
        return None;
    };
    if type_ref.type_params.is_some() {
        return None;
    }
    let ast::TsEntityName::Ident(ident) = &type_ref.type_name else {
        return None;
    };
    let name = ident.sym.to_string();
    ctx.is_type_param(&name).then_some(Type::TypeVar(name))
}

fn literal_offset_path(arg: &ast::Expr) -> Option<Vec<String>> {
    let ast::Expr::Lit(ast::Lit::Str(s)) = arg else {
        return None;
    };
    let raw = s.value.as_str().unwrap_or("");
    let path: Vec<String> = raw.split('.').map(str::to_string).collect();
    (!path.is_empty() && path.iter().all(|segment| !segment.is_empty())).then_some(path)
}

/// Public compile-time POD layout constants.
pub(crate) fn try_pod_layout_constants(
    ctx: &LoweringContext,
    call: &ast::CallExpr,
    has_spread: bool,
) -> Result<Option<Expr>> {
    let ast::Callee::Expr(callee_expr) = &call.callee else {
        return Ok(None);
    };
    let ast::Expr::Ident(ident) = callee_expr.as_ref() else {
        return Ok(None);
    };
    let name = ident.sym.as_ref();
    if !matches!(name, "sizeof" | "alignof" | "offsetof") {
        return Ok(None);
    }
    if pod_layout_intrinsic_is_shadowed(ctx, name) {
        return Ok(None);
    }
    if has_spread {
        crate::lower_bail!(call.span, "{}(...) does not accept spread arguments", name);
    }

    let ty = explicit_single_type_arg(ctx, call, name)?;
    match name {
        "sizeof" => {
            if !call.args.is_empty() {
                crate::lower_bail!(call.span, "sizeof<T>() expects no arguments");
            }
            Ok(Some(Expr::PodLayoutSizeOf { ty }))
        }
        "alignof" => {
            if !call.args.is_empty() {
                crate::lower_bail!(call.span, "alignof<T>() expects no arguments");
            }
            Ok(Some(Expr::PodLayoutAlignOf { ty }))
        }
        "offsetof" => {
            if call.args.len() != 1 {
                crate::lower_bail!(
                    call.span,
                    "offsetof<T>(field) expects exactly one string-literal field path"
                );
            }
            let Some(field_path) = literal_offset_path(call.args[0].expr.as_ref()) else {
                crate::lower_bail!(
                    call.span,
                    "offsetof<T>(field) requires a compile-time string-literal field path"
                );
            };
            Ok(Some(Expr::PodLayoutOffsetOf { ty, field_path }))
        }
        _ => Ok(None),
    }
}

fn native_arena_hidden_kind_from_expr(expr: &ast::Expr) -> Option<u8> {
    match expr {
        ast::Expr::Lit(ast::Lit::Str(s)) => {
            crate::ir::typed_array_kind_for_name(s.value.as_str().unwrap_or(""))
        }
        ast::Expr::Lit(ast::Lit::Num(n)) if n.value.fract() == 0.0 => {
            let raw = n.value as i64;
            (0..=crate::ir::TYPED_ARRAY_KIND_BIGUINT64 as i64)
                .contains(&raw)
                .then_some(raw as u8)
        }
        _ => None,
    }
}

fn native_arena_public_kind_from_expr(ctx: &LoweringContext, expr: &ast::Expr) -> Option<u8> {
    match expr {
        ast::Expr::Lit(ast::Lit::Str(s)) => {
            crate::ir::typed_array_kind_for_name(s.value.as_str().unwrap_or(""))
        }
        ast::Expr::Ident(ident)
            if ctx.lookup_local(ident.sym.as_ref()).is_none()
                && ctx.lookup_func(ident.sym.as_ref()).is_none()
                && ctx.lookup_imported_func(ident.sym.as_ref()).is_none()
                && ctx.lookup_class(ident.sym.as_ref()).is_none() =>
        {
            crate::ir::typed_array_kind_for_name(ident.sym.as_ref())
        }
        ast::Expr::Paren(paren) => native_arena_public_kind_from_expr(ctx, &paren.expr),
        ast::Expr::TsAs(ts_as) => native_arena_public_kind_from_expr(ctx, &ts_as.expr),
        ast::Expr::TsTypeAssertion(ts_assert) => {
            native_arena_public_kind_from_expr(ctx, &ts_assert.expr)
        }
        ast::Expr::TsNonNull(non_null) => native_arena_public_kind_from_expr(ctx, &non_null.expr),
        ast::Expr::TsConstAssertion(const_assert) => {
            native_arena_public_kind_from_expr(ctx, &const_assert.expr)
        }
        _ => None,
    }
}

fn native_arena_global_is_shadowed(ctx: &LoweringContext) -> bool {
    ctx.lookup_local("NativeArena").is_some()
        || ctx.lookup_func("NativeArena").is_some()
        || ctx.lookup_imported_func("NativeArena").is_some()
        || ctx.lookup_class("NativeArena").is_some()
}

fn native_memory_global_is_shadowed(ctx: &LoweringContext) -> bool {
    ctx.lookup_local("NativeMemory").is_some()
        || ctx.lookup_func("NativeMemory").is_some()
        || ctx.lookup_imported_func("NativeMemory").is_some()
        || ctx.lookup_class("NativeMemory").is_some()
}

pub(crate) fn try_native_memory_public_api(
    ctx: &mut LoweringContext,
    call: &ast::CallExpr,
    has_spread: bool,
) -> Result<Option<Expr>> {
    let ast::Callee::Expr(callee_expr) = &call.callee else {
        return Ok(None);
    };
    let ast::Expr::Member(member) = callee_expr.as_ref() else {
        return Ok(None);
    };
    let ast::MemberProp::Ident(prop) = &member.prop else {
        return Ok(None);
    };
    if !matches!(member.obj.as_ref(), ast::Expr::Ident(obj) if obj.sym.as_ref() == "NativeMemory")
        || native_memory_global_is_shadowed(ctx)
    {
        return Ok(None);
    }

    match prop.sym.as_ref() {
        "fillU32" => {
            if has_spread {
                crate::lower_bail!(
                    call.span,
                    "NativeMemory.fillU32(view, value) does not accept spread arguments"
                );
            }
            if call.args.len() != 2 {
                crate::lower_bail!(
                    call.span,
                    "NativeMemory.fillU32(view, value) expects exactly two arguments"
                );
            }
            Ok(Some(Expr::NativeMemoryFillU32 {
                view: Box::new(lower_expr(ctx, &call.args[0].expr)?),
                value: Box::new(lower_expr(ctx, &call.args[1].expr)?),
            }))
        }
        "copy" => {
            if has_spread {
                crate::lower_bail!(
                    call.span,
                    "NativeMemory.copy(dst, src) does not accept spread arguments"
                );
            }
            if call.args.len() != 2 {
                crate::lower_bail!(
                    call.span,
                    "NativeMemory.copy(dst, src) expects exactly two arguments"
                );
            }
            Ok(Some(Expr::NativeMemoryCopy {
                dst: Box::new(lower_expr(ctx, &call.args[0].expr)?),
                src: Box::new(lower_expr(ctx, &call.args[1].expr)?),
            }))
        }
        _ => Ok(None),
    }
}

fn is_native_arena_alloc_call(ctx: &LoweringContext, call: &ast::CallExpr) -> bool {
    let ast::Callee::Expr(callee_expr) = &call.callee else {
        return false;
    };
    let ast::Expr::Member(member) = callee_expr.as_ref() else {
        return false;
    };
    matches!(member.obj.as_ref(), ast::Expr::Ident(obj) if obj.sym.as_ref() == "NativeArena")
        && matches!(&member.prop, ast::MemberProp::Ident(prop) if prop.sym.as_ref() == "alloc")
        && !native_arena_global_is_shadowed(ctx)
}

fn native_arena_owner_type(ty: &crate::types::Type) -> bool {
    matches!(ty, crate::types::Type::Named(name) if name == "NativeArena" || name == "NativeArenaOwner")
}

fn is_native_arena_owner_expr(ctx: &LoweringContext, expr: &ast::Expr) -> bool {
    match expr {
        ast::Expr::Ident(ident) => ctx
            .lookup_local_type(ident.sym.as_ref())
            .is_some_and(native_arena_owner_type),
        ast::Expr::Call(call) => is_native_arena_alloc_call(ctx, call),
        ast::Expr::Paren(paren) => is_native_arena_owner_expr(ctx, &paren.expr),
        ast::Expr::TsAs(ts_as) => is_native_arena_owner_expr(ctx, &ts_as.expr),
        ast::Expr::TsTypeAssertion(ts_assert) => is_native_arena_owner_expr(ctx, &ts_assert.expr),
        ast::Expr::TsNonNull(non_null) => is_native_arena_owner_expr(ctx, &non_null.expr),
        ast::Expr::TsConstAssertion(const_assert) => {
            is_native_arena_owner_expr(ctx, &const_assert.expr)
        }
        _ => false,
    }
}

/// Public compile-time NativeArena API. The runtime still exposes only the
/// internal helpers; these direct dot-call shapes lower to the same HIR nodes.
pub(crate) fn try_native_arena_public_api(
    ctx: &mut LoweringContext,
    call: &ast::CallExpr,
    has_spread: bool,
) -> Result<Option<Expr>> {
    let ast::Callee::Expr(callee_expr) = &call.callee else {
        return Ok(None);
    };
    let ast::Expr::Member(member) = callee_expr.as_ref() else {
        return Ok(None);
    };
    let ast::MemberProp::Ident(prop) = &member.prop else {
        return Ok(None);
    };
    let method = prop.sym.as_ref();

    if matches!(member.obj.as_ref(), ast::Expr::Ident(obj) if obj.sym.as_ref() == "NativeArena") {
        if method != "alloc" || native_arena_global_is_shadowed(ctx) {
            return Ok(None);
        }
        if has_spread {
            crate::lower_bail!(
                call.span,
                "NativeArena.alloc(byteLength) does not accept spread arguments"
            );
        }
        if call.args.len() != 1 {
            crate::lower_bail!(
                call.span,
                "NativeArena.alloc(byteLength) expects exactly one argument"
            );
        }
        return Ok(Some(Expr::NativeArenaAlloc(Box::new(lower_expr(
            ctx,
            &call.args[0].expr,
        )?))));
    }

    if !is_native_arena_owner_expr(ctx, member.obj.as_ref()) {
        return Ok(None);
    }

    match method {
        "view" => {
            if has_spread {
                crate::lower_bail!(
                    call.span,
                    "NativeArena.view(kind, byteOffset, length) does not accept spread arguments"
                );
            }
            if call.args.len() != 3 {
                crate::lower_bail!(
                    call.span,
                    "NativeArena.view(kind, byteOffset, length) expects exactly three arguments"
                );
            }
            let Some(kind) = native_arena_public_kind_from_expr(ctx, call.args[0].expr.as_ref())
            else {
                crate::lower_bail!(
                    call.span,
                    "NativeArena.view kind must be a typed-array constructor or string literal"
                );
            };
            Ok(Some(Expr::NativeArenaView {
                owner: Box::new(lower_expr(ctx, member.obj.as_ref())?),
                kind,
                byte_offset: Box::new(lower_expr(ctx, &call.args[1].expr)?),
                length: Box::new(lower_expr(ctx, &call.args[2].expr)?),
            }))
        }
        "podView" => {
            if has_spread {
                crate::lower_bail!(
                    call.span,
                    "NativeArena.podView(byteOffset, count) does not accept spread arguments"
                );
            }
            if call.args.len() != 2 {
                crate::lower_bail!(
                    call.span,
                    "NativeArena.podView(byteOffset, count) expects exactly two arguments"
                );
            }
            let view_type = match call.type_args.as_ref() {
                Some(type_args) if type_args.params.len() == 1 => {
                    let type_arg = &type_args.params[0];
                    let pod_ty = bare_type_param_type_arg(ctx, type_arg)
                        .unwrap_or_else(|| extract_ts_type_with_ctx(type_arg, Some(ctx)));
                    Some(Type::Generic {
                        base: "PerryPodView".to_string(),
                        type_args: vec![pod_ty],
                    })
                }
                Some(_) => {
                    crate::lower_bail!(
                        call.span,
                        "NativeArena.podView<T>(byteOffset, count) expects exactly one explicit type argument"
                    );
                }
                None => None,
            };
            Ok(Some(Expr::NativePodView {
                owner: Box::new(lower_expr(ctx, member.obj.as_ref())?),
                byte_offset: Box::new(lower_expr(ctx, &call.args[0].expr)?),
                count: Box::new(lower_expr(ctx, &call.args[1].expr)?),
                view_type,
            }))
        }
        "dispose" => {
            if has_spread {
                crate::lower_bail!(
                    call.span,
                    "NativeArena.dispose() does not accept spread arguments"
                );
            }
            if !call.args.is_empty() {
                crate::lower_bail!(call.span, "NativeArena.dispose() expects no arguments");
            }
            Ok(Some(Expr::NativeArenaDispose(Box::new(lower_expr(
                ctx,
                member.obj.as_ref(),
            )?))))
        }
        _ => Ok(None),
    }
}

/// Hidden internal native-arena intrinsics. They intentionally require the
/// view kind to be a literal so native lowering can carry width facts.
pub(crate) fn try_native_arena_intrinsics(
    ctx: &mut LoweringContext,
    call: &ast::CallExpr,
    has_spread: bool,
) -> Result<Option<Expr>> {
    if has_spread {
        return Ok(None);
    }
    let ast::Callee::Expr(callee_expr) = &call.callee else {
        return Ok(None);
    };
    let ast::Expr::Ident(ident) = callee_expr.as_ref() else {
        return Ok(None);
    };
    let name = ident.sym.as_ref();
    if name == "__perry_native_pod_view" {
        if call.args.len() != 3 || call.args.iter().any(|arg| arg.spread.is_some()) {
            crate::lower_bail!(
                call.span,
                "__perry_native_pod_view(owner, byteOffset, count) expects exactly three arguments"
            );
        }
        return Ok(Some(Expr::NativePodView {
            owner: Box::new(lower_expr(ctx, &call.args[0].expr)?),
            byte_offset: Box::new(lower_expr(ctx, &call.args[1].expr)?),
            count: Box::new(lower_expr(ctx, &call.args[2].expr)?),
            view_type: None,
        }));
    }
    if !name.starts_with("__perry_native_arena_") {
        return Ok(None);
    }
    if ctx.lookup_local(name).is_some() || ctx.lookup_func(name).is_some() {
        return Ok(None);
    }
    match name {
        "__perry_native_arena_alloc" => {
            if call.args.len() != 1 || call.args[0].spread.is_some() {
                crate::lower_bail!(
                    call.span,
                    "__perry_native_arena_alloc(byteLength) expects exactly one argument"
                );
            }
            Ok(Some(Expr::NativeArenaAlloc(Box::new(lower_expr(
                ctx,
                &call.args[0].expr,
            )?))))
        }
        "__perry_native_arena_view" => {
            if call.args.len() != 4 || call.args.iter().any(|arg| arg.spread.is_some()) {
                crate::lower_bail!(
                    call.span,
                    "__perry_native_arena_view(owner, kind, byteOffset, length) expects exactly four arguments"
                );
            }
            let Some(kind) = native_arena_hidden_kind_from_expr(call.args[1].expr.as_ref()) else {
                crate::lower_bail!(
                    call.span,
                    "__perry_native_arena_view kind must be a typed-array name or kind literal"
                );
            };
            Ok(Some(Expr::NativeArenaView {
                owner: Box::new(lower_expr(ctx, &call.args[0].expr)?),
                kind,
                byte_offset: Box::new(lower_expr(ctx, &call.args[2].expr)?),
                length: Box::new(lower_expr(ctx, &call.args[3].expr)?),
            }))
        }
        "__perry_native_arena_dispose" => {
            if call.args.len() != 1 || call.args[0].spread.is_some() {
                crate::lower_bail!(
                    call.span,
                    "__perry_native_arena_dispose(owner) expects exactly one argument"
                );
            }
            Ok(Some(Expr::NativeArenaDispose(Box::new(lower_expr(
                ctx,
                &call.args[0].expr,
            )?))))
        }
        _ => Ok(None),
    }
}
