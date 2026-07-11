use perry_hir::{BinaryOp, Expr, Function, Stmt};

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
        Expr::Conditional {
            condition,
            then_expr,
            else_expr,
        } => {
            // Must be real control flow, not `select`: a branch can hold the
            // self-recursive call, and evaluating both sides unconditionally
            // would recurse past the base case.
            let slot = cx.f.block_mut(cx.cur).unwrap().alloca(I64);
            let cond = i64_cond(cx, condition);
            let _ = cx.f.create_block("i64.cond.then");
            let ti = cx.f.num_blocks() - 1;
            let tl = cx.f.blocks()[ti].label.clone();
            let _ = cx.f.create_block("i64.cond.else");
            let ei = cx.f.num_blocks() - 1;
            let el = cx.f.blocks()[ei].label.clone();
            let _ = cx.f.create_block("i64.cond.merge");
            let mi = cx.f.num_blocks() - 1;
            let ml = cx.f.blocks()[mi].label.clone();
            cx.f.block_mut(cx.cur).unwrap().cond_br(&cond, &tl, &el);
            cx.cur = ti;
            let tv = i64_val(cx, then_expr);
            let blk = cx.f.block_mut(cx.cur).unwrap();
            blk.store(I64, &tv, &slot);
            blk.br(&ml);
            cx.cur = ei;
            let ev = i64_val(cx, else_expr);
            let blk = cx.f.block_mut(cx.cur).unwrap();
            blk.store(I64, &ev, &slot);
            blk.br(&ml);
            cx.cur = mi;
            cx.f.block_mut(cx.cur).unwrap().load(I64, &slot)
        }
        _ => "0".to_string(),
    }
}

// ── Escape analysis for scalar replacement of non-escaping objects ──
