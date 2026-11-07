//! Locals bound to an empty array literal that a loop entered after the
//! binding pushes into (`const moves = []; for (…) moves.push(m)`).
//!
//! An empty literal is born with four slots, so such a loop reallocates at
//! its fifth push. A reallocation (allocate, copy, forwarding stub) costs
//! about 800 instructions against about 55 for an in-capacity push. Sixteen
//! pushes into `[]` therefore spent most of their time in one regrowth. The
//! literal of a binding in this set is born with
//! [`crate::target_layout::INLINE_PUSHED_ARRAY_SLOTS`] slots instead: the
//! same empty array, with more room behind it.
//!
//! The loop must be entered after the binding: `for (…) { const m = []; m.push(x) }`
//! pushes once per array and keeps the small birth. A push from a nested
//! closure is not counted. Missing a site only loses the larger birth.
//! `PERRY_PUSHED_ARRAY_RESERVE=0` turns the larger birth off (A/B control).
//!
//! The same walk finds the **owned** array locals: bound once to an array
//! literal, and used only as a push receiver, through `.length`, through an
//! index read, or as the operand of `return`. Nothing else ever holds such an
//! array while the function runs, so nothing can freeze it, give it an own
//! property or a custom prototype, or keep an old head that a growth leaves
//! forwarded. Its push skips the receiver probes for those cases
//! (`expr::array_push`). Any other mention (an alias, a reassignment, a
//! property or index write, a capture, `for…of`) leaves it out.
//! `PERRY_OWNED_PUSH_ARRAYS=0` turns the owned admission off (A/B control).
use perry_hir::{Expr, Module, Stmt};
use std::collections::{HashMap, HashSet};

/// The two sets the walk computes.
#[derive(Default)]
pub(crate) struct PushedArrays {
    /// Empty literals a later loop pushes into: born with the larger reserve.
    pub reserve: HashSet<u32>,
    /// Array-literal locals nothing but this function's own pushes, length
    /// reads, index reads and `return` ever touch.
    pub owned: HashSet<u32>,
}

pub(crate) fn loop_pushed_arrays(hir: &Module) -> PushedArrays {
    let reserve_on = std::env::var("PERRY_PUSHED_ARRAY_RESERVE").as_deref() != Ok("0");
    let owned_on = std::env::var("PERRY_OWNED_PUSH_ARRAYS").as_deref() != Ok("0");
    let mut out = PushedArrays::default();
    // Locals some body mentions without declaring them (a closure or a
    // nested function reaching a binding of an outer body). Such a local is
    // never owned: the other body's uses were not examined where it lives.
    let mut foreign: HashSet<u32> = HashSet::new();
    crate::scope_env::for_each_body(hir, &mut |_, body| {
        let mut walk = Walk {
            declared: HashMap::new(),
            depth: 0,
            out: &mut out.reserve,
            literals: HashSet::new(),
            escaped: HashSet::new(),
            mentioned: HashSet::new(),
        };
        walk.stmts(body);
        foreign.extend(walk.mentioned.difference(&walk.literals).copied());
        if owned_on {
            out.owned
                .extend(walk.literals.difference(&walk.escaped).copied());
        }
    });
    // Expressions that run outside every body: parameter defaults, class
    // field initializers and keys, `extends` clauses, global initializers.
    // Any local they mention is not owned.
    let mut outside: Vec<&Expr> = Vec::new();
    for func in &hir.functions {
        outside.extend(func.params.iter().filter_map(|p| p.default.as_ref()));
    }
    for c in &hir.classes {
        for m in c
            .methods
            .iter()
            .chain(c.static_methods.iter())
            .chain(c.getters.iter().map(|(_, g)| g))
            .chain(c.setters.iter().map(|(_, s)| s))
            .chain(c.computed_members.iter().map(|m| &m.function))
            .chain(c.constructor.iter())
        {
            outside.extend(m.params.iter().filter_map(|p| p.default.as_ref()));
        }
        for field in c.fields.iter().chain(c.static_fields.iter()) {
            outside.extend(field.init.iter());
            outside.extend(field.key_expr.iter());
        }
        outside.extend(c.extends_expr.as_deref());
    }
    for g in &hir.globals {
        outside.extend(g.init.iter());
    }
    for e in outside {
        foreign.extend(mentions(e));
    }
    out.owned.retain(|id| !foreign.contains(id));
    if !reserve_on {
        out.reserve.clear();
    }
    out
}

/// Every local `e` mentions, closures' parameter defaults included.
fn mentions(e: &Expr) -> HashSet<u32> {
    let mut sink = HashSet::new();
    let mut walk = Walk {
        declared: HashMap::new(),
        depth: 0,
        out: &mut sink,
        literals: HashSet::new(),
        escaped: HashSet::new(),
        mentioned: HashSet::new(),
    };
    walk.expr(e);
    walk.mentioned
}

struct Walk<'o> {
    /// Loop depth at which each empty-literal binding was declared.
    declared: HashMap<u32, u32>,
    depth: u32,
    out: &'o mut HashSet<u32>,
    /// Locals bound to an array literal.
    literals: HashSet<u32>,
    /// Locals mentioned other than as an owned use, or bound twice.
    escaped: HashSet<u32>,
    /// Every local this body mentions, owned use or not.
    mentioned: HashSet<u32>,
}

impl Walk<'_> {
    fn stmts(&mut self, stmts: &[Stmt]) {
        for s in stmts {
            self.stmt(s);
        }
    }

    fn looped(&mut self, f: impl FnOnce(&mut Self)) {
        self.depth += 1;
        f(self);
        self.depth -= 1;
    }

    fn stmt(&mut self, s: &Stmt) {
        match s {
            Stmt::Let { id, init, .. } => {
                if !self.literals.insert(*id) {
                    self.escaped.insert(*id);
                }
                match init {
                    Some(init) => {
                        self.expr(init);
                        if !matches!(init, Expr::Array(..)) {
                            self.escaped.insert(*id);
                        }
                        if matches!(init, Expr::Array(elems, _) if elems.is_empty()) {
                            self.declared.insert(*id, self.depth);
                        }
                    }
                    None => {
                        self.escaped.insert(*id);
                    }
                }
            }
            Stmt::Expr(e) | Stmt::Throw(e) => self.expr(e),
            // Returning the array hands it out only as the function ends.
            Stmt::Return(Some(Expr::LocalGet(id))) => {
                self.mentioned.insert(*id);
            }
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
                if let Some(b) = else_branch {
                    self.stmts(b);
                }
            }
            Stmt::While { condition, body } | Stmt::DoWhile { condition, body } => {
                self.looped(|w| {
                    w.expr(condition);
                    w.stmts(body);
                })
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
                self.looped(|w| {
                    if let Some(c) = condition {
                        w.expr(c);
                    }
                    if let Some(u) = update {
                        w.expr(u);
                    }
                    w.stmts(body);
                })
            }
            Stmt::Labeled { body, .. } => self.stmt(body),
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                self.stmts(body);
                if let Some(c) = catch {
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
            Stmt::Break
            | Stmt::Continue
            | Stmt::LabeledBreak(_)
            | Stmt::LabeledContinue(_)
            | Stmt::PreallocateBoxes(_)
            | Stmt::PreallocateTdzBoxes(_)
            | Stmt::ReleaseBoxes(_) => {}
        }
    }

    fn expr(&mut self, e: &Expr) {
        match e {
            // A closure's pushes run when it is called, not in this loop; the
            // walker would still visit its parameter defaults. What it
            // captures it can do anything with.
            Expr::Closure {
                params,
                captures,
                mutable_captures,
                ..
            } => {
                self.escaped
                    .extend(captures.iter().chain(mutable_captures).copied());
                // A default runs when the closure is called.
                for d in params.iter().filter_map(|p| p.default.as_ref()) {
                    let m = mentions(d);
                    self.escaped.extend(m.iter().copied());
                    self.mentioned.extend(m);
                }
                return;
            }
            // The owned uses: their receiver operand is not a mention.
            Expr::PropertyGet {
                object, property, ..
            } if property == "length" && matches!(object.as_ref(), Expr::LocalGet(_)) => {
                if let Expr::LocalGet(id) = object.as_ref() {
                    self.mentioned.insert(*id);
                }
                return;
            }
            Expr::IndexGet { object, index, .. }
                if matches!(object.as_ref(), Expr::LocalGet(_)) =>
            {
                if let Expr::LocalGet(id) = object.as_ref() {
                    self.mentioned.insert(*id);
                }
                return self.expr(index);
            }
            Expr::LocalGet(id) | Expr::LocalSet(id, _) | Expr::Update { id, .. } => {
                self.escaped.insert(*id);
                self.mentioned.insert(*id);
            }
            _ => {}
        }
        if let Expr::ArrayPush { array_id, .. } | Expr::ArrayPushSpread { array_id, .. } = e {
            self.mentioned.insert(*array_id);
            if self.declared.get(array_id).is_some_and(|d| *d < self.depth) {
                self.out.insert(*array_id);
            }
        }
        perry_hir::walker::walk_expr_children(e, &mut |c| self.expr(c));
    }
}
