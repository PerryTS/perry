//! In-bounds index stores: `a[k] = v` where `k` is at most the length of
//! the Array `a` holds, so the store overwrites an element or appends and
//! leaves no hole (`Cons::IndexKept`, `Model::index_kept`).
//!
//! An index store may write past the end of an Array and leave a hole, so
//! every one marks the Array's iteration as yielding `undefined`
//! (`Model::mark_hole`). The interval of the key cannot rule that out for
//! an Array whose length the program decides at run time (a `[]` grown by
//! `push`, a `slice` copy): the key is bounded by the length, not by a
//! constant. This module proves the relation at the store, from the text:
//!
//! - a **length guard** `i < a.length` that is true for the code being
//!   walked (`gen/narrow.rs`: a `for` test, an `if`, a `while`, `&&`, `?:`,
//!   an early exit), with `i` not written since it was tested
//!   (`Narrow::written`), and
//! - a key that is at most `i`: `i` itself, `i - c` (`c >= 0`), or `j`,
//!   `j + c`, `j - c` for a local `j` **derived below** `i`: declared in the
//!   guarded code as `let j = i - c0` (`c0 >= 0`, or `let j = i`), whose
//!   every other write is a decrement (`j--`, `--j`, `j = j - c`), so
//!   `j <= i - c0` for as long as `i` is not written; `j + c` needs
//!   `c <= c0`.
//!
//! So `k <= i < length(a)` held when the guard ran, with the value `a` and
//! `i` still hold at the store. What is left is the Array's own length
//! between the guard and the store, which only the solve knows: the store
//! keeps an Array hole-free only while nothing may shrink it
//! (`Model::arr_grow_only`), whatever name or callee would do it.
//!
//! The locals must be the walked body's own:
//! - `a` is never written after its declaration (`stable`) and no closure
//!   writes it (`narrow::scope_locals`, or a `const`);
//! - `i` holds a Number or BigInt by construction (`numeric`: every write
//!   is an arithmetic expression or an update), never an object whose
//!   `valueOf` could answer the guard and the key differently;
//! - `j` is declared by `let` in the guarded code and is used after its
//!   declaration in the same block (the declaration runs again before each
//!   use in a loop body).
//!
//! A Number key that is no index (negative, a fraction, NaN) is a named
//! property: it moves neither the length nor the elements, so the proof
//! needs no lower bound and no integrality. The solve drops the proof for
//! a key that may be anything but a Number (`Cons::StoreIndexed`).
//!
//! `PERRY_FACTS_INBOUNDS_STORE=0` turns it off. Probe: one
//! `# inbounds_store` line per proven store site (`probe`).

use super::*;
use perry_hir::{BinaryOp, Expr, Stmt, UpdateOp};

/// `PERRY_FACTS_INBOUNDS_STORE=0` (`off`, `false`) turns the in-bounds
/// stores off: every index store may leave a hole again.
pub(crate) fn enabled() -> bool {
    !matches!(
        std::env::var("PERRY_FACTS_INBOUNDS_STORE").as_deref(),
        Ok("0") | Ok("off") | Ok("false")
    )
}

/// What the walked body's text says of its locals (`scan`).
pub(crate) struct BodyLocals {
    f: FnIdx,
    /// Declared here (a parameter or a `let`) and never written again.
    stable: crate::pts::FxHashSet<u32>,
    /// Declared by `let` with an arithmetic initializer, and every write
    /// is arithmetic or an update: a Number or BigInt at every read.
    numeric: crate::pts::FxHashSet<u32>,
    /// Declared by `let`, and every later write is a decrement.
    falling: crate::pts::FxHashSet<u32>,
}

/// `j <= i - c0` while narrowing `narrow` of `i` is not written.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Derived {
    mi: u32,
    j: u32,
    i: u32,
    c0: i64,
    /// Index of the narrowing of `i` in `Gen::narrow` that was live and
    /// unwritten at the declaration.
    narrow: usize,
    f: FnIdx,
}

#[derive(Default)]
pub(crate) struct InBounds {
    pub(crate) body: Option<BodyLocals>,
    pub(crate) derived: Vec<Derived>,
    /// Proven store sites: (module, byte offset of the store, receiver
    /// node), for the probe.
    pub(crate) proven: Vec<(u32, u32, NodeId)>,
}

/// A non-negative integer literal.
fn nonneg_lit(e: &Expr) -> Option<i64> {
    match e {
        Expr::Integer(v) if *v >= 0 => Some(*v),
        Expr::Number(v) if *v >= 0.0 && v.fract() == 0.0 && *v < 9.0e15 => Some(*v as i64),
        _ => None,
    }
}

/// An expression whose value is a Number or a BigInt whatever its operands
/// hold (or it throws): a literal, an arithmetic or bitwise operator other
/// than `+`, a `+` of two such expressions, an update.
fn arithmetic(e: &Expr) -> bool {
    match e {
        Expr::Integer(_) | Expr::Number(_) | Expr::Update { .. } => true,
        Expr::Binary {
            op: BinaryOp::Add,
            left,
            right,
        } => arithmetic(left) && arithmetic(right),
        Expr::Binary { .. } => true,
        _ => false,
    }
}

/// `j = j - c` with `c >= 0`.
fn decrement(id: u32, v: &Expr) -> bool {
    matches!(v, Expr::Binary { op: BinaryOp::Sub, left, right }
        if matches!(left.as_ref(), Expr::LocalGet(l) if *l == id) && nonneg_lit(right).is_some())
}

#[derive(Default)]
struct Scan {
    declared: crate::pts::FxHashSet<u32>,
    lets: crate::pts::FxHashSet<u32>,
    written: crate::pts::FxHashSet<u32>,
    not_numeric: crate::pts::FxHashSet<u32>,
    not_falling: crate::pts::FxHashSet<u32>,
}

impl Scan {
    fn expr(&mut self, e: &Expr) {
        match e {
            Expr::LocalSet(id, v) => {
                self.written.insert(*id);
                if !arithmetic(v) {
                    self.not_numeric.insert(*id);
                }
                if !decrement(*id, v) {
                    self.not_falling.insert(*id);
                }
            }
            Expr::Update { id, op, .. } => {
                self.written.insert(*id);
                if !matches!(op, UpdateOp::Decrement) {
                    self.not_falling.insert(*id);
                }
            }
            _ => {}
        }
        perry_hir::walker::walk_expr_children(e, &mut |c| self.expr(c));
    }

    fn stmts(&mut self, ss: &[Stmt]) {
        for s in ss {
            self.stmt(s);
        }
    }

    fn stmt(&mut self, s: &Stmt) {
        match s {
            Stmt::Let { id, init, .. } => {
                self.declared.insert(*id);
                self.lets.insert(*id);
                if !init.as_ref().is_some_and(arithmetic) {
                    self.not_numeric.insert(*id);
                }
                if let Some(e) = init {
                    self.expr(e);
                }
            }
            Stmt::Expr(e) | Stmt::Throw(e) => self.expr(e),
            Stmt::Return(e) => {
                if let Some(e) = e {
                    self.expr(e);
                }
            }
            Stmt::If {
                condition,
                then_branch,
                else_branch,
            } => {
                self.expr(condition);
                self.stmts(then_branch);
                if let Some(e) = else_branch {
                    self.stmts(e);
                }
            }
            Stmt::While { condition, body } | Stmt::DoWhile { condition, body } => {
                self.expr(condition);
                self.stmts(body);
            }
            Stmt::For {
                init,
                condition,
                update,
                body,
            } => {
                if let Some(i) = init {
                    self.stmt(i);
                }
                if let Some(c) = condition {
                    self.expr(c);
                }
                if let Some(u) = update {
                    self.expr(u);
                }
                self.stmts(body);
            }
            Stmt::Labeled { body, .. } => self.stmt(body),
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                self.stmts(body);
                if let Some(c) = catch {
                    // The catch binding is written by the throw.
                    if let Some((id, _)) = &c.param {
                        self.written.insert(*id);
                        self.not_numeric.insert(*id);
                        self.not_falling.insert(*id);
                    }
                    self.stmts(&c.body);
                }
                if let Some(f) = finally {
                    self.stmts(f);
                }
            }
            Stmt::Switch {
                discriminant,
                cases,
            } => {
                self.expr(discriminant);
                for c in cases {
                    if let Some(t) = &c.test {
                        self.expr(t);
                    }
                    self.stmts(&c.body);
                }
            }
            // Any other statement holds no expression of its own
            // (`walker::stmt_any_expr` visits each one this match names).
            other => {
                let _ = perry_hir::walker::stmt_any_expr(other, &mut |e| {
                    self.expr(e);
                    false
                });
            }
        }
    }
}

/// The body's own locals by how its text writes them.
fn scan(f: FnIdx, params: &[perry_hir::Param], body: &[Stmt]) -> BodyLocals {
    let mut s = Scan::default();
    for p in params {
        s.declared.insert(p.id);
    }
    s.stmts(body);
    let Scan {
        declared,
        lets,
        written,
        not_numeric,
        not_falling,
    } = s;
    BodyLocals {
        f,
        stable: declared
            .iter()
            .filter(|id| !written.contains(id))
            .copied()
            .collect(),
        numeric: lets
            .iter()
            .filter(|id| !not_numeric.contains(id))
            .copied()
            .collect(),
        falling: lets
            .iter()
            .filter(|id| !not_falling.contains(id))
            .copied()
            .collect(),
    }
}

impl<'a> Gen<'a> {
    /// Entering a function body (`narrow_enter_body`): what its text says
    /// of its locals; the enclosing body's, to restore on exit.
    pub(crate) fn in_bounds_enter_body(
        &mut self,
        f: FnIdx,
        params: &[perry_hir::Param],
        body: &[Stmt],
    ) -> (Option<BodyLocals>, Vec<Derived>) {
        let own = self.m.inbounds_store.then(|| scan(f, params, body));
        (
            std::mem::replace(&mut self.in_bounds.body, own),
            std::mem::take(&mut self.in_bounds.derived),
        )
    }

    pub(crate) fn in_bounds_exit_body(&mut self, saved: (Option<BodyLocals>, Vec<Derived>)) {
        self.in_bounds.body = saved.0;
        self.in_bounds.derived = saved.1;
    }

    /// Local `id` of the walked body that no other code can write.
    fn in_bounds_own(&self, mi: u32, id: u32) -> bool {
        self.narrow_scope
            .as_ref()
            .is_some_and(|(_, ls)| ls.contains(&id))
            || self.narrow_consts.contains(&(mi, id))
    }

    /// The index in `Gen::narrow` of a live narrowing of `i` (of this
    /// function) that is not written, and whose test (when `array` is
    /// given) is `i < array.length`.
    fn in_bounds_guard(&self, mi: u32, i: u32, array: Option<u32>) -> Option<usize> {
        let f = self.m.cur_fn;
        self.narrow.iter().enumerate().rev().find_map(|(k, n)| {
            let (nmi, nid, nf) = n.key();
            (nmi == mi
                && nid == i
                && nf == f
                && n.unwritten()
                && array.is_none_or(|a| n.below_length_of() == Some(a)))
            .then_some(k)
        })
    }

    /// After `let j = init` is walked: `j` is derived below a guarded
    /// counter `i` when `init` is `i` or `i - c0` (module docs).
    pub(crate) fn in_bounds_let(&mut self, fx: &Fx, j: u32, init: Option<&Expr>) {
        let Some(body) = &self.in_bounds.body else {
            return;
        };
        if body.f != fx.f || self.m.cur_fn != fx.f || !body.falling.contains(&j) {
            return;
        }
        let (i, c0) = match init {
            Some(Expr::LocalGet(i)) => (*i, 0),
            Some(Expr::Binary {
                op: BinaryOp::Sub,
                left,
                right,
            }) => match (left.as_ref(), nonneg_lit(right)) {
                (Expr::LocalGet(i), Some(c)) => (*i, c),
                _ => return,
            },
            _ => return,
        };
        if !body.numeric.contains(&i) {
            return;
        }
        let mi = fx.module;
        if !self.in_bounds_own(mi, j) || !self.in_bounds_own(mi, i) {
            return;
        }
        let Some(narrow) = self.in_bounds_guard(mi, i, None) else {
            return;
        };
        self.in_bounds.derived.push(Derived {
            mi,
            j,
            i,
            c0,
            narrow,
            f: fx.f,
        });
    }

    /// The counter `i` with `key <= i` now, by the text (module docs).
    fn in_bounds_key(&self, mi: u32, key: &Expr) -> Option<u32> {
        let f = self.m.cur_fn;
        // `x + c`: the local and the constant added to it.
        let (x, c) = match key {
            Expr::LocalGet(x) => (*x, 0),
            Expr::Binary { op, left, right } => match (op, left.as_ref(), nonneg_lit(right)) {
                (BinaryOp::Add, Expr::LocalGet(x), Some(c)) => (*x, c),
                (BinaryOp::Sub, Expr::LocalGet(x), Some(c)) => (*x, -c),
                _ => return None,
            },
            _ => return None,
        };
        // `x` is derived below `i`: `x + c <= i - c0 + c`.
        let derived = self.in_bounds.derived.iter().rev().find(|d| {
            d.mi == mi
                && d.j == x
                && d.f == f
                && self
                    .narrow
                    .get(d.narrow)
                    .is_some_and(|n| n.key() == (mi, d.i, f) && n.unwritten())
        });
        match derived {
            Some(d) => (c <= d.c0).then_some(d.i),
            None => (c <= 0).then_some(x),
        }
    }

    /// After the store `e` (`object[index] = v` into `recv` under `key`) is
    /// generated at `site`: the site is in bounds when the text proves it
    /// (`in_bounds_store`). The solve reads the verdict
    /// (`Model::inbounds_sites`).
    pub(crate) fn in_bounds_site(
        &mut self,
        fx: &Fx,
        e: &Expr,
        recv: NodeId,
        key: Key,
        site: SiteId,
    ) {
        let Expr::IndexSet {
            object,
            index,
            element_offset,
            ..
        } = e
        else {
            return;
        };
        if matches!(key, Key::Any(_)) && self.in_bounds_store(fx, object, index) {
            self.m.inbounds_sites.insert(site);
            self.in_bounds
                .proven
                .push((fx.module, *element_offset, recv));
        }
    }

    /// Whether the store `a[key] = v` being walked is in bounds by the
    /// text: `key <= i < a.length` held at a guard of the walked code, and
    /// `a` and `i` hold the values it tested (module docs).
    fn in_bounds_store(&self, fx: &Fx, object: &Expr, key: &Expr) -> bool {
        let Some(body) = &self.in_bounds.body else {
            return false;
        };
        if body.f != fx.f || self.m.cur_fn != fx.f {
            return false;
        }
        let Expr::LocalGet(a) = object else {
            return false;
        };
        let mi = fx.module;
        let Some(i) = self.in_bounds_key(mi, key) else {
            return false;
        };
        body.stable.contains(a)
            && body.numeric.contains(&i)
            && self.in_bounds_own(mi, *a)
            && self.in_bounds_own(mi, i)
            && self.in_bounds_guard(mi, i, Some(*a)).is_some()
    }
}

/// The probe (`PERRY_FACTS_DUMP`): one `# inbounds_store` line per store
/// site gen proved in bounds, with the solve's verdict over the Arrays its
/// receiver holds: `kept=1` when each is hole-free as far as this store
/// goes (an Array of this unit that never shrinks), `kept=0` when one is
/// not (`shrunk`: how many of them may shrink or are not this unit's).
pub(crate) fn probe(g: &Gen<'_>) -> Vec<String> {
    if !g.m.inbounds_store {
        return Vec::new();
    }
    let mut buf = Vec::new();
    let mut lines: std::collections::BTreeMap<(u32, u32), (u32, u32)> = Default::default();
    for &(mi, offset, recv) in &g.in_bounds.proven {
        buf.clear();
        g.m.nodes[recv as usize].pts.sorted_into(&mut buf);
        let e = lines.entry((mi, offset)).or_default();
        for &t in &buf {
            if t == crate::model::TOP || !g.m.is_arr(t) {
                continue;
            }
            e.0 += 1;
            if !g.m.arr_grow_only(t) {
                e.1 += 1;
            }
        }
    }
    lines
        .into_iter()
        .map(|((mi, offset), (arrays, shrunk))| {
            format!(
                "# inbounds_store module={mi} site={offset} arrays={arrays} kept={} shrunk={shrunk}",
                (shrunk == 0) as u8
            )
        })
        .collect()
}
