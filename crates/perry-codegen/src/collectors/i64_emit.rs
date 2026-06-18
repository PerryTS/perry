use perry_hir::{BinaryOp, Expr, Function, Stmt};
use std::collections::HashSet;

use super::*;

/// Emit an i64-specialized function directly as LLVM IR text.
pub fn emit_i64_function(llmod: &mut crate::module::LlModule, f: &Function, i64_name: &str) {
    use crate::types::I64;
    let params: Vec<(crate::types::LlvmType, String)> = f
        .params
        .iter()
        .map(|p| (I64, format!("%arg{}", p.id)))
        .collect();
    let lf = llmod.define_function(i64_name, I64, params);
    lf.force_inline = true;
    let _ = lf.create_block("entry");
    if let Some(param_id) = fibonacci_recurrence_param(f) {
        emit_i64_fibonacci_loop(lf, param_id);
        return;
    }
    let mut locals: std::collections::HashMap<u32, String> = std::collections::HashMap::new();
    {
        let blk = lf.block_mut(0).unwrap();
        for p in &f.params {
            let slot = blk.alloca(I64);
            blk.store(I64, &format!("%arg{}", p.id), &slot);
            locals.insert(p.id, slot);
        }
    }
    let mut cx = I64Cx {
        f: lf,
        cur: 0,
        locals,
        sn: i64_name.to_string(),
        sid: f.id,
    };
    i64_body(&mut cx, &f.body);
    if !cx.f.block_mut(cx.cur).unwrap().is_terminated() {
        cx.f.block_mut(cx.cur).unwrap().ret(I64, "0");
    }
}

fn emit_i64_fibonacci_loop(lf: &mut crate::function::LlFunction, param_id: u32) {
    use crate::types::I64;

    let arg = format!("%arg{}", param_id);
    let _ = lf.create_block("i64.fib.base");
    let base_idx = lf.num_blocks() - 1;
    let base_label = lf.blocks()[base_idx].label.clone();
    let _ = lf.create_block("i64.fib.loop");
    let loop_idx = lf.num_blocks() - 1;
    let loop_label = lf.blocks()[loop_idx].label.clone();
    let _ = lf.create_block("i64.fib.cont");
    let cont_idx = lf.num_blocks() - 1;
    let cont_label = lf.blocks()[cont_idx].label.clone();
    let _ = lf.create_block("i64.fib.done");
    let done_idx = lf.num_blocks() - 1;
    let done_label = lf.blocks()[done_idx].label.clone();

    let (prev_slot, curr_slot, i_slot) = {
        let entry = lf.block_mut(0).unwrap();
        let prev_slot = entry.alloca(I64);
        let curr_slot = entry.alloca(I64);
        let i_slot = entry.alloca(I64);
        entry.store(I64, "0", &prev_slot);
        entry.store(I64, "1", &curr_slot);
        entry.store(I64, "2", &i_slot);
        let is_base = entry.icmp_sle(I64, &arg, "1");
        entry.cond_br(&is_base, &base_label, &loop_label);
        (prev_slot, curr_slot, i_slot)
    };

    lf.block_mut(base_idx).unwrap().ret(I64, &arg);

    let (curr, next, i) = {
        let loop_block = lf.block_mut(loop_idx).unwrap();
        let prev = loop_block.load(I64, &prev_slot);
        let curr = loop_block.load(I64, &curr_slot);
        let next = loop_block.add(I64, &prev, &curr);
        let i = loop_block.load(I64, &i_slot);
        let done = loop_block.icmp_sge(I64, &i, &arg);
        loop_block.cond_br(&done, &done_label, &cont_label);
        (curr, next, i)
    };

    {
        let cont = lf.block_mut(cont_idx).unwrap();
        cont.store(I64, &curr, &prev_slot);
        cont.store(I64, &next, &curr_slot);
        let next_i = cont.add(I64, &i, "1");
        cont.store(I64, &next_i, &i_slot);
        cont.br(&loop_label);
    }

    lf.block_mut(done_idx).unwrap().ret(I64, &next);
}

fn fibonacci_recurrence_param(f: &Function) -> Option<u32> {
    if f.params.len() != 1 {
        return None;
    }
    let param_id = f.params[0].id;
    match f.body.as_slice() {
        [base_case, recursive_return]
            if is_fibonacci_base_case(base_case, param_id)
                && is_fibonacci_recursive_return(recursive_return, f.id, param_id) =>
        {
            Some(param_id)
        }
        _ => None,
    }
}

fn is_fibonacci_base_case(stmt: &Stmt, param_id: u32) -> bool {
    matches!(
        stmt,
        Stmt::If {
            condition: Expr::Compare {
                op: perry_hir::CompareOp::Le,
                left,
                right,
            },
            then_branch,
            else_branch: None,
        } if matches!(left.as_ref(), Expr::LocalGet(id) if *id == param_id)
            && integer_literal(right, 1)
            && matches!(
                then_branch.as_slice(),
                [Stmt::Return(Some(Expr::LocalGet(id)))] if *id == param_id
            )
    )
}

fn is_fibonacci_recursive_return(stmt: &Stmt, sid: u32, param_id: u32) -> bool {
    let Stmt::Return(Some(Expr::Binary {
        op: BinaryOp::Add,
        left,
        right,
    })) = stmt
    else {
        return false;
    };
    (is_self_call_minus(left, sid, param_id, 1) && is_self_call_minus(right, sid, param_id, 2))
        || (is_self_call_minus(left, sid, param_id, 2)
            && is_self_call_minus(right, sid, param_id, 1))
}

fn is_self_call_minus(expr: &Expr, sid: u32, param_id: u32, amount: i64) -> bool {
    matches!(
        expr,
        Expr::Call { callee, args, .. }
            if matches!(callee.as_ref(), Expr::FuncRef(id) if *id == sid)
                && matches!(
                    args.as_slice(),
                    [Expr::Binary {
                        op: BinaryOp::Sub,
                        left,
                        right,
                    }] if matches!(left.as_ref(), Expr::LocalGet(id) if *id == param_id)
                        && integer_literal(right, amount)
                )
    )
}

fn integer_literal(expr: &Expr, expected: i64) -> bool {
    match expr {
        Expr::Integer(n) => *n == expected,
        Expr::Number(n) => *n == expected as f64,
        _ => false,
    }
}

struct I64Cx<'a> {
    f: &'a mut crate::function::LlFunction,
    cur: usize,
    locals: std::collections::HashMap<u32, String>,
    sn: String,
    sid: u32,
}

pub fn i64_body(cx: &mut I64Cx<'_>, ss: &[Stmt]) {
    use crate::types::I64;
    for s in ss {
        if cx.f.block_mut(cx.cur).unwrap().is_terminated() {
            break;
        }
        match s {
            Stmt::Return(Some(e)) => {
                let v = i64_val(cx, e);
                cx.f.block_mut(cx.cur).unwrap().ret(I64, &v);
            }
            Stmt::Return(None) => {
                cx.f.block_mut(cx.cur).unwrap().ret(I64, "0");
            }
            Stmt::Let {
                id, init: Some(e), ..
            } => {
                let v = i64_val(cx, e);
                let slot = cx.f.block_mut(cx.cur).unwrap().alloca(I64);
                cx.f.block_mut(cx.cur).unwrap().store(I64, &v, &slot);
                cx.locals.insert(*id, slot);
            }
            Stmt::Let { id, init: None, .. } => {
                let slot = cx.f.block_mut(cx.cur).unwrap().alloca(I64);
                cx.f.block_mut(cx.cur).unwrap().store(I64, "0", &slot);
                cx.locals.insert(*id, slot);
            }
            Stmt::Expr(e) => {
                let _ = i64_val(cx, e);
            }
            Stmt::If {
                condition,
                then_branch,
                else_branch,
            } => {
                let cond = i64_cond(cx, condition);
                let _ = cx.f.create_block("i64.then");
                let ti = cx.f.num_blocks() - 1;
                let tl = cx.f.blocks()[ti].label.clone();
                let ei = if else_branch.is_some() {
                    let _ = cx.f.create_block("i64.else");
                    cx.f.num_blocks() - 1
                } else {
                    0
                };
                let el = if else_branch.is_some() {
                    cx.f.blocks()[ei].label.clone()
                } else {
                    String::new()
                };
                let _ = cx.f.create_block("i64.merge");
                let mi = cx.f.num_blocks() - 1;
                let ml = cx.f.blocks()[mi].label.clone();
                let target_else = if else_branch.is_some() { &el } else { &ml };
                cx.f.block_mut(cx.cur)
                    .unwrap()
                    .cond_br(&cond, &tl, target_else);
                cx.cur = ti;
                i64_body(cx, then_branch);
                if !cx.f.block_mut(cx.cur).unwrap().is_terminated() {
                    cx.f.block_mut(cx.cur).unwrap().br(&ml);
                }
                if let Some(eb) = else_branch {
                    cx.cur = ei;
                    i64_body(cx, eb);
                    if !cx.f.block_mut(cx.cur).unwrap().is_terminated() {
                        cx.f.block_mut(cx.cur).unwrap().br(&ml);
                    }
                }
                cx.cur = mi;
            }
            _ => {}
        }
    }
}
pub fn i64_cond(cx: &mut I64Cx<'_>, e: &Expr) -> String {
    use crate::types::I64;
    if let Expr::Compare { op, left, right } = e {
        let l = i64_val(cx, left);
        let r = i64_val(cx, right);
        let blk = cx.f.block_mut(cx.cur).unwrap();
        return match op {
            perry_hir::CompareOp::Le => blk.icmp_sle(I64, &l, &r),
            perry_hir::CompareOp::Lt => blk.icmp_slt(I64, &l, &r),
            perry_hir::CompareOp::Gt => blk.icmp_sgt(I64, &l, &r),
            perry_hir::CompareOp::Ge => blk.icmp_sge(I64, &l, &r),
            perry_hir::CompareOp::Eq | perry_hir::CompareOp::LooseEq => blk.icmp_eq(I64, &l, &r),
            perry_hir::CompareOp::Ne | perry_hir::CompareOp::LooseNe => blk.icmp_ne(I64, &l, &r),
        };
    }
    let v = i64_val(cx, e);
    cx.f.block_mut(cx.cur).unwrap().icmp_ne(I64, &v, "0")
}
pub fn i64_val(cx: &mut I64Cx<'_>, e: &Expr) -> String {
    use crate::types::I64;
    match e {
        Expr::Integer(n) => n.to_string(),
        Expr::Number(n) => (*n as i64).to_string(),
        Expr::LocalGet(id) => {
            if let Some(slot) = cx.locals.get(id).cloned() {
                cx.f.block_mut(cx.cur).unwrap().load(I64, &slot)
            } else {
                "0".to_string()
            }
        }
        Expr::Binary { op, left, right } => {
            let l = i64_val(cx, left);
            let r = i64_val(cx, right);
            let blk = cx.f.block_mut(cx.cur).unwrap();
            match op {
                BinaryOp::Add => blk.add(I64, &l, &r),
                BinaryOp::Sub => blk.sub(I64, &l, &r),
                BinaryOp::Mul => blk.mul(I64, &l, &r),
                _ => "0".to_string(),
            }
        }
        Expr::Call { callee, args, .. } => {
            if let Expr::FuncRef(id) = callee.as_ref() {
                if *id == cx.sid {
                    let mut lo: Vec<(crate::types::LlvmType, String)> = Vec::new();
                    for a in args {
                        let v = i64_val(cx, a);
                        lo.push((I64, v));
                    }
                    let refs: Vec<(crate::types::LlvmType, &str)> =
                        lo.iter().map(|(t, v)| (*t, v.as_str())).collect();
                    let nm = cx.sn.clone();
                    return cx.f.block_mut(cx.cur).unwrap().call(I64, &nm, &refs);
                }
            }
            "0".to_string()
        }
        _ => "0".to_string(),
    }
}

// ── Escape analysis for scalar replacement of non-escaping objects ──
