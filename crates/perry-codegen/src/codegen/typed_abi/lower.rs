//! Typed-ABI clone body lowering.
//!
//! Split out of `typed_abi.rs` to keep that file under the 2000-line cap.
//! Pure mechanical move — every item below is a verbatim copy.

use std::collections::HashMap;

use perry_hir::types::Type;
use perry_hir::{BinaryOp, CompareOp, Expr, LogicalOp, Stmt, UnaryOp};

use super::{
    expr_is_typed_f64_safe, expr_is_typed_i1_safe, expr_is_typed_i32_safe,
    integer_literal_fits_i32, is_f64_type, is_string_type, typed_param_rep_for_type, TypedParamRep,
    TypedReceiverMethodInfo,
};

fn lower_typed_f64_expr_with_env(
    blk: &mut crate::block::LlBlock,
    expr: &Expr,
    locals: &HashMap<u32, String>,
    reps: &HashMap<u32, TypedParamRep>,
) -> anyhow::Result<String> {
    match expr {
        Expr::Number(n) => Ok(crate::nanbox::double_literal(*n)),
        Expr::Integer(n) => Ok(format!("{}.0", *n)),
        Expr::LocalGet(id) => {
            let value = locals
                .get(id)
                .cloned()
                .unwrap_or_else(|| format!("%arg{id}"));
            if matches!(reps.get(id), Some(TypedParamRep::I32)) {
                Ok(blk.sitofp(crate::types::I32, &value, crate::types::DOUBLE))
            } else {
                Ok(value)
            }
        }
        Expr::Unary {
            op: UnaryOp::Pos,
            operand,
        } => lower_typed_f64_expr_with_env(blk, operand, locals, reps),
        Expr::Unary {
            op: UnaryOp::Neg,
            operand,
        } => {
            let v = lower_typed_f64_expr_with_env(blk, operand, locals, reps)?;
            Ok(blk.fneg(&v))
        }
        Expr::Binary { op, left, right } => {
            let l = lower_typed_f64_expr_with_env(blk, left, locals, reps)?;
            let r = lower_typed_f64_expr_with_env(blk, right, locals, reps)?;
            Ok(match op {
                BinaryOp::Add => blk.fadd(&l, &r),
                BinaryOp::Sub => blk.fsub(&l, &r),
                BinaryOp::Mul => blk.fmul(&l, &r),
                BinaryOp::Div => blk.fdiv(&l, &r),
                BinaryOp::Mod => blk.frem(&l, &r),
                _ => {
                    anyhow::bail!("typed-f64 clone cannot lower non-arithmetic expression")
                }
            })
        }
        _ => anyhow::bail!(
            "typed-f64 clone cannot lower expression kind {}",
            crate::expr::variant_name(expr)
        ),
    }
}

fn lower_typed_i32_expr_with_env(
    blk: &mut crate::block::LlBlock,
    expr: &Expr,
    locals: &HashMap<u32, String>,
) -> anyhow::Result<String> {
    match expr {
        Expr::Integer(n) if integer_literal_fits_i32(*n) => Ok(n.to_string()),
        Expr::LocalGet(id) => Ok(locals
            .get(id)
            .cloned()
            .unwrap_or_else(|| format!("%arg{id}"))),
        Expr::Unary {
            op: UnaryOp::BitNot,
            operand,
        } => {
            let v = lower_typed_i32_expr_with_env(blk, operand, locals)?;
            Ok(blk.xor(crate::types::I32, &v, "-1"))
        }
        Expr::Binary { op, left, right } => {
            let l = lower_typed_i32_expr_with_env(blk, left, locals)?;
            let r_raw = lower_typed_i32_expr_with_env(blk, right, locals)?;
            let r = if matches!(op, BinaryOp::Shl | BinaryOp::Shr) {
                blk.and(crate::types::I32, &r_raw, "31")
            } else {
                r_raw
            };
            Ok(match op {
                BinaryOp::BitAnd => blk.and(crate::types::I32, &l, &r),
                BinaryOp::BitOr => blk.or(crate::types::I32, &l, &r),
                BinaryOp::BitXor => blk.xor(crate::types::I32, &l, &r),
                BinaryOp::Shl => blk.shl(crate::types::I32, &l, &r),
                BinaryOp::Shr => blk.ashr(crate::types::I32, &l, &r),
                _ => anyhow::bail!("typed-i32 clone cannot lower non-bitwise expression"),
            })
        }
        _ => anyhow::bail!(
            "typed-i32 clone cannot lower expression kind {}",
            crate::expr::variant_name(expr)
        ),
    }
}

fn lower_typed_i1_expr_with_env(
    blk: &mut crate::block::LlBlock,
    expr: &Expr,
    locals: &HashMap<u32, String>,
    reps: &HashMap<u32, TypedParamRep>,
) -> anyhow::Result<String> {
    match expr {
        Expr::Bool(value) => Ok(value.to_string()),
        Expr::LocalGet(id) => Ok(locals
            .get(id)
            .cloned()
            .unwrap_or_else(|| format!("%arg{id}"))),
        Expr::Unary {
            op: UnaryOp::Not,
            operand,
        } => {
            let v = lower_typed_i1_expr_with_env(blk, operand, locals, reps)?;
            Ok(blk.xor(crate::types::I1, &v, "true"))
        }
        Expr::Logical { op, left, right } => {
            let l = lower_typed_i1_expr_with_env(blk, left, locals, reps)?;
            let r = lower_typed_i1_expr_with_env(blk, right, locals, reps)?;
            Ok(match op {
                LogicalOp::And => blk.and(crate::types::I1, &l, &r),
                LogicalOp::Or => blk.or(crate::types::I1, &l, &r),
                LogicalOp::Coalesce => {
                    anyhow::bail!("typed-i1 clone cannot lower nullish coalesce")
                }
            })
        }
        Expr::Compare { op, left, right } => {
            if expr_is_typed_i1_safe(left, reps)
                && expr_is_typed_i1_safe(right, reps)
                && matches!(
                    op,
                    CompareOp::Eq | CompareOp::Ne | CompareOp::LooseEq | CompareOp::LooseNe
                )
            {
                let l = lower_typed_i1_expr_with_env(blk, left, locals, reps)?;
                let r = lower_typed_i1_expr_with_env(blk, right, locals, reps)?;
                return Ok(match op {
                    CompareOp::Eq | CompareOp::LooseEq => blk.icmp_eq(crate::types::I1, &l, &r),
                    CompareOp::Ne | CompareOp::LooseNe => blk.icmp_ne(crate::types::I1, &l, &r),
                    _ => unreachable!("guarded boolean comparison op"),
                });
            }
            if expr_is_typed_f64_safe(left, reps) && expr_is_typed_f64_safe(right, reps) {
                if expr_is_typed_i32_safe(left, reps) && expr_is_typed_i32_safe(right, reps) {
                    let l = lower_typed_i32_expr_with_env(blk, left, locals)?;
                    let r = lower_typed_i32_expr_with_env(blk, right, locals)?;
                    return Ok(match op {
                        CompareOp::Eq | CompareOp::LooseEq => {
                            blk.icmp_eq(crate::types::I32, &l, &r)
                        }
                        CompareOp::Ne | CompareOp::LooseNe => {
                            blk.icmp_ne(crate::types::I32, &l, &r)
                        }
                        CompareOp::Lt => blk.icmp_slt(crate::types::I32, &l, &r),
                        CompareOp::Le => blk.icmp_sle(crate::types::I32, &l, &r),
                        CompareOp::Gt => blk.icmp_sgt(crate::types::I32, &l, &r),
                        CompareOp::Ge => blk.icmp_sge(crate::types::I32, &l, &r),
                    });
                }
                let l = lower_typed_f64_expr_with_env(blk, left, locals, reps)?;
                let r = lower_typed_f64_expr_with_env(blk, right, locals, reps)?;
                let cond = match op {
                    CompareOp::Eq | CompareOp::LooseEq => "oeq",
                    CompareOp::Ne | CompareOp::LooseNe => "une",
                    CompareOp::Lt => "olt",
                    CompareOp::Le => "ole",
                    CompareOp::Gt => "ogt",
                    CompareOp::Ge => "oge",
                };
                return Ok(blk.fcmp(cond, &l, &r));
            }
            anyhow::bail!("typed-i1 clone cannot lower mixed comparison")
        }
        _ => anyhow::bail!(
            "typed-i1 clone cannot lower expression kind {}",
            crate::expr::variant_name(expr)
        ),
    }
}

fn lower_typed_string_expr_with_env(
    expr: &Expr,
    locals: &HashMap<u32, String>,
) -> anyhow::Result<String> {
    match expr {
        Expr::LocalGet(id) => Ok(locals
            .get(id)
            .cloned()
            .unwrap_or_else(|| format!("%arg{id}"))),
        _ => anyhow::bail!(
            "typed-string clone cannot lower expression kind {}",
            crate::expr::variant_name(expr)
        ),
    }
}

pub(crate) fn lower_typed_f64_body_with_seed_locals(
    blk: &mut crate::block::LlBlock,
    params: &[perry_hir::Param],
    body: &[Stmt],
    locals: HashMap<u32, String>,
) -> anyhow::Result<String> {
    lower_typed_f64_body_with_seed_locals_and_reps(blk, params, body, locals, HashMap::new())
}

pub(crate) fn lower_typed_f64_body_with_seed_locals_and_reps(
    blk: &mut crate::block::LlBlock,
    params: &[perry_hir::Param],
    body: &[Stmt],
    mut locals: HashMap<u32, String>,
    mut reps: HashMap<u32, TypedParamRep>,
) -> anyhow::Result<String> {
    for param in params {
        locals.insert(param.id, format!("%arg{}", param.id));
        if let Some(rep) = typed_param_rep_for_type(&param.ty) {
            reps.insert(param.id, rep);
        }
    }
    let Some((last, prefix)) = body.split_last() else {
        anyhow::bail!("typed-f64 clone cannot lower empty body");
    };
    for stmt in prefix {
        match stmt {
            Stmt::Let {
                id,
                ty,
                mutable: false,
                init: Some(expr),
                ..
            } if is_f64_type(ty) => {
                let value = lower_typed_f64_expr_with_env(blk, expr, &locals, &reps)?;
                locals.insert(*id, value);
                reps.insert(*id, TypedParamRep::F64);
            }
            Stmt::Let {
                id,
                ty: Type::Int32,
                mutable: false,
                init: Some(expr),
                ..
            } => {
                let value = lower_typed_i32_expr_with_env(blk, expr, &locals)?;
                locals.insert(*id, value);
                reps.insert(*id, TypedParamRep::I32);
            }
            _ => anyhow::bail!("typed-f64 clone cannot lower non-straight-line statement"),
        }
    }
    match last {
        Stmt::Return(Some(expr)) => lower_typed_f64_expr_with_env(blk, expr, &locals, &reps),
        _ => anyhow::bail!("typed-f64 clone requires a final return value"),
    }
}

pub(crate) fn lower_typed_f64_body(
    blk: &mut crate::block::LlBlock,
    params: &[perry_hir::Param],
    body: &[Stmt],
) -> anyhow::Result<String> {
    lower_typed_f64_body_with_seed_locals(blk, params, body, HashMap::new())
}

pub(crate) fn lower_typed_i32_body(
    blk: &mut crate::block::LlBlock,
    params: &[perry_hir::Param],
    body: &[Stmt],
) -> anyhow::Result<String> {
    lower_typed_i32_body_with_seed_locals(blk, params, body, HashMap::new())
}

pub(crate) fn lower_typed_i32_body_with_seed_locals(
    blk: &mut crate::block::LlBlock,
    params: &[perry_hir::Param],
    body: &[Stmt],
    mut locals: HashMap<u32, String>,
) -> anyhow::Result<String> {
    for param in params {
        locals.insert(param.id, format!("%arg{}", param.id));
    }
    let Some((last, prefix)) = body.split_last() else {
        anyhow::bail!("typed-i32 clone cannot lower empty body");
    };
    for stmt in prefix {
        match stmt {
            Stmt::Let {
                id,
                ty: Type::Int32,
                mutable: false,
                init: Some(expr),
                ..
            } => {
                let value = lower_typed_i32_expr_with_env(blk, expr, &locals)?;
                locals.insert(*id, value);
            }
            _ => anyhow::bail!("typed-i32 clone cannot lower non-straight-line statement"),
        }
    }
    match last {
        Stmt::Return(Some(expr)) => lower_typed_i32_expr_with_env(blk, expr, &locals),
        _ => anyhow::bail!("typed-i32 clone requires a final return value"),
    }
}

pub(crate) fn lower_typed_string_body_with_seed_locals(
    _blk: &mut crate::block::LlBlock,
    params: &[perry_hir::Param],
    body: &[Stmt],
    mut locals: HashMap<u32, String>,
) -> anyhow::Result<String> {
    for param in params {
        locals.insert(param.id, format!("%arg{}", param.id));
    }
    let Some((last, prefix)) = body.split_last() else {
        anyhow::bail!("typed-string clone cannot lower empty body");
    };
    for stmt in prefix {
        match stmt {
            Stmt::Let {
                id,
                ty,
                mutable: false,
                init: Some(expr),
                ..
            } if is_string_type(ty) => {
                let value = lower_typed_string_expr_with_env(expr, &locals)?;
                locals.insert(*id, value);
            }
            _ => anyhow::bail!("typed-string clone cannot lower non-straight-line statement"),
        }
    }
    match last {
        Stmt::Return(Some(expr)) => lower_typed_string_expr_with_env(expr, &locals),
        _ => anyhow::bail!("typed-string clone requires a final return value"),
    }
}

pub(crate) fn lower_typed_string_body(
    blk: &mut crate::block::LlBlock,
    params: &[perry_hir::Param],
    body: &[Stmt],
) -> anyhow::Result<String> {
    lower_typed_string_body_with_seed_locals(blk, params, body, HashMap::new())
}

fn lower_typed_f64_receiver_field(
    blk: &mut crate::block::LlBlock,
    field_index: u32,
    header_skip: u64,
) -> String {
    let obj_ptr = blk.inttoptr(crate::types::I64, "%this_obj");
    let header_skip_str = header_skip.to_string();
    let fields_base = blk.gep(
        crate::types::I8,
        &obj_ptr,
        &[(crate::types::I64, &header_skip_str)],
    );
    let field_index_str = field_index.to_string();
    let field_ptr = blk.gep(
        crate::types::DOUBLE,
        &fields_base,
        &[(crate::types::I64, &field_index_str)],
    );
    blk.load(crate::types::DOUBLE, &field_ptr)
}

fn lower_typed_f64_receiver_expr_with_env(
    blk: &mut crate::block::LlBlock,
    expr: &Expr,
    locals: &HashMap<u32, String>,
    receiver: &TypedReceiverMethodInfo,
    header_skip: u64,
) -> anyhow::Result<String> {
    match expr {
        Expr::Number(n) => Ok(crate::nanbox::double_literal(*n)),
        Expr::Integer(n) => Ok(format!("{}.0", *n)),
        Expr::LocalGet(id) => Ok(locals
            .get(id)
            .cloned()
            .unwrap_or_else(|| format!("%arg{id}"))),
        Expr::PropertyGet {
            object, property, ..
        } if matches!(object.as_ref(), Expr::This) => {
            let Some(field_index) = receiver.field_index(property) else {
                anyhow::bail!("typed-f64 receiver clone cannot lower unproven receiver field")
            };
            Ok(lower_typed_f64_receiver_field(
                blk,
                field_index,
                header_skip,
            ))
        }
        Expr::Unary {
            op: UnaryOp::Pos,
            operand,
        } => lower_typed_f64_receiver_expr_with_env(blk, operand, locals, receiver, header_skip),
        Expr::Unary {
            op: UnaryOp::Neg,
            operand,
        } => {
            let v = lower_typed_f64_receiver_expr_with_env(
                blk,
                operand,
                locals,
                receiver,
                header_skip,
            )?;
            Ok(blk.fneg(&v))
        }
        Expr::Binary { op, left, right } => {
            let l =
                lower_typed_f64_receiver_expr_with_env(blk, left, locals, receiver, header_skip)?;
            let r =
                lower_typed_f64_receiver_expr_with_env(blk, right, locals, receiver, header_skip)?;
            Ok(match op {
                BinaryOp::Add => blk.fadd(&l, &r),
                BinaryOp::Sub => blk.fsub(&l, &r),
                BinaryOp::Mul => blk.fmul(&l, &r),
                BinaryOp::Div => blk.fdiv(&l, &r),
                BinaryOp::Mod => blk.frem(&l, &r),
                _ => {
                    anyhow::bail!("typed-f64 receiver clone cannot lower non-arithmetic expression")
                }
            })
        }
        _ => anyhow::bail!(
            "typed-f64 receiver clone cannot lower expression kind {}",
            crate::expr::variant_name(expr)
        ),
    }
}

pub(crate) fn lower_typed_f64_receiver_body(
    blk: &mut crate::block::LlBlock,
    params: &[perry_hir::Param],
    body: &[Stmt],
    receiver: &TypedReceiverMethodInfo,
    header_skip: u64,
) -> anyhow::Result<String> {
    let mut locals = HashMap::new();
    for param in params {
        locals.insert(param.id, format!("%arg{}", param.id));
    }
    let Some((last, prefix)) = body.split_last() else {
        anyhow::bail!("typed-f64 receiver clone cannot lower empty body");
    };
    for stmt in prefix {
        match stmt {
            Stmt::Let {
                id,
                ty,
                mutable: false,
                init: Some(expr),
                ..
            } if is_f64_type(ty) => {
                let value = lower_typed_f64_receiver_expr_with_env(
                    blk,
                    expr,
                    &locals,
                    receiver,
                    header_skip,
                )?;
                locals.insert(*id, value);
            }
            _ => anyhow::bail!("typed-f64 receiver clone cannot lower non-straight-line statement"),
        }
    }
    match last {
        Stmt::Return(Some(expr)) => {
            lower_typed_f64_receiver_expr_with_env(blk, expr, &locals, receiver, header_skip)
        }
        _ => anyhow::bail!("typed-f64 receiver clone requires a final return value"),
    }
}

pub(crate) fn lower_typed_i1_body_with_seed_locals(
    blk: &mut crate::block::LlBlock,
    params: &[perry_hir::Param],
    body: &[Stmt],
    mut locals: HashMap<u32, String>,
    mut reps: HashMap<u32, TypedParamRep>,
) -> anyhow::Result<String> {
    for param in params {
        locals.insert(param.id, format!("%arg{}", param.id));
        if let Some(rep) = typed_param_rep_for_type(&param.ty) {
            reps.insert(param.id, rep);
        }
    }
    let Some((last, prefix)) = body.split_last() else {
        anyhow::bail!("typed-i1 clone cannot lower empty body");
    };
    for stmt in prefix {
        match stmt {
            Stmt::Let {
                id,
                ty: Type::Boolean,
                mutable: false,
                init: Some(expr),
                ..
            } => {
                let value = lower_typed_i1_expr_with_env(blk, expr, &locals, &reps)?;
                locals.insert(*id, value);
                reps.insert(*id, TypedParamRep::I1);
            }
            Stmt::Let {
                id,
                ty,
                mutable: false,
                init: Some(expr),
                ..
            } if is_f64_type(ty) => {
                let value = lower_typed_f64_expr_with_env(blk, expr, &locals, &reps)?;
                locals.insert(*id, value);
                reps.insert(*id, TypedParamRep::F64);
            }
            Stmt::Let {
                id,
                ty: Type::Int32,
                mutable: false,
                init: Some(expr),
                ..
            } => {
                let value = lower_typed_i32_expr_with_env(blk, expr, &locals)?;
                locals.insert(*id, value);
                reps.insert(*id, TypedParamRep::I32);
            }
            _ => anyhow::bail!("typed-i1 clone cannot lower non-straight-line statement"),
        }
    }
    match last {
        Stmt::Return(Some(expr)) => lower_typed_i1_expr_with_env(blk, expr, &locals, &reps),
        _ => anyhow::bail!("typed-i1 clone requires a final return value"),
    }
}

pub(crate) fn lower_typed_i1_body(
    blk: &mut crate::block::LlBlock,
    params: &[perry_hir::Param],
    body: &[Stmt],
) -> anyhow::Result<String> {
    lower_typed_i1_body_with_seed_locals(blk, params, body, HashMap::new(), HashMap::new())
}
