//! Closed World: bindings definitely assigned before any reader can run.
//!
//! A `var` declared at the top level of a body (the module's own top level, a
//! CommonJS factory body, or any other non-async, non-generator function body)
//! is hoisted: it reads `undefined` until its declarator runs. When nothing can
//! read it before that, and nothing writes it afterwards, every read in the
//! module sees the declarator's value, so the binding has the type of its
//! initializer at every read. qs's `var hexTable = (function () { var array =
//! []; ...; return array; }())` is the motivating shape: its readers live in
//! `encode`, a closure that cannot run before the table exists.
//!
//! The proof, for a binding `V` of body `B` with top-level statements `S`:
//!
//! 1. Exactly one write. `S[k]` is `V`'s only declarator with an initializer;
//!    every other declarator of `V` is a hoisting seed (`undefined` or no
//!    initializer) at the top level of `S` before `k`. `V` is no parameter
//!    and has no assignment, update or `with` fallback write anywhere in the
//!    module. `S[k]` is at the top level of `B`, so it runs unconditionally
//!    once the statements before it completed; if they throw, no later
//!    statement runs.
//! 2. No reader runs before `S[k]`. A function body (closure or function
//!    declaration) is TAINTED when calling or creating it can reach a read of
//!    `V`: it reads `V`, reads a tainted binding, creates a tainted closure or
//!    names a tainted function. A binding is tainted when some value written
//!    to it carries a tainted closure, function or binding (aliases, object
//!    literals holding the closure, ...). The code before the declarator
//!    (`S[0..k]` and the initializer itself, not descending into closure
//!    bodies) must not read `V`, name a tainted binding or function, or create
//!    a tainted closure anywhere but as the whole initializer of a top-level
//!    declarator (a hoisted function declaration or `var f = function`),
//!    whose binding is then tainted and subject to the same rule. A function
//!    can only run if something calls it, and nothing can call it before it
//!    escapes: a call, an argument, a property store (`module.exports`,
//!    getters, `valueOf`/`toString`, Proxy traps), a return value -- each of
//!    those names it. A `require` cycle re-entering this module before
//!    `S[k]` sees only what the code before `S[k]` stored in `module.exports`,
//!    which is again a name.
//! 3. Whatever cannot be modelled disqualifies: a class whose members reach a
//!    tainted name (its static parts may run anywhere), an export of `V` or
//!    of a tainted name from the module top level (an ES-module cycle can call
//!    an exported function before this module's body runs), a reflected
//!    script-global function, a read of `V` outside `B`. A direct `eval` of
//!    non-constant source needs nothing extra: lowering bridges the bindings
//!    it can name through a getter/setter object (`lower::dynamic_eval`), so
//!    a settable `V` has a write (the setter) and the getter is a reader
//!    created where the eval runs.
//!
//! The type comes from the initializer by construction: an array literal, or
//! an IIFE every return of which yields an array literal or a never-written
//! local holding one. Such a binding holds one Array for its whole life (the
//! object may grow or change elements; it stays an Array), so its readers get
//! `Array` as a proof and pick the array lowering. Element types are not
//! claimed: an out-of-bounds read is `undefined`.
//!
//! The facts reach the readers through the body's one seed set
//! (`typed_locals::outer_proofs`), together with Closed World 5d's capture
//! proofs; the two agree or meet there (see that function for the rule).
//!
//! Compile-time facts only: nothing here exists at run time.

use std::collections::{HashMap, HashSet};

use perry_hir::types::Type as HirType;
use perry_hir::{Expr, Module, Stmt};

/// `PERRY_DEFINITE_INIT=0` turns the facts off (A/B control).
pub(crate) fn enabled() -> bool {
    use std::sync::OnceLock;
    static CACHED: OnceLock<bool> = OnceLock::new();
    *CACHED.get_or_init(|| {
        !matches!(
            std::env::var("PERRY_DEFINITE_INIT").as_deref(),
            Ok("0") | Ok("off") | Ok("false")
        )
    })
}

fn trace_enabled() -> bool {
    use std::sync::OnceLock;
    static CACHED: OnceLock<bool> = OnceLock::new();
    *CACHED.get_or_init(|| std::env::var("PERRY_DEFINITE_INIT_TRACE").as_deref() == Ok("1"))
}

/// Negative control: `PERRY_DEFINITE_INIT_SABOTAGE=<check>[,<check>...]`
/// skips the named disqualifier checks (`reassigned`, `second_write`,
/// `early_read`, `tainted_name`, `tainted_closure`, `class`, `export`,
/// `outside`; `confined` admits every proven array as confined), so the gap
/// fixtures can show each one is load-bearing. Keyed
/// into the object cache like the facts switch.
fn sabotaged(check: &str) -> bool {
    use std::sync::OnceLock;
    static CACHED: OnceLock<Vec<String>> = OnceLock::new();
    CACHED
        .get_or_init(|| {
            std::env::var("PERRY_DEFINITE_INIT_SABOTAGE")
                .map(|v| v.split(',').map(str::to_string).collect())
                .unwrap_or_default()
        })
        .iter()
        .any(|c| c == check)
}

/// The proven bindings of one module.
#[derive(Debug, Default, Clone)]
pub(crate) struct DefiniteInit {
    /// Binding -> the type it has at every read.
    pub kinds: HashMap<u32, HirType>,
    /// Binding -> the bodies that hold its declarators (`None` = module top
    /// level). Those bodies lower the declarator itself and get no seed.
    pub declaring: HashMap<u32, HashSet<Option<u32>>>,
    /// Proven Array bindings whose array no code but its construction and
    /// element/`length` reads ever names (see [`confined`]).
    pub confined: HashSet<u32>,
}

impl DefiniteInit {
    fn applies(&self, id: u32, body: Option<u32>) -> bool {
        !self.declaring.get(&id).is_some_and(|b| b.contains(&body))
    }

    /// The facts a body may assume: every proven binding but those it
    /// declares (`body` = the function id being compiled, `None` = module
    /// init). They join the body's one seed set through
    /// `typed_locals::outer_proofs`, never a map of their own.
    pub(crate) fn facts_for(
        &self,
        body: Option<u32>,
    ) -> impl Iterator<Item = (u32, &HirType)> + '_ {
        self.kinds
            .iter()
            .filter(move |(id, _)| self.applies(**id, body))
            .map(|(id, ty)| (*id, ty))
    }
}

// ---------------------------------------------------------------------------
// Shallow scans: what a piece of code names when it runs, not descending into
// closure bodies (those are their own nodes).

#[derive(Default)]
struct Shallow {
    /// Locals read or written (by any LocalId-bearing form).
    locals: Vec<u32>,
    /// `FuncRef`s.
    funcs: Vec<u32>,
    /// Closure expressions created here.
    closures: Vec<u32>,
    /// Locals named as the receiver of an element or `length` read
    /// (`x[k]`, `x.length`), not under `delete`.
    receivers: Vec<u32>,
    /// Locals named as the target of `x.push(v)`.
    pushes: Vec<u32>,
    /// Locals named by an append store `x[x.length] = v` as its target (and,
    /// for an ordinary [[Set]], its receiver); the key's `x.length` is a
    /// receiver read.
    appends: Vec<u32>,
}

/// `x.length` for local `x`.
fn length_of(e: &Expr) -> Option<u32> {
    match e {
        Expr::PropertyGet {
            object, property, ..
        } if property == "length" => match object.as_ref() {
            Expr::LocalGet(id) => Some(*id),
            _ => None,
        },
        _ => None,
    }
}

fn shallow_expr(e: &Expr, s: &mut Shallow) {
    shallow_expr_at(e, s, true)
}

/// `read`: `e` is evaluated as a value (not the operand of `delete`).
fn shallow_expr_at(e: &Expr, s: &mut Shallow, read: bool) {
    if read {
        match e {
            Expr::IndexGet { object, .. } => {
                if let Expr::LocalGet(id) = object.as_ref() {
                    s.receivers.push(*id);
                }
            }
            Expr::PropertyGet {
                object, property, ..
            } if property == "length" => {
                if let Expr::LocalGet(id) = object.as_ref() {
                    s.receivers.push(*id);
                }
            }
            _ => {}
        }
    }
    match e {
        Expr::Delete(inner) => {
            shallow_expr_at(inner, s, false);
            return;
        }
        Expr::IndexSet { object, index, .. } => {
            if let (Expr::LocalGet(id), Some(len)) = (object.as_ref(), length_of(index)) {
                if *id == len {
                    s.appends.push(*id);
                }
            }
        }
        Expr::PutValueSet {
            target,
            key,
            receiver,
            ..
        } => {
            if let (Expr::LocalGet(t), Some(len), Expr::LocalGet(r)) =
                (target.as_ref(), length_of(key), receiver.as_ref())
            {
                if *t == len && *r == len {
                    s.appends.push(*t);
                    s.appends.push(*r);
                }
            }
        }
        Expr::ArrayPush { array_id, .. } => {
            s.pushes.push(*array_id);
            s.locals.push(*array_id);
        }
        Expr::LocalGet(id) | Expr::LocalSet(id, _) | Expr::Update { id, .. } => {
            s.locals.push(*id);
        }
        Expr::ArrayPushSpread { array_id, .. }
        | Expr::ArrayUnshift { array_id, .. }
        | Expr::ArraySplice { array_id, .. }
        | Expr::ArrayCopyWithin { array_id, .. } => s.locals.push(*array_id),
        Expr::ArrayPop(id) | Expr::ArrayShift(id) => s.locals.push(*id),
        Expr::SetAdd { set_id, .. } => s.locals.push(*set_id),
        Expr::FuncRef(f) => s.funcs.push(*f),
        Expr::Closure { func_id, .. } => {
            // The closure's parameter defaults and body belong to its node.
            s.closures.push(*func_id);
            return;
        }
        _ => {}
    }
    perry_hir::walker::walk_expr_children(e, &mut |c| shallow_expr_at(c, s, true));
}

/// Visit every expression of `stmt` (and its nested statements) at statement
/// level, in source order.
fn for_each_stmt_expr<'a>(stmt: &'a Stmt, f: &mut impl FnMut(&'a Expr)) {
    match stmt {
        Stmt::Let { init, .. } | Stmt::Return(init) => {
            if let Some(e) = init {
                f(e);
            }
        }
        Stmt::Expr(e) | Stmt::Throw(e) => f(e),
        Stmt::If {
            condition,
            then_branch,
            else_branch,
        } => {
            f(condition);
            then_branch.iter().for_each(|s| for_each_stmt_expr(s, f));
            if let Some(b) = else_branch {
                b.iter().for_each(|s| for_each_stmt_expr(s, f));
            }
        }
        Stmt::While { condition, body } | Stmt::DoWhile { condition, body } => {
            f(condition);
            body.iter().for_each(|s| for_each_stmt_expr(s, f));
        }
        Stmt::For {
            init,
            condition,
            update,
            body,
        } => {
            if let Some(s) = init {
                for_each_stmt_expr(s, f);
            }
            if let Some(c) = condition {
                f(c);
            }
            if let Some(u) = update {
                f(u);
            }
            body.iter().for_each(|s| for_each_stmt_expr(s, f));
        }
        Stmt::Labeled { body, .. } => for_each_stmt_expr(body, f),
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            body.iter().for_each(|s| for_each_stmt_expr(s, f));
            if let Some(c) = catch {
                c.body.iter().for_each(|s| for_each_stmt_expr(s, f));
            }
            if let Some(b) = finally {
                b.iter().for_each(|s| for_each_stmt_expr(s, f));
            }
        }
        Stmt::Switch {
            discriminant,
            cases,
        } => {
            f(discriminant);
            for c in cases {
                if let Some(t) = &c.test {
                    f(t);
                }
                c.body.iter().for_each(|s| for_each_stmt_expr(s, f));
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

/// Every statement of `stmt`, nested ones included (not closure bodies).
fn for_each_stmt<'a>(stmt: &'a Stmt, f: &mut impl FnMut(&'a Stmt)) {
    f(stmt);
    match stmt {
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            then_branch.iter().for_each(|s| for_each_stmt(s, f));
            if let Some(b) = else_branch {
                b.iter().for_each(|s| for_each_stmt(s, f));
            }
        }
        Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => {
            body.iter().for_each(|s| for_each_stmt(s, f))
        }
        Stmt::For { init, body, .. } => {
            if let Some(s) = init {
                for_each_stmt(s, f);
            }
            body.iter().for_each(|s| for_each_stmt(s, f));
        }
        Stmt::Labeled { body, .. } => for_each_stmt(body, f),
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            body.iter().for_each(|s| for_each_stmt(s, f));
            if let Some(c) = catch {
                c.body.iter().for_each(|s| for_each_stmt(s, f));
            }
            if let Some(b) = finally {
                b.iter().for_each(|s| for_each_stmt(s, f));
            }
        }
        Stmt::Switch { cases, .. } => {
            for c in cases {
                c.body.iter().for_each(|s| for_each_stmt(s, f));
            }
        }
        _ => {}
    }
}

fn shallow_stmts<'a>(stmts: impl IntoIterator<Item = &'a Stmt>, s: &mut Shallow) {
    for st in stmts {
        for_each_stmt_expr(st, &mut |e| shallow_expr(e, s));
    }
}

fn is_seed(init: &Option<Expr>) -> bool {
    matches!(init, None | Some(Expr::Undefined))
}

// ---------------------------------------------------------------------------
// The module model.

/// A function-like body: a closure or a `hir.functions` entry.
struct Node<'a> {
    id: u32,
    /// The body that creates this closure (`None` = module top level or a
    /// `hir.functions` entry, which has no lexical parent node here).
    parent: Option<u32>,
    /// Logical statements (entry chunks inlined).
    stmts: Vec<&'a Stmt>,
    /// Non-async, non-generator: a body whose top level runs straight through.
    plain: bool,
    scan: Shallow,
}

struct Model<'a> {
    hir: &'a Module,
    /// Module top level, logical (entry chunks inlined).
    init: Vec<&'a Stmt>,
    init_scan: Shallow,
    nodes: HashMap<u32, Node<'a>>,
    /// Entry-chunk function id -> the body whose logical statements hold it.
    chunk_owner: HashMap<u32, Option<u32>>,
    /// All locals and functions named by class members (any depth: nested
    /// closures are reached through `class_closures`).
    class_scan: Shallow,
    /// Every `Let` per binding, module-wide.
    let_counts: HashMap<u32, usize>,
    params: HashSet<u32>,
    writes: HashSet<u32>,
    /// Binding -> what its written values carry.
    sources: HashMap<u32, Shallow>,
}

fn record_lets_and_sources<'a>(
    stmts: &[&'a Stmt],
    m_lets: &mut HashMap<u32, usize>,
    sources: &mut HashMap<u32, Shallow>,
) {
    for st in stmts {
        for_each_stmt(st, &mut |s| {
            if let Stmt::Let { id, init, .. } = s {
                *m_lets.entry(*id).or_insert(0) += 1;
                if let Some(e) = init {
                    shallow_expr(e, sources.entry(*id).or_default());
                }
            }
        });
        for_each_stmt_expr(st, &mut |e| record_set_sources(e, sources));
    }
}

/// `LocalSet(b, v)` anywhere in `e` (not in closures): `b` carries `v`.
fn record_set_sources(e: &Expr, sources: &mut HashMap<u32, Shallow>) {
    if let Expr::Closure { .. } = e {
        return;
    }
    if let Expr::LocalSet(id, value) = e {
        shallow_expr(value, sources.entry(*id).or_default());
    }
    perry_hir::walker::walk_expr_children(e, &mut |c| record_set_sources(c, sources));
}

fn collect_params(params: &[perry_hir::Param], out: &mut HashSet<u32>) {
    for p in params {
        out.insert(p.id);
    }
}

/// Find closure expressions in `stmts` (not descending into them).
fn closures_in<'a>(stmts: &[&'a Stmt], out: &mut Vec<&'a Expr>) {
    fn in_expr<'a>(e: &'a Expr, out: &mut Vec<&'a Expr>) {
        if let Expr::Closure { .. } = e {
            out.push(e);
            return;
        }
        perry_hir::walker::walk_expr_children(e, &mut |c| in_expr(c, out));
    }
    for st in stmts {
        for_each_stmt_expr(st, &mut |e| in_expr(e, out));
    }
}

/// Closure expressions inside a closure's parameter defaults.
fn closures_in_param_defaults<'a>(params: &'a [perry_hir::Param], out: &mut Vec<&'a Expr>) {
    fn in_expr<'a>(e: &'a Expr, out: &mut Vec<&'a Expr>) {
        if let Expr::Closure { .. } = e {
            out.push(e);
            return;
        }
        perry_hir::walker::walk_expr_children(e, &mut |c| in_expr(c, out));
    }
    for p in params {
        if let Some(d) = &p.default {
            in_expr(d, out);
        }
    }
}

impl<'a> Model<'a> {
    fn build(hir: &'a Module) -> Self {
        let chunks = crate::codegen::entry_outline::chunk_map(hir);
        let logical = |stmts: &'a [Stmt]| -> Vec<&'a Stmt> {
            crate::codegen::entry_outline::logical_stmts_of(stmts, &chunks)
        };
        let mut chunk_owner: HashMap<u32, Option<u32>> = HashMap::new();
        let note_chunks =
            |stmts: &'a [Stmt], owner: Option<u32>, out: &mut HashMap<u32, Option<u32>>| {
                for st in stmts {
                    if let Stmt::Expr(Expr::Call { callee, args, .. }) = st {
                        if let Expr::FuncRef(id) = callee.as_ref() {
                            if args.is_empty() && chunks.contains_key(id) {
                                out.insert(*id, owner);
                            }
                        }
                    }
                }
            };
        let mut model = Model {
            hir,
            init: logical(&hir.init),
            init_scan: Shallow::default(),
            nodes: HashMap::new(),
            chunk_owner: HashMap::new(),
            class_scan: Shallow::default(),
            let_counts: HashMap::new(),
            params: HashSet::new(),
            writes: crate::collectors::reassigned_locals_in_module(hir),
            sources: HashMap::new(),
        };
        note_chunks(&hir.init, None, &mut chunk_owner);
        shallow_stmts(model.init.iter().copied(), &mut model.init_scan);
        record_lets_and_sources(&model.init, &mut model.let_counts, &mut model.sources);

        // Work list of closure expressions with their parent body.
        let mut pending: Vec<(&'a Expr, Option<u32>)> = Vec::new();
        {
            let mut found = Vec::new();
            closures_in(&model.init, &mut found);
            pending.extend(found.into_iter().map(|e| (e, None)));
        }
        for f in &hir.functions {
            collect_params(&f.params, &mut model.params);
            if chunks.contains_key(&f.id) {
                continue; // part of the body that calls it
            }
            note_chunks(&f.body, Some(f.id), &mut chunk_owner);
            let stmts = logical(&f.body);
            let mut scan = Shallow::default();
            for p in &f.params {
                if let Some(d) = &p.default {
                    shallow_expr(d, &mut scan);
                }
            }
            shallow_stmts(stmts.iter().copied(), &mut scan);
            record_lets_and_sources(&stmts, &mut model.let_counts, &mut model.sources);
            let mut found = Vec::new();
            closures_in(&stmts, &mut found);
            closures_in_param_defaults(&f.params, &mut found);
            pending.extend(found.into_iter().map(|e| (e, Some(f.id))));
            model.nodes.insert(
                f.id,
                Node {
                    id: f.id,
                    parent: None,
                    stmts,
                    plain: !f.is_async && !f.is_generator,
                    scan,
                },
            );
        }
        // Classes: one collective scan.
        let mut class_bodies: Vec<&'a perry_hir::Function> = Vec::new();
        let mut class_exprs: Vec<&'a Expr> = Vec::new();
        for c in &hir.classes {
            if let Some(e) = &c.extends_expr {
                class_exprs.push(e);
            }
            for d in &c.decorators {
                class_exprs.extend(d.args.iter());
            }
            if let Some(ctor) = &c.constructor {
                class_bodies.push(ctor);
            }
            class_bodies.extend(c.methods.iter());
            class_bodies.extend(c.static_methods.iter());
            class_bodies.extend(c.getters.iter().map(|(_, g)| g));
            class_bodies.extend(c.setters.iter().map(|(_, s)| s));
            for cm in &c.computed_members {
                class_exprs.push(&cm.key_expr);
                class_bodies.push(&cm.function);
            }
            for field in c.fields.iter().chain(c.static_fields.iter()) {
                if let Some(k) = &field.key_expr {
                    class_exprs.push(k);
                }
                if let Some(i) = &field.init {
                    class_exprs.push(i);
                }
                for d in &field.decorators {
                    class_exprs.extend(d.args.iter());
                }
            }
        }
        let mut class_scan = Shallow::default();
        for e in &class_exprs {
            shallow_expr(e, &mut class_scan);
            let mut found = Vec::new();
            fn in_expr<'b>(e: &'b Expr, out: &mut Vec<&'b Expr>) {
                if let Expr::Closure { .. } = e {
                    out.push(e);
                    return;
                }
                perry_hir::walker::walk_expr_children(e, &mut |c| in_expr(c, out));
            }
            in_expr(e, &mut found);
            pending.extend(found.into_iter().map(|e| (e, None)));
            record_set_sources(e, &mut model.sources);
        }
        for f in &class_bodies {
            collect_params(&f.params, &mut model.params);
            for p in &f.params {
                if let Some(d) = &p.default {
                    shallow_expr(d, &mut class_scan);
                }
            }
            let stmts: Vec<&'a Stmt> = f.body.iter().collect();
            shallow_stmts(stmts.iter().copied(), &mut class_scan);
            record_lets_and_sources(&stmts, &mut model.let_counts, &mut model.sources);
            let mut found = Vec::new();
            closures_in(&stmts, &mut found);
            closures_in_param_defaults(&f.params, &mut found);
            pending.extend(found.into_iter().map(|e| (e, None)));
        }
        // Closures, transitively. A closure created directly by a class
        // member is named by `class_scan.closures`.
        while let Some((e, parent)) = pending.pop() {
            let Expr::Closure {
                func_id,
                params,
                body,
                is_async,
                is_generator,
                ..
            } = e
            else {
                continue;
            };
            if model.nodes.contains_key(func_id) {
                continue;
            }
            collect_params(params, &mut model.params);
            note_chunks(body, Some(*func_id), &mut chunk_owner);
            let stmts = logical(body);
            let mut scan = Shallow::default();
            for p in params {
                if let Some(d) = &p.default {
                    shallow_expr(d, &mut scan);
                }
            }
            shallow_stmts(stmts.iter().copied(), &mut scan);
            record_lets_and_sources(&stmts, &mut model.let_counts, &mut model.sources);
            let mut found = Vec::new();
            closures_in(&stmts, &mut found);
            closures_in_param_defaults(params, &mut found);
            pending.extend(found.into_iter().map(|c| (c, Some(*func_id))));
            model.nodes.insert(
                *func_id,
                Node {
                    id: *func_id,
                    parent,
                    stmts,
                    plain: !*is_async && !*is_generator,
                    scan,
                },
            );
        }
        model.class_scan = class_scan;
        model.chunk_owner = chunk_owner;
        model
    }
}

// ---------------------------------------------------------------------------
// Taint.

struct Reverse {
    /// local -> nodes naming it
    node_by_local: HashMap<u32, Vec<u32>>,
    /// function -> nodes creating or naming it
    node_by_func: HashMap<u32, Vec<u32>>,
    /// local -> bindings whose values carry it
    binding_by_local: HashMap<u32, Vec<u32>>,
    /// function -> bindings whose values carry it
    binding_by_func: HashMap<u32, Vec<u32>>,
}

impl Reverse {
    fn build(m: &Model<'_>) -> Self {
        let mut r = Reverse {
            node_by_local: HashMap::new(),
            node_by_func: HashMap::new(),
            binding_by_local: HashMap::new(),
            binding_by_func: HashMap::new(),
        };
        for n in m.nodes.values() {
            for l in &n.scan.locals {
                r.node_by_local.entry(*l).or_default().push(n.id);
            }
            for f in n.scan.funcs.iter().chain(n.scan.closures.iter()) {
                r.node_by_func.entry(*f).or_default().push(n.id);
            }
        }
        for (b, s) in &m.sources {
            for l in &s.locals {
                r.binding_by_local.entry(*l).or_default().push(*b);
            }
            for f in s.funcs.iter().chain(s.closures.iter()) {
                r.binding_by_func.entry(*f).or_default().push(*b);
            }
        }
        r
    }
}

#[derive(Default)]
struct Taint {
    funcs: HashSet<u32>,
    bindings: HashSet<u32>,
}

/// `body` (the declaring body) is never tainted: its own code is split at
/// the declarator and checked directly, and calling it again runs a new
/// activation that reaches its own declarator before any of its own reads.
fn taint_of(v: u32, body: Option<u32>, r: &Reverse) -> Taint {
    let mut t = Taint::default();
    t.bindings.insert(v);
    let mut work_b = vec![v];
    let mut work_f: Vec<u32> = Vec::new();
    loop {
        if let Some(b) = work_b.pop() {
            for n in r.node_by_local.get(&b).into_iter().flatten() {
                if Some(*n) != body && t.funcs.insert(*n) {
                    work_f.push(*n);
                }
            }
            for b2 in r.binding_by_local.get(&b).into_iter().flatten() {
                if t.bindings.insert(*b2) {
                    work_b.push(*b2);
                }
            }
            continue;
        }
        if let Some(f) = work_f.pop() {
            for n in r.node_by_func.get(&f).into_iter().flatten() {
                if Some(*n) != body && t.funcs.insert(*n) {
                    work_f.push(*n);
                }
            }
            for b2 in r.binding_by_func.get(&f).into_iter().flatten() {
                if t.bindings.insert(*b2) {
                    work_b.push(*b2);
                }
            }
            continue;
        }
        break;
    }
    t
}

// ---------------------------------------------------------------------------
// Initializer kinds.

fn is_array_literal(e: &Expr) -> bool {
    matches!(e, Expr::Array(_) | Expr::ArraySpread(_))
}

/// The single top-level declarator of `id` in `stmts` with an initializer,
/// with every other declarator a seed before it. Returns its index.
fn sole_declarator(stmts: &[&Stmt], id: u32) -> Option<usize> {
    let mut found: Option<usize> = None;
    for (i, st) in stmts.iter().enumerate() {
        if let Stmt::Let { id: lid, init, .. } = st {
            if *lid != id {
                continue;
            }
            if is_seed(init) {
                if found.is_some() && !sabotaged("second_write") {
                    return None; // a seed after the write re-declares it
                }
            } else if found.is_some() {
                if !sabotaged("second_write") {
                    return None;
                }
            } else {
                found = Some(i);
            }
        }
    }
    found
}

fn top_level_let_count(stmts: &[&Stmt], id: u32) -> usize {
    stmts
        .iter()
        .filter(|s| matches!(s, Stmt::Let { id: lid, .. } if *lid == id))
        .count()
}

/// An IIFE whose every return yields an array: `(function () { var a = [];
/// ...; return a; }())`.
fn iife_returns_array(m: &Model<'_>, body: &[Stmt]) -> bool {
    let stmts: Vec<&Stmt> = body.iter().collect();
    let Some(Stmt::Return(Some(_))) = stmts.last() else {
        return false;
    };
    for (i, st) in stmts.iter().enumerate() {
        let mut ok = true;
        for_each_stmt(st, &mut |s| {
            if let Stmt::Return(r) = s {
                ok &= match r {
                    Some(e) if is_array_literal(e) => true,
                    Some(Expr::LocalGet(a)) => array_local_before(m, &stmts, *a, i),
                    _ => false,
                };
            }
        });
        if !ok {
            return false;
        }
    }
    true
}

/// `a` is declared in `stmts` by one array-literal declarator before index
/// `at`, and never written or declared elsewhere.
fn array_local_before(m: &Model<'_>, stmts: &[&Stmt], a: u32, at: usize) -> bool {
    let Some(k) = sole_declarator(stmts, a) else {
        return false;
    };
    let Stmt::Let {
        init: Some(init), ..
    } = stmts[k]
    else {
        return false;
    };
    k < at
        && is_array_literal(init)
        && !m.writes.contains(&a)
        && !m.params.contains(&a)
        && m.let_counts.get(&a).copied() == Some(top_level_let_count(stmts, a))
}

fn initializer_kind(m: &Model<'_>, init: &Expr) -> Option<HirType> {
    let array = HirType::Array(Box::new(HirType::Any));
    if is_array_literal(init) {
        return Some(array);
    }
    if let Expr::Call { callee, args, .. } = init {
        if let Expr::Closure {
            params,
            body,
            is_async: false,
            is_generator: false,
            ..
        } = callee.as_ref()
        {
            if args.is_empty() && params.is_empty() && iife_returns_array(m, body) {
                return Some(array);
            }
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Confinement.

fn count(v: &[u32], id: u32) -> usize {
    v.iter().filter(|x| **x == id).count()
}

/// Every occurrence of `id` in the module: as a named local, and as the
/// receiver of an element or `length` read.
fn module_uses(m: &Model<'_>, id: u32) -> (usize, usize) {
    let mut total = count(&m.init_scan.locals, id) + count(&m.class_scan.locals, id);
    let mut receiver = count(&m.init_scan.receivers, id) + count(&m.class_scan.receivers, id);
    for n in m.nodes.values() {
        total += count(&n.scan.locals, id);
        receiver += count(&n.scan.receivers, id);
    }
    (total, receiver)
}

/// A proven Array binding is CONFINED when no code can hold its array but the
/// construction and element/`length` reads of the binding:
///
/// * every occurrence of the binding in the module is the receiver of an
///   element or `length` read (no call argument, store, alias, method call,
///   `delete`, spread or write);
/// * the array is an array literal, or the never-written local of the
///   initializing IIFE that the IIFE only appends to (`push`, or a store at
///   `length`; the values not naming it), reads and returns, and that no
///   nested closure names. Appends keep it dense.
///
/// No user code then receives the array as a value, so no user code can
/// define an element descriptor or a named property on it, or retarget its
/// prototype -- except through an accessor or patched method on the default
/// prototype chain that a read or `push` runs with the array as `this`. Any
/// such change to `%Array.prototype%`/`%Object.prototype%` sets
/// `PERRY_ARRAY_INDEX_FAST_PATH_INVALIDATED` for good, so the element read
/// tests that byte in place of the receiver's descriptor layout.
fn confined(m: &Model<'_>, v: u32, init: &Expr) -> bool {
    let (total, receiver) = module_uses(m, v);
    if total != receiver {
        return false;
    }
    if matches!(init, Expr::Array(_)) {
        return true;
    }
    let Expr::Call { callee, .. } = init else {
        return false;
    };
    let Expr::Closure { func_id, body, .. } = callee.as_ref() else {
        return false;
    };
    let Some(Stmt::Return(Some(Expr::LocalGet(a)))) = body.last() else {
        return false;
    };
    let Some(node) = m.nodes.get(func_id) else {
        return false;
    };
    let stmts: Vec<&Stmt> = body.iter().collect();
    let Some(k) = sole_declarator(&stmts, *a) else {
        return false;
    };
    if !matches!(
        stmts[k],
        Stmt::Let {
            init: Some(Expr::Array(_)),
            ..
        }
    ) {
        return false;
    }
    let mut returns = 0usize;
    let mut other_returns = false;
    for st in &stmts {
        for_each_stmt(st, &mut |s| {
            if let Stmt::Return(r) = s {
                match r {
                    Some(Expr::LocalGet(x)) if x == a => returns += 1,
                    _ => other_returns = true,
                }
            }
        });
    }
    let named_by_nested = m
        .nodes
        .values()
        .any(|n| n.parent == Some(*func_id) && n.scan.locals.contains(a));
    !other_returns
        && !named_by_nested
        && count(&node.scan.locals, *a)
            == count(&node.scan.pushes, *a)
                + count(&node.scan.appends, *a)
                + count(&node.scan.receivers, *a)
                + returns
}

// ---------------------------------------------------------------------------
// The proof.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Verdict {
    Proven,
    EarlyRead,
    TaintedName,
    TaintedClosure,
    Class,
    Exported,
    OutsideBody,
}

/// Can anything read `v` before `stmts[k]` runs?
fn order_verdict(
    m: &Model<'_>,
    r: &Reverse,
    v: u32,
    name: &str,
    body: Option<u32>,
    stmts: &[&Stmt],
    k: usize,
) -> Verdict {
    let t = taint_of(v, body, r);
    // A class member reaching V: its static parts can run anywhere.
    if !sabotaged("class")
        && (m.class_scan.locals.iter().any(|l| t.bindings.contains(l))
            || m.class_scan
                .funcs
                .iter()
                .chain(m.class_scan.closures.iter())
                .any(|f| t.funcs.contains(f)))
    {
        return Verdict::Class;
    }
    // Reads of V outside B's subtree (a body that is not B's descendant).
    if body.is_some() && !sabotaged("outside") {
        let in_b = |mut n: u32| -> bool {
            loop {
                if Some(n) == body {
                    return true;
                }
                if let Some(owner) = m.chunk_owner.get(&n) {
                    if *owner == body {
                        return true;
                    }
                }
                match m.nodes.get(&n).and_then(|node| node.parent) {
                    Some(p) => n = p,
                    None => return false,
                }
            }
        };
        if m.init_scan.locals.contains(&v)
            || r.node_by_local
                .get(&v)
                .into_iter()
                .flatten()
                .any(|n| !in_b(*n))
        {
            return Verdict::OutsideBody;
        }
    }
    // Exports and reflected script globals reach names before the body runs.
    if body.is_none() && !sabotaged("export") {
        let exported_locals: HashSet<&str> = m
            .hir
            .exports
            .iter()
            .filter_map(|e| match e {
                perry_hir::Export::Named { local, .. } => Some(local.as_str()),
                _ => None,
            })
            .collect();
        // A script-level `var` is also a property of the global object.
        if exported_locals.contains(name)
            || m.hir.exported_objects.iter().any(|o| o == name)
            || m.hir
                .annexb_global_undefined_names
                .iter()
                .any(|o| o == name)
        {
            return Verdict::Exported;
        }
        for f in &m.hir.functions {
            if t.funcs.contains(&f.id)
                && (f.is_exported || exported_locals.contains(f.name.as_str()))
            {
                return Verdict::Exported;
            }
        }
        if m.hir
            .script_global_functions
            .iter()
            .any(|(_, id)| t.funcs.contains(id))
        {
            return Verdict::Exported;
        }
        // Exported tainted bindings, by name.
        let tainted_names: HashSet<&str> = stmts
            .iter()
            .filter_map(|s| match s {
                Stmt::Let { id, name, .. } if t.bindings.contains(id) => Some(name.as_str()),
                _ => None,
            })
            .collect();
        if exported_locals.iter().any(|n| tainted_names.contains(n)) {
            return Verdict::Exported;
        }
    }
    // The code before the declarator.
    let mut pre = Shallow::default();
    let mut allowed_closures: HashSet<u32> = HashSet::new();
    for st in &stmts[..k] {
        if let Stmt::Let {
            init: Some(Expr::Closure { func_id, .. }),
            ..
        } = st
        {
            allowed_closures.insert(*func_id);
        }
    }
    shallow_stmts(stmts[..k].iter().copied(), &mut pre);
    if let Stmt::Let { init: Some(e), .. } = stmts[k] {
        shallow_expr(e, &mut pre);
    }
    if !sabotaged("early_read") && pre.locals.contains(&v) {
        return Verdict::EarlyRead;
    }
    if !sabotaged("tainted_name")
        && (pre.locals.iter().any(|l| *l != v && t.bindings.contains(l))
            || pre.funcs.iter().any(|f| t.funcs.contains(f)))
    {
        return Verdict::TaintedName;
    }
    if !sabotaged("tainted_closure")
        && pre
            .closures
            .iter()
            .any(|c| t.funcs.contains(c) && !allowed_closures.contains(c))
    {
        return Verdict::TaintedClosure;
    }
    Verdict::Proven
}

/// Is node `b` the CommonJS wrapper's `__perry_cjs_factory` closure?
fn is_cjs_factory(m: &Model<'_>, b: u32) -> bool {
    let parent_stmts: &[&Stmt] = match m.nodes.get(&b).and_then(|n| n.parent) {
        Some(p) => m.nodes.get(&p).map_or(&[][..], |n| &n.stmts[..]),
        None => &m.init[..],
    };
    parent_stmts.iter().any(|s| {
        matches!(s, Stmt::Let { name, init: Some(Expr::Closure { func_id, .. }), .. }
            if *func_id == b && name == "__perry_cjs_factory")
    })
}

/// Compute the facts of one module.
pub(crate) fn collect(hir: &Module) -> DefiniteInit {
    let mut out = DefiniteInit::default();
    if !enabled() {
        return out;
    }
    let trace = trace_enabled();
    let m = Model::build(hir);
    let mut reverse: Option<Reverse> = None;
    // Bodies: the module top level and every plain node.
    let mut bodies: Vec<(Option<u32>, &[&Stmt])> = vec![(None, &m.init[..])];
    let mut node_ids: Vec<u32> = m.nodes.keys().copied().collect();
    node_ids.sort_unstable();
    for id in &node_ids {
        let n = &m.nodes[id];
        if n.plain {
            bodies.push((Some(n.id), &n.stmts[..]));
        }
    }
    for (body, stmts) in bodies {
        for (k, st) in stmts.iter().enumerate() {
            let Stmt::Let {
                id,
                name,
                init: Some(init),
                ..
            } = st
            else {
                continue;
            };
            if matches!(init, Expr::Undefined) {
                continue;
            }
            let v = *id;
            if m.params.contains(&v)
                || (m.writes.contains(&v) && !sabotaged("reassigned"))
                || sole_declarator(stmts, v) != Some(k)
                || (m.let_counts.get(&v).copied() != Some(top_level_let_count(stmts, v))
                    && !sabotaged("second_write"))
            {
                continue;
            }
            let kind = initializer_kind(&m, init);
            // Function bindings are not data; count only data bindings.
            let is_fn = matches!(init, Expr::Closure { .. });
            if kind.is_none() && !(trace && !is_fn) {
                continue;
            }
            let r = reverse.get_or_insert_with(|| Reverse::build(&m));
            let verdict = order_verdict(&m, r, v, name, body, stmts, k);
            let is_confined = verdict == Verdict::Proven
                && kind.is_some()
                && (sabotaged("confined") || confined(&m, v, init));
            if trace {
                // `module` = the module's own top level or its CommonJS
                // factory body; `fn` = any other function body.
                let module_level = body.is_none_or(|b| is_cjs_factory(&m, b));
                eprintln!(
                    "[definite-init] module={} scope={} body={} var={} id={} kind={} verdict={:?} confined={} uses={:?}",
                    hir.name,
                    if module_level { "module" } else { "fn" },
                    body.map_or("init".to_string(), |b| b.to_string()),
                    name,
                    v,
                    kind.as_ref().map_or("-".to_string(), |k| format!("{k:?}")),
                    verdict,
                    is_confined,
                    module_uses(&m, v)
                );
            }
            if verdict == Verdict::Proven {
                if let Some(kind) = kind {
                    out.kinds.insert(v, kind);
                    let mut declaring: HashSet<Option<u32>> = HashSet::new();
                    declaring.insert(body);
                    for (chunk, owner) in &m.chunk_owner {
                        if *owner == body {
                            declaring.insert(Some(*chunk));
                        }
                    }
                    out.declaring.insert(v, declaring);
                    if is_confined {
                        out.confined.insert(v);
                    }
                }
            }
        }
    }
    out
}
