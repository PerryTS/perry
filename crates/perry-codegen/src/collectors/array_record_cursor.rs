//! The for…of IteratorRecord cursor is a u32 by construction.
//!
//! perry-hir lowers `for (x of a)` over an ordinary array to an
//! IteratorRecord (`perry-hir/src/iterator_record.rs`) whose indexed mode
//! steps a private cursor:
//!
//! ```text
//! Let c = arrayRecordIndex(0)
//! For { init: Let c = arrayRecordIndex(0);
//!       cond: c < arrayRecordForBound(m, L, c, step, state);
//!       update: arrayRecordForUpdate(m, c++) }
//! ```
//!
//! with `L` the record's length: `arrayRecordLength(payload)` (the live
//! length of an Array, a `u32`, at most 2^32-1) or the element count of a
//! dense literal. The cursor stays a JS Number in HIR because a signed i32
//! would wrap at 2^31. It is still exactly a `u32`:
//!
//! * Indexed mode (`m` false): `c++` runs only as the loop update, i.e.
//!   after the condition `c < L` held on this iteration (the body's end or a
//!   `continue`). So the new value is at most `L <= 2^32-1`.
//! * Iterator mode (`m` true): `arrayRecordForUpdate` yields 0 without
//!   evaluating `c++`, and the bound reads `c` only to compare it with itself
//!   or `+Infinity`. The cursor stays 0. This holds whatever the program did
//!   to `%ArrayIteratorPrototype%`, so the proof needs no protocol fact.
//!
//! A cursor qualifies only when every write to it is one of those two forms;
//! any other write (a destructuring advance, a `LocalSet`, an `Update`
//! elsewhere) leaves it a double. Qualifying cursors join
//! `unsigned_i32_locals`, whose consumers already treat the i32 slot as a
//! u32 bit pattern (`uitofp` at boxed reads, `LoweredValue::u32` operands).
use std::collections::HashSet;

use perry_hir::{CompareOp, Expr, Stmt, UpdateOp};

pub(crate) fn collect_array_record_cursor_locals(stmts: &[Stmt]) -> HashSet<u32> {
    let mut scan = Scan::default();
    scan.stmts(stmts);
    scan.candidates
        .into_iter()
        .filter(|id| !scan.disqualified.contains(id))
        .collect()
}

#[derive(Default)]
struct Scan {
    candidates: HashSet<u32>,
    disqualified: HashSet<u32>,
}

fn runtime_call<'a>(e: &'a Expr, name: &str) -> Option<&'a [Expr]> {
    match e {
        Expr::NativeMethodCall {
            module,
            class_name: None,
            object: None,
            method,
            args,
        } if module == "__perry_runtime" && method == name => Some(args),
        _ => None,
    }
}

fn is_cursor_init(e: &Expr) -> bool {
    matches!(runtime_call(e, "arrayRecordIndex"), Some([Expr::Integer(0)]))
}

/// The record's mode test: `use_iter == true`.
fn mode_local(e: &Expr) -> Option<u32> {
    match e {
        Expr::Compare {
            op: CompareOp::Eq,
            left,
            right,
        } if matches!(right.as_ref(), Expr::Bool(true)) => match left.as_ref() {
            Expr::LocalGet(id) => Some(*id),
            _ => None,
        },
        _ => None,
    }
}

/// The cursor a `For` steps under the record's bound, when its condition and
/// update are exactly the record's: `c < ForBound(m, L, c, ..)` and
/// `ForUpdate(m, c++)` with one mode local and `L` a u32 length.
fn bounded_record_cursor(condition: Option<&Expr>, update: Option<&Expr>) -> Option<u32> {
    let Some(Expr::Compare {
        op: CompareOp::Lt,
        left,
        right,
    }) = condition
    else {
        return None;
    };
    let Expr::LocalGet(cursor) = left.as_ref() else {
        return None;
    };
    let [mode, length, Expr::LocalGet(compared), ..] = runtime_call(right, "arrayRecordForBound")?
    else {
        return None;
    };
    let length_is_u32 = match length {
        Expr::Integer(n) => u32::try_from(*n).is_ok(),
        other => matches!(runtime_call(other, "arrayRecordLength"), Some([Expr::LocalGet(_)])),
    };
    let [update_mode, Expr::Update {
        id,
        op: UpdateOp::Increment,
        ..
    }] = runtime_call(update?, "arrayRecordForUpdate")?
    else {
        return None;
    };
    let mode = mode_local(mode)?;
    (compared == cursor
        && id == cursor
        && length_is_u32
        && mode_local(update_mode) == Some(mode)
        && *cursor != mode)
        .then_some(*cursor)
}

impl Scan {
    fn stmts(&mut self, stmts: &[Stmt]) {
        for stmt in stmts {
            self.stmt(stmt);
        }
    }

    fn stmt(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::Let { id, mutable, init, .. } => {
                match init {
                    Some(init) if *mutable && is_cursor_init(init) => {
                        self.candidates.insert(*id);
                    }
                    // A redeclaration with any other initializer is a write
                    // this proof does not cover.
                    Some(_) => {
                        self.disqualified.insert(*id);
                    }
                    None => {}
                }
                if let Some(init) = init {
                    self.expr(init);
                }
            }
            Stmt::For {
                init,
                condition,
                update,
                body,
            } => {
                if let Some(init) = init {
                    self.stmt(init);
                }
                if let Some(condition) = condition {
                    self.expr(condition);
                }
                if let Some(update) = update {
                    match bounded_record_cursor(condition.as_ref(), Some(update)) {
                        // The licensed `c++`: walk the mode operand only.
                        Some(_) => {
                            if let Some(args) = runtime_call(update, "arrayRecordForUpdate") {
                                self.expr(&args[0]);
                            }
                        }
                        None => self.expr(update),
                    }
                }
                self.stmts(body);
            }
            other => {
                // Every other statement: its own expressions plus nested
                // statements (each nested `For` reaches the arm above).
                match other {
                    Stmt::If {
                        condition,
                        then_branch,
                        else_branch,
                    } => {
                        self.expr(condition);
                        self.stmts(then_branch);
                        if let Some(else_branch) = else_branch {
                            self.stmts(else_branch);
                        }
                    }
                    Stmt::While { condition, body } | Stmt::DoWhile { condition, body } => {
                        self.expr(condition);
                        self.stmts(body);
                    }
                    Stmt::Labeled { body, .. } => self.stmt(body),
                    Stmt::Try {
                        body,
                        catch,
                        finally,
                    } => {
                        self.stmts(body);
                        if let Some(catch) = catch {
                            self.stmts(&catch.body);
                        }
                        if let Some(finally) = finally {
                            self.stmts(finally);
                        }
                    }
                    Stmt::Switch {
                        discriminant,
                        cases,
                    } => {
                        self.expr(discriminant);
                        for case in cases {
                            if let Some(test) = &case.test {
                                self.expr(test);
                            }
                            self.stmts(&case.body);
                        }
                    }
                    Stmt::Expr(e) | Stmt::Throw(e) => self.expr(e),
                    Stmt::Return(e) => {
                        if let Some(e) = e {
                            self.expr(e);
                        }
                    }
                    Stmt::Let { .. } | Stmt::For { .. } => unreachable!("handled above"),
                    Stmt::Break
                    | Stmt::Continue
                    | Stmt::LabeledBreak(_)
                    | Stmt::LabeledContinue(_)
                    | Stmt::PreallocateBoxes(_)
                    | Stmt::PreallocateTdzBoxes(_)
                    | Stmt::ReleaseBoxes(_) => {}
                }
            }
        }
    }

    fn expr(&mut self, e: &Expr) {
        match e {
            Expr::LocalSet(id, _) | Expr::Update { id, .. } => {
                self.disqualified.insert(*id);
            }
            // A nested function body writes through its captures; walk it so
            // such a write disqualifies too.
            Expr::Closure { body, .. } => self.stmts(body),
            _ => {}
        }
        perry_hir::walker::walk_expr_children(e, &mut |child| self.expr(child));
    }
}

#[cfg(test)]
#[path = "array_record_cursor_tests.rs"]
mod tests;
