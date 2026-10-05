//! The HIR rewrite that forms scope groups (see the module docs for the rule).
//!
//! For every function-like body it finds the eligible captured-and-mutated
//! bindings, groups them by (home statement list, TDZ seeding, capturing
//! closure set), strips each grouped id from whatever preallocation statement
//! named it, and inserts one `PreallocateBoxes`/`PreallocateTdzBoxes` per group
//! immediately before the group's earliest home. Codegen then allocates one
//! scope object per such statement.

use std::collections::{BTreeMap, HashMap, HashSet};

use perry_hir::{Expr, Module as HirModule, Param, Stmt};

use super::analysis::{self, DeclKind};

/// See the merge in `plan_body`.
const MAX_GROUPS_PER_LIST: usize = 16;

#[derive(Default)]
struct ListEdits {
    /// (insertion index, group statement), in any order.
    inserts: Vec<(usize, Stmt)>,
}

/// Group captured-and-mutated bindings into scope objects. Idempotent: a
/// second run finds every group already homed at its own statement and
/// rebuilds the same statements.
pub fn group_scope_boxes(hir: &mut HirModule) {
    let module_boxed = crate::codegen::boxed_locals::collect_module_boxed_vars(hir);
    if module_boxed.is_empty() {
        return;
    }
    let counts = analysis::prealloc_counts(hir);
    let mut edits: HashMap<usize, ListEdits> = HashMap::new();
    let mut grouped: HashSet<u32> = HashSet::new();
    let init_ptr = hir.init.as_ptr() as usize;
    super::for_each_body(hir, &mut |params: &[Param], stmts: &[Stmt]| {
        // Module-scope bindings that closures capture are globalized by
        // codegen and already have shared storage; leave the init body alone.
        if stmts.as_ptr() as usize == init_ptr {
            return;
        }
        plan_body(
            params,
            stmts,
            &module_boxed,
            &counts,
            &mut edits,
            &mut grouped,
        );
    });
    if grouped.is_empty() {
        return;
    }
    rewrite_module(hir, &mut edits, &grouped);
}

fn plan_body(
    params: &[Param],
    stmts: &[Stmt],
    module_boxed: &HashSet<u32>,
    counts: &HashMap<u32, u32>,
    edits: &mut HashMap<usize, ListEdits>,
    grouped: &mut HashSet<u32>,
) {
    let mut declared = HashSet::new();
    crate::collectors::collect_let_ids(stmts, &mut declared);
    analysis::collect_shallow_prealloc_ids(stmts, &mut declared);
    declared.retain(|id| {
        module_boxed.contains(id)
            && counts.get(id).copied().unwrap_or(0) <= 1
            && !params.iter().any(|p| p.id == *id)
    });
    if declared.is_empty() {
        return;
    }
    let facts = analysis::analyze_body(stmts, &declared);
    let capture_sets = analysis::closure_capture_sets(stmts, &declared);
    let hoisted = analysis::hoisted_decl_closures(stmts);
    // key: (home list token, tdz, capturing closures) → members (home num, id)
    let mut groups: BTreeMap<(u32, bool, Vec<u32>), Vec<(u32, u32, usize, usize)>> =
        BTreeMap::new();
    let mut ids: Vec<u32> = declared.iter().copied().collect();
    ids.sort_unstable();
    for id in ids {
        let Some(home) = facts.dominating_home(id) else {
            continue;
        };
        let Some(pos) = home.pos else {
            continue;
        };
        let tdz = home.kind == DeclKind::PreallocTdz;
        // The body's hoisted function declarations are one retention class;
        // every other closure literal (an arrow, a callback, a returned or
        // stored function expression) is its own class.
        let closures: Vec<u32> = capture_sets
            .get(&id)
            .map(|set| {
                set.iter()
                    .map(|f| {
                        if hoisted.contains(f) {
                            analysis::HOISTED_DECL_CLASS
                        } else {
                            *f
                        }
                    })
                    .collect::<std::collections::BTreeSet<u32>>()
                    .into_iter()
                    .collect()
            })
            .unwrap_or_default();
        groups
            .entry((pos.token, tdz, closures))
            .or_default()
            .push((home.num, id, pos.ptr, pos.index));
    }
    // A statement list whose bindings would split into more than
    // `MAX_GROUPS_PER_LIST` objects is a module-scale wrapper (a bundler's
    // top-level scope, an esbuild `__esm` factory): its closures live as long
    // as the module, so capture-set splitting buys no reclamation there, and
    // every extra object is another root live across the whole body. Such a
    // list gets one object per TDZ kind, V8's shape.
    let mut per_list: HashMap<u32, usize> = HashMap::new();
    for (token, _, _) in groups.keys() {
        *per_list.entry(*token).or_default() += 1;
    }
    if per_list.values().any(|n| *n > MAX_GROUPS_PER_LIST) {
        let mut merged: BTreeMap<(u32, bool, Vec<u32>), Vec<(u32, u32, usize, usize)>> =
            BTreeMap::new();
        for ((token, tdz, closures), members) in std::mem::take(&mut groups) {
            let key = if per_list[&token] > MAX_GROUPS_PER_LIST {
                (token, tdz, Vec::new())
            } else {
                (token, tdz, closures)
            };
            merged.entry(key).or_default().extend(members);
        }
        groups = merged;
    }
    for ((_, tdz, _), mut members) in groups {
        members.sort_unstable();
        for chunk in members.chunks(super::SCOPE_MAX_SLOTS) {
            let (_, _, ptr, index) = chunk[0];
            let ids: Vec<u32> = chunk.iter().map(|m| m.1).collect();
            grouped.extend(ids.iter().copied());
            let stmt = if tdz {
                Stmt::PreallocateTdzBoxes(ids)
            } else {
                Stmt::PreallocateBoxes(ids)
            };
            edits.entry(ptr).or_default().inserts.push((index, stmt));
        }
    }
}

fn rewrite_module(
    hir: &mut HirModule,
    edits: &mut HashMap<usize, ListEdits>,
    grouped: &HashSet<u32>,
) {
    let mut ctx = Rewrite { edits, grouped };
    ctx.list(&mut hir.init);
    for f in &mut hir.functions {
        ctx.function(f);
    }
    for c in &mut hir.classes {
        for m in c
            .methods
            .iter_mut()
            .chain(c.static_methods.iter_mut())
            .chain(c.getters.iter_mut().map(|(_, g)| g))
            .chain(c.setters.iter_mut().map(|(_, s)| s))
            .chain(c.computed_members.iter_mut().map(|m| &mut m.function))
            .chain(c.constructor.iter_mut())
        {
            ctx.function(m);
        }
        for field in c.fields.iter_mut().chain(c.static_fields.iter_mut()) {
            if let Some(e) = &mut field.init {
                ctx.expr(e);
            }
            if let Some(e) = &mut field.key_expr {
                ctx.expr(e);
            }
        }
        if let Some(e) = &mut c.extends_expr {
            ctx.expr(e);
        }
    }
    for g in &mut hir.globals {
        if let Some(e) = &mut g.init {
            ctx.expr(e);
        }
    }
}

struct Rewrite<'a> {
    edits: &'a mut HashMap<usize, ListEdits>,
    grouped: &'a HashSet<u32>,
}

impl Rewrite<'_> {
    fn function(&mut self, f: &mut perry_hir::Function) {
        for p in &mut f.params {
            if let Some(d) = &mut p.default {
                self.expr(d);
            }
        }
        self.list(&mut f.body);
    }

    fn list(&mut self, stmts: &mut Vec<Stmt>) {
        let ptr = stmts.as_ptr() as usize;
        let mut inserts = self
            .edits
            .remove(&ptr)
            .map(|e| e.inserts)
            .unwrap_or_default();
        let touches_prealloc = stmts.iter().any(|s| {
            matches!(s, Stmt::PreallocateBoxes(ids) | Stmt::PreallocateTdzBoxes(ids)
                if ids.iter().any(|id| self.grouped.contains(id)))
        });
        if !inserts.is_empty() || touches_prealloc {
            inserts.sort_by_key(|(index, _)| *index);
            let old = std::mem::take(stmts);
            let mut pending = inserts.into_iter().peekable();
            for (index, mut s) in old.into_iter().enumerate() {
                while pending.peek().is_some_and(|(at, _)| *at == index) {
                    stmts.push(pending.next().expect("peeked").1);
                }
                if let Stmt::PreallocateBoxes(ids) | Stmt::PreallocateTdzBoxes(ids) = &mut s {
                    let before = ids.len();
                    ids.retain(|id| !self.grouped.contains(id));
                    if ids.is_empty() && before > 0 {
                        continue;
                    }
                }
                stmts.push(s);
            }
            stmts.extend(pending.map(|(_, s)| s));
        }
        for s in stmts.iter_mut() {
            self.stmt(s);
        }
    }

    fn stmt(&mut self, s: &mut Stmt) {
        match s {
            Stmt::Let { init, .. } => {
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
                self.list(then_branch);
                if let Some(eb) = else_branch {
                    self.list(eb);
                }
            }
            Stmt::While { condition, body } | Stmt::DoWhile { body, condition } => {
                self.expr(condition);
                self.list(body);
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
                self.list(body);
            }
            Stmt::Labeled { body, .. } => self.stmt(body),
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                self.list(body);
                if let Some(c) = catch {
                    self.list(&mut c.body);
                }
                if let Some(f) = finally {
                    self.list(f);
                }
            }
            Stmt::Switch {
                discriminant,
                cases,
            } => {
                self.expr(discriminant);
                for case in cases {
                    if let Some(t) = &mut case.test {
                        self.expr(t);
                    }
                    self.list(&mut case.body);
                }
            }
            Stmt::PreallocateBoxes(_)
            | Stmt::PreallocateTdzBoxes(_)
            | Stmt::ReleaseBoxes(_)
            | Stmt::Break
            | Stmt::Continue
            | Stmt::LabeledBreak(_)
            | Stmt::LabeledContinue(_) => {}
        }
    }

    fn expr(&mut self, e: &mut Expr) {
        if let Expr::Closure { body, .. } = e {
            self.list(body);
        }
        perry_hir::walker::walk_expr_children_mut(e, &mut |child| self.expr(child));
    }
}
