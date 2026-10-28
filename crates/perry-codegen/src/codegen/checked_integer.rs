//! Guarded integer return transport for closed pure scalar recurrences.
//!
//! The entry validates an exact input interval proved by integer_contract.
//! All source intermediates then fit nonnegative i32, with no per-operation
//! bailout. Calls retain their order, physical nonleaf frames and stack checks.
//! A proven effect-free, frameless base entry can be substituted at a call;
//! the original caller-side stack check still precedes that call decision.
use crate::types::{DOUBLE, I1, I32, I64, I8, PTR, VOID};
use crate::{
    expr::{lower_expr, FnCtx},
    function::LlFunction,
    module::LlModule,
};
use anyhow::Result;
use perry_hir::{BinaryOp, CompareOp, Expr, Function, Stmt};
use std::collections::HashMap;

const LIMIT: i64 = i32::MAX as i64;

fn expr_ok(e: &Expr, f: &Function, recursive: &mut bool) -> bool {
    match e {
        Expr::Integer(n) => (0..=LIMIT).contains(n),
        Expr::Number(n) => {
            n.is_finite()
                && n.fract() == 0.0
                && *n >= 0.0
                && *n <= LIMIT as f64
                && n.to_bits() != (-0.0f64).to_bits()
        }
        Expr::LocalGet(id) => f.params.iter().any(|p| p.id == *id),
        Expr::Binary {
            op: BinaryOp::Add | BinaryOp::Sub,
            left,
            right,
        } => expr_ok(left, f, recursive) && expr_ok(right, f, recursive),
        Expr::Conditional {
            condition,
            then_expr,
            else_expr,
        } => {
            matches!(condition.as_ref(), Expr::Compare { .. })
                && condition_ok(condition, f, recursive)
                && expr_ok(then_expr, f, recursive)
                && expr_ok(else_expr, f, recursive)
        }
        Expr::Call { callee, args, .. }
            if matches!(callee.as_ref(), Expr::FuncRef(id) if *id == f.id)
                && args.len() == f.params.len() =>
        {
            *recursive = true;
            args.iter().all(|a| expr_ok(a, f, recursive))
        }
        _ => false,
    }
}
fn condition_ok(e: &Expr, f: &Function, recursive: &mut bool) -> bool {
    match e {
        Expr::Compare { left, right, .. } => {
            expr_ok(left, f, recursive) && expr_ok(right, f, recursive)
        }
        _ => false,
    }
}
fn stmts_ok(body: &[Stmt], f: &Function, recursive: &mut bool) -> Option<bool> {
    let mut returns = false;
    for stmt in body {
        match stmt {
            Stmt::Return(Some(e)) if expr_ok(e, f, recursive) => {
                // Comparisons are Boolean, not integer return values.
                if matches!(e, Expr::Compare { .. }) {
                    return None;
                }
                returns = true;
            }
            Stmt::If {
                condition,
                then_branch,
                else_branch,
            } if matches!(condition, Expr::Compare { .. })
                && condition_ok(condition, f, recursive) =>
            {
                let a = stmts_ok(then_branch, f, recursive)?;
                let b = match else_branch {
                    Some(b) => stmts_ok(b, f, recursive)?,
                    None => false,
                };
                returns |= a && b;
            }
            _ => return None,
        }
    }
    Some(returns)
}
pub(super) fn eligible(f: &Function, target: &str) -> bool {
    if !super::spec_abi::spec_preserve_none_enabled()
        || !super::spec_abi::preserve_none_target_ok(target)
        || !crate::expr::agent_ptr::executable_tls(target)
        || f.params.is_empty()
        || !f.captures.is_empty()
        || f.is_async
        || f.is_generator
        || f.was_plain_async
        || f.params.iter().any(|p| {
            p.default.is_some()
                || p.is_rest
                || !matches!(
                    p.ty,
                    perry_hir::types::Type::Number | perry_hir::types::Type::Int32
                )
        })
        || !matches!(
            f.return_type,
            perry_hir::types::Type::Number | perry_hir::types::Type::Int32
        )
    {
        return false;
    }
    let mut recursive = false;
    stmts_ok(&f.body, f, &mut recursive) == Some(true) && recursive
}
pub(super) fn name(body: &str) -> String {
    format!("{body}.$checked_i32")
}

struct Lowerer<'a> {
    lf: &'a mut LlFunction,
    current: usize,
    contract: super::integer_contract::Contract,
    symbol: String,
    values: HashMap<u32, String>,
    checked_stack: bool,
    sp: &'static str,
}
impl Lowerer<'_> {
    fn block(&mut self) -> &mut crate::block::LlBlock {
        self.lf.block_mut(self.current).unwrap()
    }
    fn new_block(&mut self, label: &str) -> (usize, String) {
        let label = self.lf.create_block(label).label.clone();
        (self.lf.blocks().len() - 1, label)
    }
    fn stack_guard(&mut self) {
        if self.checked_stack {
            return;
        }
        let tls = "@PERRY_AGENT_PTRS".to_string();
        let offset = (crate::runtime_abi::AGENT_PTR_STACK_LIMIT * 8).to_string();
        let at = self.block().gep(I8, &tls, &[(I64, &offset)]);
        let limit = self.block().load(PTR, &at);
        let limit = self.block().ptrtoint(&limit, I64);
        let sp = self.block().next_reg();
        let register = self.sp;
        self.block().emit_raw(format!(
            "{sp} = call i64 @llvm.read_register.i64(metadata !{{!\"{register}\"}})"
        ));
        let deep = self.block().icmp_ult(I64, &sp, &limit);
        let (overflow, overflow_label) = self.new_block("integer.stack_overflow");
        let (ok, ok_label) = self.new_block("integer.stack_ok");
        self.block().cond_br(&deep, &overflow_label, &ok_label);
        self.current = overflow;
        self.block().call_void("js_stack_overflow", &[]);
        self.block().br(&ok_label);
        self.current = ok;
        self.checked_stack = true;
    }
    fn expr(&mut self, e: &Expr) -> String {
        match e {
            Expr::Integer(n) => n.to_string(),
            Expr::Number(n) => (*n as i64).to_string(),
            Expr::LocalGet(id) => self.values[id].clone(),
            Expr::Binary { op, left, right } => {
                let a = self.expr(left);
                let b = self.expr(right);
                let v = match op {
                    BinaryOp::Add => self.block().add(I32, &a, &b),
                    BinaryOp::Sub => self.block().sub(I32, &a, &b),
                    _ => unreachable!(),
                };
                v
            }
            Expr::Compare { op, left, right } => {
                let a = self.expr(left);
                let b = self.expr(right);
                match op {
                    CompareOp::Lt => self.block().icmp_slt(I32, &a, &b),
                    CompareOp::Le => self.block().icmp_sle(I32, &a, &b),
                    CompareOp::Gt => self.block().icmp_sgt(I32, &a, &b),
                    CompareOp::Ge => self.block().icmp_sge(I32, &a, &b),
                    CompareOp::Eq | CompareOp::LooseEq => self.block().icmp_eq(I32, &a, &b),
                    CompareOp::Ne | CompareOp::LooseNe => self.block().icmp_ne(I32, &a, &b),
                }
            }
            Expr::Call { args, .. } => {
                let value = self.expr(&args[0]);
                self.stack_guard();
                let boundary = self.contract.base_limit.to_string();
                let leaf = self.block().icmp_slt(I32, &value, &boundary);
                let (base, base_label) = self.new_block("integer.base");
                let (call, call_label) = self.new_block("integer.call");
                let (join, join_label) = self.new_block("integer.call_result");
                self.block().cond_br(&leaf, &base_label, &call_label);
                self.current = base;
                let base_value = match self.contract.base {
                    super::integer_contract::Base::Param => value.clone(),
                    super::integer_contract::Base::Constant(n) => n.to_string(),
                };
                self.block().br(&join_label);
                self.current = call;
                let symbol = self.symbol.clone();
                let result = self.block().call(I32, &symbol, &[(I32, &value)]);
                self.block().br(&join_label);
                self.current = join;
                self.block()
                    .phi(I32, &[(&base_value, &base_label), (&result, &call_label)])
            }
            Expr::Conditional {
                condition,
                then_expr,
                else_expr,
            } => {
                let c = self.expr(condition);
                let before = self.checked_stack;
                let (a, al) = self.new_block("integer.then");
                let (b, bl) = self.new_block("integer.else");
                let (join, jl) = self.new_block("integer.merge");
                self.block().cond_br(&c, &al, &bl);
                self.current = a;
                let av = self.expr(then_expr);
                let ac = self.checked_stack;
                let ap = self.block().label.clone();
                self.block().br(&jl);
                self.current = b;
                self.checked_stack = before;
                let bv = self.expr(else_expr);
                self.checked_stack &= ac;
                let bp = self.block().label.clone();
                self.block().br(&jl);
                self.current = join;
                self.block().phi(I32, &[(&av, &ap), (&bv, &bp)])
            }
            _ => unreachable!(),
        }
    }
    fn stmts(&mut self, body: &[Stmt]) {
        for s in body {
            if self.block().is_terminated() {
                break;
            }
            match s {
                Stmt::Return(Some(e)) => {
                    let v = self.expr(e);
                    self.block().ret(I32, &v);
                }
                Stmt::If {
                    condition,
                    then_branch,
                    else_branch,
                } => {
                    let bit = self.expr(condition);
                    let before = self.checked_stack;
                    let (a, al) = self.new_block("integer.if");
                    let (b, bl) = self.new_block("integer.else");
                    let (join, jl) = self.new_block("integer.after");
                    self.block().cond_br(&bit, &al, &bl);
                    self.current = a;
                    self.stmts(then_branch);
                    if !self.block().is_terminated() {
                        self.block().br(&jl);
                    }
                    self.current = b;
                    self.checked_stack = before;
                    if let Some(body) = else_branch {
                        self.stmts(body);
                    }
                    if !self.block().is_terminated() {
                        self.block().br(&jl);
                    }
                    // A preceding branch can return. Conservatively recheck
                    // after the join instead of assuming a dominating check.
                    self.checked_stack = before;
                    self.current = join;
                }
                _ => unreachable!(),
            }
        }
    }
}
pub(super) fn emit(
    llmod: &mut LlModule,
    f: &Function,
    body: &str,
    contract: super::integer_contract::Contract,
) {
    let symbol = name(body);
    llmod.declare_function("llvm.assume", VOID, &[I1]);
    if super::spec_abi::spec_preserve_none_enabled()
        && super::spec_abi::preserve_none_target_ok(&llmod.target_triple)
    {
        // Use the existing compiler-owned convention set for definitions,
        // declarations and calls; no additional dispatch state is introduced.
        llmod.set_preserve_none_fns([symbol.clone()]);
    }
    let sp = if llmod.target_triple.starts_with("x86_64") {
        "rsp"
    } else {
        "sp"
    };
    let params: Vec<_> = f
        .params
        .iter()
        .map(|p| (I32, format!("%i{}", p.id)))
        .collect();
    let values = f
        .params
        .iter()
        .zip(&params)
        .map(|(p, (_, v))| (p.id, v.clone()))
        .collect();
    let lf = llmod.define_function(&symbol, I32, params);
    lf.linkage = "internal".into();
    lf.disable_tail_calls = true;
    lf.create_block("entry");
    let mut lower = Lowerer {
        lf,
        current: 0,
        contract,
        symbol,
        values,
        checked_stack: false,
        sp,
    };
    // The only callers are the actual-value guard and checked self calls.
    for param in &f.params {
        let value = lower.values[&param.id].clone();
        let valid = lower.block().icmp_sge(I32, &value, "0");
        let upper = lower
            .block()
            .icmp_sle(I32, &value, &contract.input_max.to_string());
        let valid = lower.block().and(I1, &valid, &upper);
        lower.block().call_void("llvm.assume", &[(I1, &valid)]);
    }
    lower.stmts(&f.body);
    if !lower.block().is_terminated() {
        lower.block().unreachable();
    }
}
pub(super) fn dispatch(
    ctx: &mut FnCtx<'_>,
    f: &Function,
    body: &str,
    contract: super::integer_contract::Contract,
) -> Result<()> {
    let mut all = "true".to_string();
    let mut args = Vec::new();
    for p in &f.params {
        let v = lower_expr(ctx, &Expr::LocalGet(p.id))?;
        let converted = ctx.block().fptosi(DOUBLE, &v, I32);
        let raw = ctx.block().next_reg();
        ctx.block()
            .emit_raw(format!("{raw} = freeze i32 {converted}"));
        let back = ctx.block().sitofp(I32, &raw, DOUBLE);
        let exact = ctx.block().fcmp("oeq", &v, &back);
        let bits = ctx.block().bitcast_double_to_i64(&v);
        let positive_zero = ctx.block().icmp_ne(I64, &bits, "-9223372036854775808");
        let exact = ctx.block().and(I1, &exact, &positive_zero);
        let nonnegative = ctx.block().icmp_sge(I32, &raw, "0");
        let upper = ctx
            .block()
            .icmp_sle(I32, &raw, &contract.input_max.to_string());
        let bounded = ctx.block().and(I1, &nonnegative, &upper);
        let exact = ctx.block().and(I1, &exact, &bounded);
        all = ctx.block().and(I1, &all, &exact);
        args.push((I32, raw));
    }
    let run = ctx.new_block("integer.entry");
    let fallback = ctx.new_block("integer.fallback");
    let result = ctx.new_block("integer.result");
    let run_label = ctx.block_label(run);
    let fallback_label = ctx.block_label(fallback);
    let result_label = ctx.block_label(result);
    ctx.block().cond_br(&all, &run_label, &fallback_label);
    ctx.current_block = run;
    let args: Vec<_> = args.iter().map(|(t, v)| (*t, v.as_str())).collect();
    let v = ctx.block().call(I32, &name(body), &args);
    ctx.block().br(&result_label);
    ctx.current_block = result;
    let v = ctx.block().sitofp(I32, &v, DOUBLE);
    ctx.block().ret(DOUBLE, &v);
    ctx.current_block = fallback;
    Ok(())
}
