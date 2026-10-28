//! Conditional, sign-only numeric ordering. Runtime admission proves the
//! primitive numeric domain (or one plain own numeric data property).
//! This interpreter proves effects and all three order relations, never types.
use crate::{BinaryOp, CompareOp, Expr, Function, LogicalOp, Param, Stmt, UnaryOp};
use std::collections::BTreeMap;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum NumericOrder {
    Ascending,
    Descending,
    Keyed { property: String, descending: bool },
}

/// A structural body, not an effect annotation. Each invocation interprets
/// its reachable work with the caller's symbolic arguments. Cycles fail closed.
#[derive(Clone, Copy)]
pub struct NumericCallable<'a> {
    id: u32,
    params: &'a [Param],
    body: &'a [Stmt],
}
impl<'a> NumericCallable<'a> {
    pub fn closure(expr: &'a Expr) -> Option<Self> {
        let Expr::Closure {
            func_id,
            params,
            body,
            captures_this,
            captures_new_target,
            is_async,
            is_generator,
            ..
        } = expr
        else {
            return None;
        };
        if *captures_this || *captures_new_target || *is_async || *is_generator {
            return None;
        }
        // Captured *values* have no environment entry and cannot be read.
        // Only a callee the resolver closes may name a captured binding.
        Self::new(*func_id, params, body)
    }
    pub fn function(f: &'a Function) -> Option<Self> {
        if f.is_async || f.is_generator || f.was_plain_async || !f.decorators.is_empty() {
            return None;
        }
        Self::new(f.id, &f.params, &f.body)
    }
    fn new(id: u32, params: &'a [Param], body: &'a [Stmt]) -> Option<Self> {
        if params.iter().any(|p| {
            p.default.is_some()
                || p.is_rest
                || !p.decorators.is_empty()
                || p.arguments_object.is_some()
        }) {
            return None;
        }
        let ids: std::collections::BTreeSet<_> = params.iter().map(|p| p.id).collect();
        (ids.len() == params.len()).then_some(Self { id, params, body })
    }
}

pub fn pure_numeric_order(callback: &Expr) -> Option<NumericOrder> {
    numeric_order_with(callback, &|_| None)
}

/// `resolve` must prove a single initialized compiled callee, with no TOP,
/// accessor, proxy or observable callee evaluation. Its body is checked here.
pub fn numeric_order_with<'a>(
    callback: &Expr,
    resolve: &impl Fn(&Expr) -> Option<NumericCallable<'a>>,
) -> Option<NumericOrder> {
    let mut key = None;
    let mut direction = None;
    for relation in [-1i8, 0, 1] {
        let mut proof = Proof {
            relation,
            key: &mut key,
            resolve,
            stack: Vec::new(),
            lifetime: std::marker::PhantomData,
        };
        let args = vec![Value::Input(0, None), Value::Input(1, None)];
        let value = proof.call(callback, args)?;
        let (signs, _) = proof.number(&value)?;
        if relation == 0 {
            if signs != ZERO {
                return None;
            }
        } else {
            let descending = if signs == sign(relation) {
                false
            } else if signs == sign(-relation) {
                true
            } else {
                return None;
            };
            if direction
                .replace(descending)
                .is_some_and(|d| d != descending)
            {
                return None;
            }
        }
    }
    match key? {
        Some(property) => Some(NumericOrder::Keyed {
            property,
            descending: direction?,
        }),
        None if direction? => Some(NumericOrder::Descending),
        None => Some(NumericOrder::Ascending),
    }
}
const NEG: u8 = 1;
const ZERO: u8 = 2;
const POS: u8 = 4;
fn sign(n: i8) -> u8 {
    if n < 0 {
        NEG
    } else if n > 0 {
        POS
    } else {
        ZERO
    }
}
#[derive(Clone, Debug, PartialEq)]
enum Value {
    Input(u8, Option<String>),
    Undefined,
    Number {
        signs: u8,
        nan: bool,
        exact: Option<f64>,
    },
    Bool(u8), // false = 1, true = 2
}
impl Value {
    fn exact(&self) -> Option<f64> {
        match self {
            Self::Number { exact, .. } => *exact,
            Self::Bool(1) => Some(0.0),
            Self::Bool(2) => Some(1.0),
            Self::Undefined => Some(f64::NAN),
            _ => None,
        }
    }
    fn constant(n: f64) -> Self {
        Self::Number {
            signs: if n.is_nan() || n == 0.0 {
                ZERO
            } else if n < 0.0 {
                NEG
            } else {
                POS
            },
            nan: n.is_nan(),
            exact: Some(n),
        }
    }
    fn merge(self, other: Self) -> Option<Self> {
        // IEEE equal zeros have different signs under later division.
        // Exactness survives a join only when the numeric bits also agree.
        let different_bits = matches!((&self, &other),
            (Self::Number { exact: Some(a), .. }, Self::Number { exact: Some(b), .. })
                if a.to_bits() != b.to_bits());
        if self == other && !different_bits {
            return Some(self);
        }
        match (self, other) {
            (
                Self::Number {
                    signs: a, nan: an, ..
                },
                Self::Number {
                    signs: b, nan: bn, ..
                },
            ) => Some(Self::Number {
                signs: a | b,
                nan: an || bn,
                exact: None,
            }),
            (Self::Bool(a), Self::Bool(b)) => Some(Self::Bool(a | b)),
            _ => None,
        }
    }
}
type Env = BTreeMap<u32, Value>;
struct Proof<'a, 'k, R> {
    relation: i8,
    key: &'k mut Option<Option<String>>,
    resolve: &'k R,
    stack: Vec<u32>,
    lifetime: std::marker::PhantomData<&'a ()>,
}
impl<'a, R: Fn(&Expr) -> Option<NumericCallable<'a>>> Proof<'a, '_, R> {
    fn domain(&mut self, key: &Option<String>) -> Option<()> {
        if let Some(current) = self.key.as_ref() {
            if current != key {
                return None;
            }
        } else {
            *self.key = Some(key.clone());
        }
        Some(())
    }
    fn number(&mut self, value: &Value) -> Option<(u8, bool)> {
        match value {
            Value::Undefined => Some((ZERO, true)),
            Value::Input(_, key) => {
                self.domain(key)?;
                Some((NEG | ZERO | POS, false))
            }
            Value::Number { signs, nan, .. } => Some((*signs, *nan)),
            Value::Bool(mask) => Some((
                if mask & 1 != 0 { ZERO } else { 0 } | if mask & 2 != 0 { POS } else { 0 },
                false,
            )),
        }
    }
    fn truth(&mut self, value: &Value) -> Option<u8> {
        if let Value::Bool(mask) = value {
            return Some(*mask);
        }
        let (s, nan) = self.number(value)?;
        Some(if s & ZERO != 0 || nan { 1 } else { 0 } | if s & (NEG | POS) != 0 { 2 } else { 0 })
    }
    fn call(&mut self, callee: &Expr, args: Vec<Value>) -> Option<Value> {
        let body = NumericCallable::closure(callee).or_else(|| (self.resolve)(callee))?;
        if self.stack.contains(&body.id) {
            return None;
        }
        self.stack.push(body.id);
        // Ordinary JS calls ignore extra arguments and pad missing ones.
        let env = body
            .params
            .iter()
            .enumerate()
            .map(|(i, p)| (p.id, args.get(i).cloned().unwrap_or(Value::Undefined)))
            .collect();
        let paths = self.block(body.body, vec![(env, None)])?;
        self.stack.pop();
        let mut result = None;
        for (_, value) in paths {
            let value = value.unwrap_or_else(|| Value::constant(0.0)); // undefined sorts as a tie
            result = Some(match result {
                None => value,
                Some(r) => Value::merge(r, value)?,
            });
        }
        result
    }
    fn block(
        &mut self,
        stmts: &[Stmt],
        mut paths: Vec<(Env, Option<Value>)>,
    ) -> Option<Vec<(Env, Option<Value>)>> {
        for stmt in stmts {
            let mut next = Vec::new();
            for (mut env, returned) in paths {
                if returned.is_some() {
                    next.push((env, returned));
                    continue;
                }
                match stmt {
                    Stmt::Return(value) => {
                        let value = match value {
                            Some(v) => self.expr(v, &env)?,
                            None => Value::constant(0.0),
                        };
                        next.push((env, Some(value)));
                    }
                    Stmt::Let {
                        id,
                        init: Some(value),
                        ..
                    } => {
                        let v = self.expr(value, &env)?;
                        if env.insert(*id, v).is_some() {
                            return None;
                        }
                        next.push((env, None));
                    }
                    Stmt::Expr(value) => {
                        self.expr(value, &env)?;
                        next.push((env, None));
                    }
                    Stmt::If {
                        condition,
                        then_branch,
                        else_branch,
                    } => {
                        let value = self.expr(condition, &env)?;
                        let truth = self.truth(&value)?;
                        if truth & 2 != 0 {
                            next.extend(self.block(then_branch, vec![(env.clone(), None)])?);
                        }
                        if truth & 1 != 0 {
                            next.extend(
                                self.block(
                                    else_branch.as_deref().unwrap_or(&[]),
                                    vec![(env, None)],
                                )?,
                            );
                        }
                    }
                    _ => return None,
                }
            }
            paths = next;
        }
        Some(paths)
    }
    fn expr(&mut self, expr: &Expr, env: &Env) -> Option<Value> {
        match expr {
            Expr::LocalGet(id) => env.get(id).cloned(),
            Expr::Undefined => Some(Value::Undefined),
            Expr::Void(v) => {
                self.expr(v, env)?;
                Some(Value::Undefined)
            }
            Expr::Number(n) => Some(Value::constant(*n)),
            Expr::Integer(n) => Some(Value::constant(*n as f64)),
            Expr::Bool(b) => Some(Value::Bool(if *b { 2 } else { 1 })),
            Expr::PropertyGet {
                object, property, ..
            } => match self.expr(object, env)? {
                Value::Input(side, None) => {
                    let key = Some(property.clone());
                    self.domain(&key)?;
                    Some(Value::Input(side, key))
                }
                _ => None,
            },
            Expr::NumberCoerce(v)
            | Expr::Unary {
                op: UnaryOp::Pos,
                operand: v,
            } => {
                let value = self.expr(v, env)?;
                self.number(&value)?;
                Some(if matches!(value, Value::Undefined) {
                    Value::constant(f64::NAN)
                } else {
                    value
                })
            }
            Expr::Unary {
                op: UnaryOp::Neg,
                operand,
            } => {
                let value = self.expr(operand, env)?;
                if let Value::Number { exact: Some(n), .. } = value {
                    return Some(Value::constant(-n));
                }
                let (s, nan) = self.number(&value)?;
                Some(Value::Number {
                    signs: ((s & NEG) << 2) | (s & ZERO) | ((s & POS) >> 2),
                    nan,
                    exact: None,
                })
            }
            Expr::Unary {
                op: UnaryOp::Not,
                operand,
            } => {
                let value = self.expr(operand, env)?;
                let b = self.truth(&value)?;
                Some(Value::Bool(((b & 1) << 1) | ((b & 2) >> 1)))
            }
            Expr::Binary {
                op: BinaryOp::Sub,
                left,
                right,
            } => {
                let a = self.expr(left, env)?;
                let b = self.expr(right, env)?;
                match (&a, &b) {
                    (Value::Input(a, ak), Value::Input(b, bk)) if ak == bk => {
                        self.domain(ak)?;
                        let relation = if a == b {
                            0
                        } else if a < b {
                            self.relation
                        } else {
                            -self.relation
                        };
                        Some(Value::Number {
                            signs: sign(relation),
                            nan: relation == 0,
                            exact: None,
                        })
                    }
                    _ => Some(Value::constant(a.exact()? - b.exact()?)),
                }
            }
            Expr::Binary {
                op: BinaryOp::Add | BinaryOp::Mul | BinaryOp::Div | BinaryOp::Mod,
                left,
                right,
            } => {
                let a = self.expr(left, env)?.exact()?;
                let b = self.expr(right, env)?.exact()?;
                let Expr::Binary { op, .. } = expr else {
                    unreachable!()
                };
                Some(Value::constant(match op {
                    BinaryOp::Add => a + b,
                    BinaryOp::Mul => a * b,
                    BinaryOp::Div => a / b,
                    BinaryOp::Mod => a % b,
                    _ => unreachable!(),
                }))
            }
            Expr::Compare { op, left, right } => {
                let a = self.expr(left, env)?;
                let b = self.expr(right, env)?;
                if matches!((&a, &b), (Value::Undefined, _) | (_, Value::Undefined)) {
                    self.number(&a)?;
                    self.number(&b)?;
                    let equal = matches!((&a, &b), (Value::Undefined, Value::Undefined));
                    let answer = match op {
                        CompareOp::Eq | CompareOp::LooseEq => equal,
                        CompareOp::Ne | CompareOp::LooseNe => !equal,
                        _ => false,
                    };
                    return Some(Value::Bool(if answer { 2 } else { 1 }));
                }
                let (relations, nan) = match (&a, &b) {
                    (Value::Input(a, ak), Value::Input(b, bk)) if ak == bk => {
                        self.domain(ak)?;
                        (
                            sign(if a == b {
                                0
                            } else if a < b {
                                self.relation
                            } else {
                                -self.relation
                            }),
                            false,
                        )
                    }
                    (
                        Value::Number { exact: Some(a), .. },
                        Value::Number { exact: Some(b), .. },
                    ) => {
                        if a.is_nan() || b.is_nan() {
                            (0, true)
                        } else {
                            (
                                sign(if a < b {
                                    -1
                                } else if a > b {
                                    1
                                } else {
                                    0
                                }),
                                false,
                            )
                        }
                    }
                    (_, Value::Number { exact: Some(z), .. }) if *z == 0.0 => self.number(&a)?,
                    (Value::Number { exact: Some(z), .. }, _) if *z == 0.0 => {
                        let (s, n) = self.number(&b)?;
                        (((s & NEG) << 2) | (s & ZERO) | ((s & POS) >> 2), n)
                    }
                    _ => {
                        self.number(&a)?;
                        self.number(&b)?;
                        (NEG | ZERO | POS, true)
                    }
                };
                let mut out = 0;
                for (mask, r) in [(NEG, -1), (ZERO, 0), (POS, 1)] {
                    if relations & mask == 0 {
                        continue;
                    }
                    let b = match op {
                        CompareOp::Lt => r < 0,
                        CompareOp::Le => r <= 0,
                        CompareOp::Gt => r > 0,
                        CompareOp::Ge => r >= 0,
                        CompareOp::Eq | CompareOp::LooseEq => r == 0,
                        CompareOp::Ne | CompareOp::LooseNe => r != 0,
                    };
                    out |= if b { 2 } else { 1 };
                }
                if nan {
                    out |= if matches!(op, CompareOp::Ne | CompareOp::LooseNe) {
                        2
                    } else {
                        1
                    };
                }
                Some(Value::Bool(out))
            }
            Expr::Conditional {
                condition,
                then_expr,
                else_expr,
            } => {
                let v = self.expr(condition, env)?;
                let b = self.truth(&v)?;
                match b {
                    1 => self.expr(else_expr, env),
                    2 => self.expr(then_expr, env),
                    _ => self.expr(then_expr, env)?.merge(self.expr(else_expr, env)?),
                }
            }
            Expr::Logical {
                op: LogicalOp::And | LogicalOp::Or,
                left,
                right,
            } => {
                let a = self.expr(left, env)?;
                let b = self.truth(&a)?;
                let take = if matches!(
                    expr,
                    Expr::Logical {
                        op: LogicalOp::And,
                        ..
                    }
                ) {
                    2
                } else {
                    1
                };
                if b == take {
                    self.expr(right, env)
                } else if b & take == 0 {
                    Some(a)
                } else {
                    a.merge(self.expr(right, env)?)
                }
            }
            Expr::Call { callee, args, .. } => {
                if matches!(callee.as_ref(), Expr::LocalGet(id) if env.contains_key(id)) {
                    return None; // symbolic numeric inputs are never callable
                }
                // No receiver/property callee evaluation is elided. The
                // resolver admits only closed, initialized binding reads.
                let args = args
                    .iter()
                    .map(|a| self.expr(a, env))
                    .collect::<Option<Vec<_>>>()?;
                self.call(callee, args)
            }
            _ => None,
        }
    }
}

#[cfg(test)]
#[path = "numeric_order_tests.rs"]
mod tests;
