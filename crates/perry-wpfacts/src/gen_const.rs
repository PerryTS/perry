//! Constant values of literal expressions (a `require` specifier, a
//! literal call argument): literals, `typeof`, `!`, comparisons, logical
//! and conditional operators over constants, and `module.isBuiltin`.
use crate::gen::Gen;
use perry_hir::{CompareOp, Expr, LogicalOp, UnaryOp};

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Const {
    Str(String),
    Num(f64),
    Bool(bool),
    Undef,
    Null,
}

impl Const {
    fn truthy(&self) -> bool {
        match self {
            Const::Str(s) => !s.is_empty(),
            Const::Num(n) => *n != 0.0 && !n.is_nan(),
            Const::Bool(b) => *b,
            Const::Undef | Const::Null => false,
        }
    }

    fn type_of(&self) -> &'static str {
        match self {
            Const::Str(_) => "string",
            Const::Num(_) => "number",
            Const::Bool(_) => "boolean",
            Const::Undef => "undefined",
            Const::Null => "object",
        }
    }
}

impl<'a> Gen<'a> {
    pub(crate) fn const_eval(&mut self, mi: u32, e: &Expr) -> Option<Const> {
        Some(match e {
            Expr::String(s) => Const::Str(s.clone()),
            Expr::Number(n) => Const::Num(*n),
            Expr::Integer(n) => Const::Num(*n as f64),
            Expr::Bool(b) => Const::Bool(*b),
            Expr::Undefined => Const::Undef,
            Expr::Null => Const::Null,
            Expr::TypeOf(x) => Const::Str(self.const_eval(mi, x)?.type_of().into()),
            Expr::Unary {
                op: UnaryOp::Not,
                operand,
            } => Const::Bool(!self.const_eval(mi, operand)?.truthy()),
            Expr::Compare { op, left, right } => {
                let l = self.const_eval(mi, left)?;
                let r = self.const_eval(mi, right)?;
                Const::Bool(compare(*op, &l, &r)?)
            }
            Expr::Logical { op, left, right } => {
                let l = self.const_eval(mi, left)?;
                let pick_left = match op {
                    LogicalOp::And => !l.truthy(),
                    LogicalOp::Or => l.truthy(),
                    LogicalOp::Coalesce => !matches!(l, Const::Undef | Const::Null),
                };
                if pick_left {
                    l
                } else {
                    self.const_eval(mi, right)?
                }
            }
            Expr::Conditional {
                condition,
                then_expr,
                else_expr,
            } => {
                if self.const_eval(mi, condition)?.truthy() {
                    self.const_eval(mi, then_expr)?
                } else {
                    self.const_eval(mi, else_expr)?
                }
            }
            Expr::NativeMethodCall {
                module,
                method,
                args,
                ..
            } => {
                let m = module.strip_prefix("node:").unwrap_or(module);
                if m == "module" && method == "isBuiltin" && args.len() == 1 {
                    let Const::Str(s) = self.const_eval(mi, &args[0])? else {
                        return Some(Const::Bool(false));
                    };
                    return Some(Const::Bool(crate::builtins::is_builtin_module(&s)));
                }
                return None;
            }
            _ => return None,
        })
    }
}

fn compare(op: CompareOp, l: &Const, r: &Const) -> Option<bool> {
    let strict = |l: &Const, r: &Const| match (l, r) {
        (Const::Num(a), Const::Num(b)) => a == b,
        (a, b) => a == b,
    };
    Some(match op {
        CompareOp::Eq => strict(l, r),
        CompareOp::Ne => !strict(l, r),
        CompareOp::LooseEq | CompareOp::LooseNe => {
            let nullish = |c: &Const| matches!(c, Const::Undef | Const::Null);
            let eq = if nullish(l) || nullish(r) {
                nullish(l) && nullish(r)
            } else if l.type_of() == r.type_of() {
                strict(l, r)
            } else {
                return None;
            };
            if op == CompareOp::LooseEq {
                eq
            } else {
                !eq
            }
        }
        CompareOp::Lt | CompareOp::Le | CompareOp::Gt | CompareOp::Ge => {
            let (Const::Num(a), Const::Num(b)) = (l, r) else {
                return None;
            };
            match op {
                CompareOp::Lt => a < b,
                CompareOp::Le => a <= b,
                CompareOp::Gt => a > b,
                _ => a >= b,
            }
        }
    })
}

