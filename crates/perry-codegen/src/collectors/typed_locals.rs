//! Closed World 5d: the most fixed type of every local of a body, by
//! construction (owner decision 140, "every function internally gets a type
//! that is as fixed as possible").
//!
//! A local is a String (or a Number) in a body when
//! 1. every write to it that can execute is String- (Number-) producing by
//!    construction, judged by a greatest fixed point over the body's
//!    exhaustive write set (`not_bigint_locals::collect_writes`, plus the
//!    `Update` and in-place array writes that walker does not list), and
//! 2. no read of it can run before its first write (`undefined` is neither):
//!    a conservative definite-assignment walk over the body.
//!
//! Writes that cannot execute are those in an `if` branch whose condition is
//! decided by an entry proof of the typed body (`typeof p === "symbol"` on a
//! parameter proven String). The proof is the typed body's O(1) entry check
//! of a parameter that is never rebound, so the branch really never runs.
//!
//! Declared types are never evidence. The leaves are the constructions the
//! codegen already trusts as proofs (`string_value_is_runtime_guaranteed`,
//! `expr_produces_canonical_raw_f64`): literals, `String(x)`, templates,
//! `typeof`, `join`, the String-returning and Number-returning
//! `String.prototype` methods called on a proven String, a proven String's
//! `.length`, and the numeric operators. A Number this way is a canonical
//! double (arithmetic gives the default NaN; `charCodeAt` out of range gives
//! the hardware quiet NaN), which is what `number_by_construction_locals`
//! promises its readers.
//!
//! Parameters, boxed cells, module globals and locals captured from an
//! enclosing scope are never candidates: their writes are not all in this
//! walk. Parameter proofs come in from the typed body's entry check.

use std::collections::{HashMap, HashSet};

use perry_hir::types::Type as HirType;
use perry_hir::{BinaryOp, CompareOp, Expr, LogicalOp, Stmt, UnaryOp};

/// `PERRY_TYPED_LOCALS=0` turns the analysis off (A/B control).
pub(crate) fn enabled() -> bool {
    use std::sync::OnceLock;
    static CACHED: OnceLock<bool> = OnceLock::new();
    *CACHED.get_or_init(|| {
        !matches!(
            std::env::var("PERRY_TYPED_LOCALS").as_deref(),
            Ok("0") | Ok("off") | Ok("false")
        )
    })
}

/// The result: body locals proven String or Number at every read.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct TypedLocals {
    pub strings: HashSet<u32>,
    pub numbers: HashSet<u32>,
    /// Locals that hold an Array created by a literal in this body.
    pub arrays: HashSet<u32>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Kind {
    Str,
    Num,
    Arr,
}

/// Facts a typed body's entry check proves for its never-rebound parameters.
fn param_kind(proofs: &HashMap<u32, HirType>, id: u32) -> Option<Kind> {
    match proofs.get(&id)? {
        HirType::String | HirType::StringLiteral(_) => Some(Kind::Str),
        HirType::Number | HirType::Int32 => Some(Kind::Num),
        HirType::Array(_) => Some(Kind::Arr),
        _ => None,
    }
}

fn typeof_name(t: &HirType) -> Option<&'static str> {
    match t {
        HirType::String | HirType::StringLiteral(_) => Some("string"),
        HirType::Number | HirType::Int32 => Some("number"),
        HirType::Boolean => Some("boolean"),
        _ => None,
    }
}

/// `Some(b)` when `cond` always evaluates to `b` under the parameter proofs:
/// `typeof p ===/!==/==/!= "<lit>"` (either side), `!c`, and `&&`/`||` of
/// decided parts.
fn decided(cond: &Expr, proofs: &HashMap<u32, HirType>) -> Option<bool> {
    match cond {
        Expr::Unary {
            op: UnaryOp::Not,
            operand,
        } => decided(operand, proofs).map(|b| !b),
        Expr::Logical { op, left, right } => {
            let (l, r) = (decided(left, proofs), decided(right, proofs));
            match op {
                LogicalOp::And => match (l, r) {
                    (Some(false), _) => Some(false),
                    (Some(true), r) => r,
                    _ => None,
                },
                LogicalOp::Or => match (l, r) {
                    (Some(true), _) => Some(true),
                    (Some(false), r) => r,
                    _ => None,
                },
                _ => None,
            }
        }
        Expr::Compare { op, left, right } => {
            let eq = match op {
                CompareOp::Eq | CompareOp::LooseEq => true,
                CompareOp::Ne | CompareOp::LooseNe => false,
                _ => return None,
            };
            let (tested, lit) = match (left.as_ref(), right.as_ref()) {
                (Expr::TypeOf(x), Expr::String(s)) | (Expr::String(s), Expr::TypeOf(x)) => (x, s),
                _ => return None,
            };
            let Expr::LocalGet(id) = tested.as_ref() else {
                return None;
            };
            let name = typeof_name(proofs.get(id)?)?;
            Some((name == lit.as_str()) == eq)
        }
        _ => None,
    }
}

/// Every write of the body (as `not_bigint_locals::collect_writes` lists
/// them), skipping `if` branches the proofs decide.
fn collect_live_writes<'a>(
    stmts: &'a [Stmt],
    proofs: &HashMap<u32, HirType>,
    writes: &mut HashMap<u32, Vec<Option<&'a Expr>>>,
    let_bound: &mut HashSet<u32>,
) {
    use super::not_bigint_locals::{collect_writes, collect_writes_expr};
    for s in stmts {
        match s {
            // A hoisted `var` (and `let x;`) starts as an `undefined` seed.
            // The seed is not a write the type must cover; instead no read
            // may run before a real write (`reads_before_write`).
            Stmt::Let {
                id,
                init: None | Some(Expr::Undefined),
                ..
            } => {
                let_bound.insert(*id);
            }
            Stmt::If {
                condition,
                then_branch,
                else_branch,
            } => {
                collect_writes_expr(condition, writes, let_bound);
                match decided(condition, proofs) {
                    Some(true) => collect_live_writes(then_branch, proofs, writes, let_bound),
                    Some(false) => {
                        if let Some(b) = else_branch {
                            collect_live_writes(b, proofs, writes, let_bound);
                        }
                    }
                    None => {
                        collect_live_writes(then_branch, proofs, writes, let_bound);
                        if let Some(b) = else_branch {
                            collect_live_writes(b, proofs, writes, let_bound);
                        }
                    }
                }
            }
            Stmt::While { condition, body } => {
                collect_writes_expr(condition, writes, let_bound);
                collect_live_writes(body, proofs, writes, let_bound);
            }
            Stmt::DoWhile { body, condition } => {
                collect_live_writes(body, proofs, writes, let_bound);
                collect_writes_expr(condition, writes, let_bound);
            }
            Stmt::For {
                init,
                condition,
                update,
                body,
            } => {
                if let Some(i) = init {
                    collect_live_writes(
                        std::slice::from_ref(i.as_ref()),
                        proofs,
                        writes,
                        let_bound,
                    );
                }
                if let Some(c) = condition {
                    collect_writes_expr(c, writes, let_bound);
                }
                if let Some(u) = update {
                    collect_writes_expr(u, writes, let_bound);
                }
                collect_live_writes(body, proofs, writes, let_bound);
            }
            Stmt::Labeled { body, .. } => collect_live_writes(
                std::slice::from_ref(body.as_ref()),
                proofs,
                writes,
                let_bound,
            ),
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                collect_live_writes(body, proofs, writes, let_bound);
                if let Some(c) = catch {
                    collect_live_writes(&c.body, proofs, writes, let_bound);
                }
                if let Some(f) = finally {
                    collect_live_writes(f, proofs, writes, let_bound);
                }
            }
            other => collect_writes(std::slice::from_ref(other), writes, let_bound),
        }
    }
}

/// Locals written by something other than a `Let`/`LocalSet` value: `x++`
/// (a Number, whatever `x` held) and the in-place array/set operations that
/// name a local (`ArrayPush { array_id }` …). Any such local is dropped from
/// the String set; `Update` keeps a Number a Number.
fn collect_other_writes(
    stmts: &[Stmt],
    updates: &mut HashSet<u32>,
    in_place: &mut HashSet<u32>,
    poisoned: &mut HashSet<u32>,
) {
    fn expr(
        e: &Expr,
        updates: &mut HashSet<u32>,
        in_place: &mut HashSet<u32>,
        poisoned: &mut HashSet<u32>,
    ) {
        match e {
            Expr::Update { id, .. } => {
                updates.insert(*id);
            }
            Expr::ArrayPush { array_id, .. }
            | Expr::ArrayPushSpread { array_id, .. }
            | Expr::ArrayUnshift { array_id, .. }
            | Expr::ArraySplice { array_id, .. }
            | Expr::ArrayCopyWithin { array_id, .. } => {
                in_place.insert(*array_id);
            }
            Expr::ArrayPop(id) | Expr::ArrayShift(id) => {
                in_place.insert(*id);
            }
            Expr::SetAdd { set_id, .. } => {
                poisoned.insert(*set_id);
            }
            Expr::ScopedTemp { id, .. } => {
                poisoned.insert(*id);
            }
            Expr::WithSet {
                fallback:
                    perry_hir::WithSetFallback::Local(id)
                    | perry_hir::WithSetFallback::SloppyImplicit(id),
                ..
            } => {
                poisoned.insert(*id);
            }
            Expr::Closure { body, .. } => collect_other_writes(body, updates, in_place, poisoned),
            _ => {}
        }
        perry_hir::walker::walk_expr_children(e, &mut |c| expr(c, updates, in_place, poisoned));
    }
    for s in stmts {
        perry_hir::walker::stmt_any_expr(s, &mut |e| {
            expr(e, updates, in_place, poisoned);
            false
        });
    }
}

struct Env<'a> {
    iife: &'a std::cell::RefCell<HashMap<u32, Option<Kind>>>,
    boxed_vars: &'a HashSet<u32>,
    module_globals: &'a HashMap<u32, String>,
    proofs: &'a HashMap<u32, HirType>,
    strings: &'a HashSet<u32>,
    numbers: &'a HashSet<u32>,
    arrays: &'a HashSet<u32>,
}

impl Env<'_> {
    fn local(&self, id: u32) -> Option<Kind> {
        if let Some(k) = param_kind(self.proofs, id) {
            return Some(k);
        }
        if self.strings.contains(&id) {
            Some(Kind::Str)
        } else if self.numbers.contains(&id) {
            Some(Kind::Num)
        } else if self.arrays.contains(&id) {
            Some(Kind::Arr)
        } else {
            None
        }
    }

    fn is(&self, e: &Expr, k: Kind) -> bool {
        self.kind(e, 0) == Some(k)
    }

    fn kind(&self, e: &Expr, depth: usize) -> Option<Kind> {
        if depth > 24 {
            return None;
        }
        let rec = |x: &Expr| self.kind(x, depth + 1);
        match e {
            Expr::Integer(_) | Expr::Number(_) => Some(Kind::Num),
            Expr::String(_)
            | Expr::WtfString(_)
            | Expr::StringCoerce(_)
            | Expr::TemplateStringCoerce(_)
            | Expr::TypeOf(_)
            | Expr::ArrayJoin { .. }
            | Expr::StringFromCharCode(_)
            | Expr::StringFromCharCodeSpread(_)
            | Expr::StringFromCodePoint(_) => Some(Kind::Str),
            Expr::Array(_) | Expr::ArraySpread(_) => Some(Kind::Arr),
            Expr::LocalGet(id) => self.local(*id),
            Expr::LocalSet(_, v) => rec(v),
            Expr::Sequence(v) => v.last().and_then(rec),
            Expr::Binary { op, left, right } => {
                let (l, r) = (rec(left), rec(right));
                match op {
                    // `+` concatenates when either side is a String; two
                    // Numbers add.
                    BinaryOp::Add => {
                        if l == Some(Kind::Str) || r == Some(Kind::Str) {
                            Some(Kind::Str)
                        } else if l == Some(Kind::Num) && r == Some(Kind::Num) {
                            Some(Kind::Num)
                        } else {
                            None
                        }
                    }
                    // `>>>` has no BigInt form; the others give a Number
                    // whenever one operand is a Number (mixing a BigInt in
                    // throws instead of producing one).
                    BinaryOp::UShr => Some(Kind::Num),
                    _ => (l == Some(Kind::Num) || r == Some(Kind::Num)).then_some(Kind::Num),
                }
            }
            Expr::Unary { op, operand } => match op {
                UnaryOp::Pos => Some(Kind::Num),
                UnaryOp::Neg | UnaryOp::BitNot => {
                    (rec(operand) == Some(Kind::Num)).then_some(Kind::Num)
                }
                UnaryOp::Not => None,
            },
            Expr::Update { id, .. } => (self.local(*id) == Some(Kind::Num)).then_some(Kind::Num),
            Expr::Conditional {
                then_expr,
                else_expr,
                ..
            } => {
                let t = rec(then_expr);
                (t.is_some() && t == rec(else_expr)).then_some(t?)
            }
            Expr::Logical { left, right, .. } => {
                let l = rec(left);
                (l.is_some() && l == rec(right)).then_some(l?)
            }
            Expr::PropertyGet {
                object, property, ..
            } if property == "length" => (rec(object) == Some(Kind::Str)).then_some(Kind::Num),
            Expr::Call { callee, args, .. } => match callee.as_ref() {
                // An IIFE: what every one of its returns is, by its own
                // typed locals (`var t = (function () { var a = []; …;
                // return a; })()`).
                Expr::Closure {
                    func_id,
                    params,
                    body,
                    is_async: false,
                    is_generator: false,
                    ..
                } if args.is_empty() && params.is_empty() => {
                    if let Some(k) = self.iife.borrow().get(func_id) {
                        return *k;
                    }
                    let k = iife_return_kind(body, self.boxed_vars, self.module_globals, depth);
                    self.iife.borrow_mut().insert(*func_id, k);
                    k
                }
                Expr::PropertyGet {
                    object, property, ..
                } if rec(object) == Some(Kind::Str) => string_method_kind(property),
                _ => None,
            },
            _ => None,
        }
    }
}

/// The kind every `return` of an IIFE body produces, when the body cannot
/// fall off its end (its last statement is a `return`).
fn iife_return_kind(
    body: &[Stmt],
    boxed_vars: &HashSet<u32>,
    module_globals: &HashMap<u32, String>,
    depth: usize,
) -> Option<Kind> {
    if depth > 4 || !matches!(body.last(), Some(Stmt::Return(Some(_)))) {
        return None;
    }
    let t = collect_typed_locals(body, &HashMap::new(), boxed_vars, module_globals);
    let iife = std::cell::RefCell::new(HashMap::new());
    let env = Env {
        iife: &iife,
        boxed_vars,
        module_globals,
        proofs: &HashMap::new(),
        strings: &t.strings,
        numbers: &t.numbers,
        arrays: &t.arrays,
    };
    let mut kind: Option<Option<Kind>> = None;
    let mut all = |e: Option<&Expr>| {
        let k = e.and_then(|e| env.kind(e, depth + 4));
        kind = Some(match kind {
            None => k,
            Some(prev) if prev == k => k,
            Some(_) => None,
        });
    };
    fn returns<'a>(stmts: &'a [Stmt], f: &mut dyn FnMut(Option<&'a Expr>)) {
        for s in stmts {
            match s {
                Stmt::Return(e) => f(e.as_ref()),
                Stmt::If {
                    then_branch,
                    else_branch,
                    ..
                } => {
                    returns(then_branch, f);
                    if let Some(b) = else_branch {
                        returns(b, f);
                    }
                }
                Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => returns(body, f),
                Stmt::For { body, .. } => returns(body, f),
                Stmt::Labeled { body, .. } => returns(std::slice::from_ref(body.as_ref()), f),
                Stmt::Try {
                    body,
                    catch,
                    finally,
                } => {
                    returns(body, f);
                    if let Some(c) = catch {
                        returns(&c.body, f);
                    }
                    if let Some(b) = finally {
                        returns(b, f);
                    }
                }
                Stmt::Switch { cases, .. } => {
                    for c in cases {
                        returns(&c.body, f);
                    }
                }
                _ => {}
            }
        }
    }
    returns(body, &mut all);
    kind.flatten()
}

/// What a `String.prototype` method returns, for a String receiver: the
/// lists `string_value_is_runtime_guaranteed` and
/// `string_method_returns_number` trust.
fn string_method_kind(name: &str) -> Option<Kind> {
    match name {
        "toString" | "toLowerCase" | "toUpperCase" | "trim" | "trimStart" | "trimEnd" | "slice"
        | "substring" | "substr" | "charAt" | "repeat" | "replace" | "replaceAll" | "padStart"
        | "padEnd" | "concat" | "normalize" => Some(Kind::Str),
        "charCodeAt" | "indexOf" | "lastIndexOf" | "search" | "localeCompare" => Some(Kind::Num),
        _ => None,
    }
}

/// Locals with a `Let` inside a loop body (closure bodies included).
fn lets_in_loops(stmts: &[Stmt]) -> HashSet<u32> {
    fn walk(stmts: &[Stmt], in_loop: bool, out: &mut HashSet<u32>) {
        for s in stmts {
            match s {
                Stmt::Let { id, init, .. } => {
                    if in_loop {
                        out.insert(*id);
                    }
                    if let Some(e) = init {
                        expr(e, out);
                    }
                }
                Stmt::If {
                    condition,
                    then_branch,
                    else_branch,
                } => {
                    expr(condition, out);
                    walk(then_branch, in_loop, out);
                    if let Some(b) = else_branch {
                        walk(b, in_loop, out);
                    }
                }
                Stmt::While { condition, body } | Stmt::DoWhile { body, condition } => {
                    expr(condition, out);
                    walk(body, true, out);
                }
                Stmt::For { init, body, .. } => {
                    if let Some(i) = init {
                        walk(std::slice::from_ref(i.as_ref()), true, out);
                    }
                    walk(body, true, out);
                }
                Stmt::Labeled { body, .. } => {
                    walk(std::slice::from_ref(body.as_ref()), in_loop, out)
                }
                Stmt::Try {
                    body,
                    catch,
                    finally,
                } => {
                    walk(body, in_loop, out);
                    if let Some(c) = catch {
                        walk(&c.body, in_loop, out);
                    }
                    if let Some(b) = finally {
                        walk(b, in_loop, out);
                    }
                }
                Stmt::Switch { cases, .. } => {
                    for c in cases {
                        walk(&c.body, in_loop, out);
                    }
                }
                other => {
                    perry_hir::walker::stmt_any_expr(other, &mut |e| {
                        expr(e, out);
                        false
                    });
                }
            }
        }
    }
    fn expr(e: &Expr, out: &mut HashSet<u32>) {
        if let Expr::Closure { body, .. } = e {
            // A closure body runs any number of times.
            walk(body, true, out);
            return;
        }
        perry_hir::walker::walk_expr_children(e, &mut |c| expr(c, out));
    }
    let mut out = HashSet::new();
    walk(stmts, false, &mut out);
    out
}

/// Locals some read of which may run before any write (so it may see the
/// `undefined` a `var` starts as). Conservative: a read is safe only when a
/// write to the local dominates it structurally.
fn reads_before_write(stmts: &[Stmt], candidates: &HashSet<u32>) -> HashSet<u32> {
    let mut bad = HashSet::new();
    let mut da = HashSet::new();
    da_stmts(stmts, candidates, &mut da, &mut bad);
    bad
}

fn da_stmts(stmts: &[Stmt], c: &HashSet<u32>, da: &mut HashSet<u32>, bad: &mut HashSet<u32>) {
    for s in stmts {
        da_stmt(s, c, da, bad);
    }
}

fn da_stmt(s: &Stmt, c: &HashSet<u32>, da: &mut HashSet<u32>, bad: &mut HashSet<u32>) {
    match s {
        Stmt::Let { id, init, .. } => match init {
            // The `undefined` seed of a hoisted `var` is not an assignment.
            None | Some(Expr::Undefined) => {
                da.remove(id);
            }
            Some(e) => {
                da_expr(e, c, da, bad);
                da.insert(*id);
            }
        },
        Stmt::Expr(e) | Stmt::Throw(e) => da_expr(e, c, da, bad),
        Stmt::Return(e) => {
            if let Some(e) = e {
                da_expr(e, c, da, bad);
            }
        }
        Stmt::If {
            condition,
            then_branch,
            else_branch,
        } => {
            da_expr(condition, c, da, bad);
            let mut t = da.clone();
            da_stmts(then_branch, c, &mut t, bad);
            let mut f = da.clone();
            if let Some(b) = else_branch {
                da_stmts(b, c, &mut f, bad);
            }
            *da = t.intersection(&f).copied().collect();
        }
        Stmt::While { condition, body } => {
            da_expr(condition, c, da, bad);
            let mut b = da.clone();
            da_stmts(body, c, &mut b, bad);
        }
        Stmt::DoWhile { body, condition } => {
            // The body runs once before the condition, but a `continue`
            // can reach the condition from anywhere in it.
            let mut b = da.clone();
            da_stmts(body, c, &mut b, bad);
            let mut cnd = da.clone();
            da_expr(condition, c, &mut cnd, bad);
        }
        Stmt::For {
            init,
            condition,
            update,
            body,
        } => {
            if let Some(i) = init {
                da_stmt(i, c, da, bad);
            }
            if let Some(cnd) = condition {
                da_expr(cnd, c, da, bad);
            }
            let mut b = da.clone();
            da_stmts(body, c, &mut b, bad);
            // `continue` reaches the update from anywhere in the body.
            if let Some(u) = update {
                let mut u_da = da.clone();
                da_expr(u, c, &mut u_da, bad);
            }
        }
        Stmt::Labeled { body, .. } => {
            let mut b = da.clone();
            da_stmt(body, c, &mut b, bad);
        }
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            let mut b = da.clone();
            da_stmts(body, c, &mut b, bad);
            if let Some(cc) = catch {
                let mut k = da.clone();
                da_stmts(&cc.body, c, &mut k, bad);
            }
            if let Some(f) = finally {
                let mut k = da.clone();
                da_stmts(f, c, &mut k, bad);
            }
        }
        Stmt::Switch {
            discriminant,
            cases,
        } => {
            da_expr(discriminant, c, da, bad);
            for case in cases {
                let mut k = da.clone();
                if let Some(t) = &case.test {
                    da_expr(t, c, &mut k, bad);
                }
                da_stmts(&case.body, c, &mut k, bad);
            }
        }
        _ => {}
    }
}

/// Reads in evaluation order; a write inside a conditional part
/// (`&&`, `||`, `??`, `?:`) does not count after it.
fn da_expr(e: &Expr, c: &HashSet<u32>, da: &mut HashSet<u32>, bad: &mut HashSet<u32>) {
    match e {
        Expr::LocalGet(id) => {
            if c.contains(id) && !da.contains(id) {
                bad.insert(*id);
            }
        }
        Expr::Update { id, .. } => {
            if c.contains(id) && !da.contains(id) {
                bad.insert(*id);
            }
        }
        Expr::LocalSet(id, v) => {
            da_expr(v, c, da, bad);
            da.insert(*id);
        }
        Expr::Logical { left, right, .. } => {
            da_expr(left, c, da, bad);
            let mut r = da.clone();
            da_expr(right, c, &mut r, bad);
        }
        Expr::Conditional {
            condition,
            then_expr,
            else_expr,
        } => {
            da_expr(condition, c, da, bad);
            let mut t = da.clone();
            da_expr(then_expr, c, &mut t, bad);
            let mut f = da.clone();
            da_expr(else_expr, c, &mut f, bad);
            *da = t.intersection(&f).copied().collect();
        }
        Expr::Closure {
            captures,
            mutable_captures,
            body,
            ..
        } => {
            // Creating the closure reads what it captures (by value unless
            // boxed, and boxed locals are never candidates).
            let mut read = |id: &u32| {
                if c.contains(id) && !da.contains(id) {
                    bad.insert(*id);
                }
            };
            captures
                .iter()
                .chain(mutable_captures.iter())
                .for_each(&mut read);
            fn gets(stmts: &[Stmt], f: &mut dyn FnMut(&u32)) {
                fn e(x: &Expr, f: &mut dyn FnMut(&u32)) {
                    match x {
                        Expr::LocalGet(id) | Expr::Update { id, .. } => f(id),
                        Expr::Closure { body, .. } => gets(body, f),
                        _ => {}
                    }
                    perry_hir::walker::walk_expr_children(x, &mut |y| e(y, f));
                }
                for s in stmts {
                    perry_hir::walker::stmt_any_expr(s, &mut |x| {
                        e(x, f);
                        false
                    });
                }
            }
            gets(body, &mut read);
        }
        _ => perry_hir::walker::walk_expr_children(e, &mut |x| da_expr(x, c, da, bad)),
    }
}

/// The typed locals of `body` under the parameter `proofs` (empty for a
/// generic body).
pub(crate) fn collect_typed_locals(
    body: &[Stmt],
    proofs: &HashMap<u32, HirType>,
    boxed_vars: &HashSet<u32>,
    module_globals: &HashMap<u32, String>,
) -> TypedLocals {
    if !enabled() {
        return TypedLocals::default();
    }
    let mut writes: HashMap<u32, Vec<Option<&Expr>>> = HashMap::new();
    let mut let_bound: HashSet<u32> = HashSet::new();
    collect_live_writes(body, proofs, &mut writes, &mut let_bound);
    let mut updates = HashSet::new();
    let mut in_place = HashSet::new();
    let mut poisoned = HashSet::new();
    collect_other_writes(body, &mut updates, &mut in_place, &mut poisoned);
    // A boxed local (captured by a closure of this body) is still written
    // only in this body and the closures inside it, all in this walk, so it
    // may carry a String or Array proof; a closure reads it no earlier than
    // its creation (a read `reads_before_write` checks). It is never a
    // Number candidate (that would change its storage), and a binding
    // declared inside a loop is left out (each iteration's binding is new).
    let in_loops = lets_in_loops(body);
    let boxed_ok = |id: &u32| !boxed_vars.contains(id) || !in_loops.contains(id);
    let base: HashSet<u32> = let_bound
        .into_iter()
        .filter(|id| {
            boxed_ok(id)
                && !module_globals.contains_key(id)
                && !poisoned.contains(id)
                && !proofs.contains_key(id)
        })
        .collect();
    // A local dropped for a read before its first write can take others
    // down with it (`c = s.charCodeAt(0)` needs `s`): rerun the fixed points
    // without it until no read precedes a write.
    let iife = std::cell::RefCell::new(HashMap::new());
    let mut excluded: HashSet<u32> = HashSet::new();
    let (strings, numbers, arrays, candidates) = loop {
        let candidates: HashSet<u32> = base.difference(&excluded).copied().collect();
        let all_ok = |id: u32, env: &Env<'_>, k: Kind| {
            writes
                .get(&id)
                .is_some_and(|ws| ws.iter().all(|w| w.is_some_and(|e| env.is(e, k))))
        };
        let empty = HashSet::new();
        // Arrays first (no Array rule depends on another kind), then Strings
        // (no String rule depends on a Number local), then Numbers.
        let mut arrays: HashSet<u32> = candidates
            .iter()
            .copied()
            .filter(|id| !updates.contains(id))
            .collect();
        loop {
            let env = Env {
                iife: &iife,
                boxed_vars,
                module_globals,
                proofs,
                strings: &empty,
                numbers: &empty,
                arrays: &arrays,
            };
            let drop: Vec<u32> = arrays
                .iter()
                .copied()
                .filter(|id| !all_ok(*id, &env, Kind::Arr))
                .collect();
            if drop.is_empty() {
                break;
            }
            for id in drop {
                arrays.remove(&id);
            }
        }
        let mut strings: HashSet<u32> = candidates
            .iter()
            .copied()
            .filter(|id| !updates.contains(id) && !in_place.contains(id) && !arrays.contains(id))
            .collect();
        loop {
            let env = Env {
                iife: &iife,
                boxed_vars,
                module_globals,
                proofs,
                strings: &strings,
                numbers: &empty,
                arrays: &arrays,
            };
            let drop: Vec<u32> = strings
                .iter()
                .copied()
                .filter(|id| !all_ok(*id, &env, Kind::Str))
                .collect();
            if drop.is_empty() {
                break;
            }
            for id in drop {
                strings.remove(&id);
            }
        }
        let mut numbers: HashSet<u32> = candidates
            .iter()
            .copied()
            .filter(|id| {
                !in_place.contains(id)
                    && !strings.contains(id)
                    && !arrays.contains(id)
                    && !boxed_vars.contains(id)
            })
            .collect();
        loop {
            let env = Env {
                iife: &iife,
                boxed_vars,
                module_globals,
                proofs,
                strings: &strings,
                numbers: &numbers,
                arrays: &arrays,
            };
            let drop: Vec<u32> = numbers
                .iter()
                .copied()
                .filter(|id| !all_ok(*id, &env, Kind::Num))
                .collect();
            if drop.is_empty() {
                break;
            }
            for id in drop {
                numbers.remove(&id);
            }
        }
        let mut typed: HashSet<u32> = strings.union(&numbers).copied().collect();
        typed.extend(arrays.iter().copied());
        let bad: HashSet<u32> = if typed.is_empty() {
            HashSet::new()
        } else {
            reads_before_write(body, &typed)
        };
        if bad.is_empty() {
            break (strings, numbers, arrays, candidates);
        }
        excluded.extend(bad);
    };
    if std::env::var_os("PERRY_TYPED_LOCALS_TRACE").is_some() {
        let mut s: Vec<_> = strings.iter().collect();
        let mut n: Vec<_> = numbers.iter().collect();
        s.sort();
        n.sort();
        eprintln!(
            "[typed_locals] proofs={:?} candidates={} strings={s:?} numbers={n:?} arrays={}",
            proofs.keys().collect::<Vec<_>>(),
            candidates.len(),
            arrays.len()
        );
    }
    TypedLocals {
        strings,
        numbers,
        arrays,
    }
}

/// The proofs among `proofs` that hold for the whole body: a parameter's
/// entry proof survives only while the body never rebinds it. A spec
/// clone's Number proof admits an int32 box as well as a plain double, so it
/// is not a Number by construction here and is left out.
pub(crate) fn stable_param_proofs(
    proofs: &HashMap<u32, HirType>,
    body: &[Stmt],
    boxed_vars: &HashSet<u32>,
) -> HashMap<u32, HirType> {
    let rebound = crate::collectors::reassigned_locals(body);
    proofs
        .iter()
        .filter(|(id, ty)| {
            !matches!(ty, HirType::Number | HirType::Int32)
                && !boxed_vars.contains(id)
                && !rebound.contains(id)
                && !crate::collectors::has_any_mutation(body, **id)
        })
        .map(|(id, ty)| (*id, ty.clone()))
        .collect()
}

/// The parameters among a typed body's entry `proofs` that it proved Number.
/// The plan admits only parameters the body never rebinds or mutates, and the
/// dispatcher hands an int32 box over as a plain double, so such a parameter
/// holds a non-pointer for the whole body: it joins `number_by_construction`
/// here and gets no root slot (`codegen/closure.rs`, before the parameter
/// slots are bound).
pub(crate) fn number_proof_params(
    proofs: &HashMap<u32, HirType>,
) -> impl Iterator<Item = u32> + '_ {
    proofs
        .iter()
        .filter_map(|(id, ty)| matches!(ty, HirType::Number | HirType::Int32).then_some(*id))
}

/// Run the analysis for one compiled body: Number locals (and Number
/// parameters proven at entry) join `number_by_construction`, and the
/// returned map is the body's `FnCtx::typed_locals`.
pub(crate) fn apply(
    body: &[Stmt],
    proofs: &HashMap<u32, HirType>,
    captures: Option<&HashMap<u32, HirType>>,
    boxed_vars: &HashSet<u32>,
    module_globals: &HashMap<u32, String>,
    number_by_construction: &mut HashSet<u32>,
) -> HashMap<u32, HirType> {
    let mut all = proofs.clone();
    if let Some(c) = captures {
        for (id, ty) in c {
            all.entry(*id).or_insert_with(|| ty.clone());
        }
    }
    let t = collect_typed_locals(body, &all, boxed_vars, module_globals);
    number_by_construction.extend(t.numbers.iter().copied());
    number_by_construction.extend(number_proof_params(proofs));
    // Numbers answer through `number_by_construction` alone (THE Number
    // query); the map carries the String and Array proofs.
    let mut m: HashMap<u32, HirType> = captures
        .map(|c| {
            c.iter()
                .filter(|(_, ty)| !matches!(ty, HirType::Number | HirType::Int32))
                .map(|(id, ty)| (*id, ty.clone()))
                .collect()
        })
        .unwrap_or_default();
    m.extend(t.strings.iter().map(|id| (*id, HirType::String)));
    m.extend(
        t.arrays
            .iter()
            .map(|id| (*id, HirType::Array(Box::new(HirType::Any)))),
    );
    m
}

fn as_types(t: &TypedLocals) -> HashMap<u32, HirType> {
    let mut m = HashMap::new();
    m.extend(t.strings.iter().map(|id| (*id, HirType::String)));
    m.extend(t.numbers.iter().map(|id| (*id, HirType::Number)));
    m.extend(
        t.arrays
            .iter()
            .map(|id| (*id, HirType::Array(Box::new(HirType::Any)))),
    );
    m
}

/// Per closure (`func_id`): the types of the locals it captures from the
/// bodies around it. A local typed in its own body holds that type at every
/// read there, and creating a closure that reads it is such a read, so the
/// value the closure sees has it too: the local is never boxed, so no later
/// write can reach the closure (a write after capture would box it).
pub(crate) fn capture_proofs(
    hir: &perry_hir::Module,
    boxed_vars: &HashSet<u32>,
    module_globals: &HashMap<u32, String>,
) -> HashMap<u32, HashMap<u32, HirType>> {
    let mut out = HashMap::new();
    if !enabled() {
        return out;
    }
    fn visit_body(
        body: &[Stmt],
        outer: &HashMap<u32, HirType>,
        boxed: &HashSet<u32>,
        globals: &HashMap<u32, String>,
        out: &mut HashMap<u32, HashMap<u32, HirType>>,
    ) {
        let mut here = outer.clone();
        here.extend(as_types(&collect_typed_locals(
            body,
            &HashMap::new(),
            boxed,
            globals,
        )));
        for s in body {
            perry_hir::walker::stmt_any_expr(s, &mut |e| {
                visit_expr(e, &here, boxed, globals, out);
                false
            });
        }
    }
    fn visit_expr(
        e: &Expr,
        env: &HashMap<u32, HirType>,
        boxed: &HashSet<u32>,
        globals: &HashMap<u32, String>,
        out: &mut HashMap<u32, HashMap<u32, HirType>>,
    ) {
        if let Expr::Closure { func_id, body, .. } = e {
            if !env.is_empty() {
                // Keep only what the closure (or a closure inside it) reads.
                let mut used = HashSet::new();
                fn reads(stmts: &[Stmt], used: &mut HashSet<u32>) {
                    fn r(x: &Expr, used: &mut HashSet<u32>) {
                        match x {
                            Expr::LocalGet(id) => {
                                used.insert(*id);
                            }
                            Expr::Closure { body, .. } => reads(body, used),
                            _ => {}
                        }
                        perry_hir::walker::walk_expr_children(x, &mut |y| r(y, used));
                    }
                    for s in stmts {
                        perry_hir::walker::stmt_any_expr(s, &mut |x| {
                            r(x, used);
                            false
                        });
                    }
                }
                reads(body, &mut used);
                let mine: HashMap<u32, HirType> = env
                    .iter()
                    .filter(|(id, _)| used.contains(id))
                    .map(|(id, ty)| (*id, ty.clone()))
                    .collect();
                if !mine.is_empty() {
                    out.insert(*func_id, mine);
                }
            }
            visit_body(body, env, boxed, globals, out);
            return;
        }
        perry_hir::walker::walk_expr_children(e, &mut |c| visit_expr(c, env, boxed, globals, out));
    }
    let none = HashMap::new();
    visit_body(&hir.init, &none, boxed_vars, module_globals, &mut out);
    for f in &hir.functions {
        visit_body(&f.body, &none, boxed_vars, module_globals, &mut out);
    }
    for c in &hir.classes {
        let fns = c
            .constructor
            .iter()
            .chain(c.methods.iter())
            .chain(c.getters.iter().map(|(_, f)| f))
            .chain(c.setters.iter().map(|(_, f)| f))
            .chain(c.static_methods.iter());
        for f in fns {
            visit_body(&f.body, &none, boxed_vars, module_globals, &mut out);
        }
    }
    out
}

#[cfg(test)]
#[path = "typed_locals_tests.rs"]
mod tests;
