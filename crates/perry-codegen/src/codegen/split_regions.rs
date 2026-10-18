//! Structural function splitting with typed regions (object-model design
//! note of 2026-10-16).
//!
//! A body above [`min_body_nodes`] is cut at single-entry/single-exit
//! statement ranges into separate top-level functions ("regions"). The
//! choice is structural only: a region is a run of statements of one
//! statement list (the body, an `if` arm, a `try` block, a `switch` case, a
//! labeled block), never a part of a loop body (a loop is wrapped whole),
//! and never chosen by profile or hotness.
//!
//! Each region has exactly one call site, its parent, so it is compiled as
//! one body. Its parameters are its live-ins, passed by value; its live-outs
//! come back as the return value (one value directly, several as an array the
//! parent unpacks). A live-in carries the type the parent proves for it at
//! the call: the body's typed locals (Closed World 5d `typed_locals`, a
//! String/Number/Array at every read of the local) or an enclosing `typeof`
//! guard when the region is the guarded arm itself. Those types are the
//! region's parameter proofs (`region_param_proofs`), so its body is a typed
//! body with no generic copy. Number parameters are normalized at entry (an
//! int32 box becomes the equal double), so a Number proof always means a
//! plain double.
//!
//! What may cross the boundary by value, decided per region:
//!
//! * a local no closure captures (the parent's own slot): any read or write.
//!   A write makes it a live-out.
//! * a captured local (or a capture of the parent closure) that nothing ever
//!   writes after its declaration, when its declaration dominates the call
//!   (a parameter, a capture, or a `let` earlier in an enclosing list).
//! * a captured local that something writes is shared mutable state: a
//!   statement that touches one stays in the parent (with
//!   `PERRY_SPLIT_CLOSURES=1`, the range instead becomes a closure region
//!   that captures the parent's cell, exactly as the parent's own closures
//!   do).
//!
//! A statement stays in the parent when it can leave the range other than by
//! falling through (`return`, a `break`/`continue` to a target outside it),
//! uses the parent's frame (`this`, `super`, `new.target`, `arguments`,
//! `await`, `yield`, a `with` scope, an arrow that captures `this`), or
//! touches class-capture or class-environment machinery whose operands are
//! not ordinary expressions.
//!
//! No caches and no side tables: a region is an ordinary HIR function whose
//! name carries [`REGION_MARK`] (a `#` cannot occur in a source function
//! name), and its parameter types are its declared types. Heap live-ins and
//! live-outs are ordinary rooted parameters and return values.

use std::collections::{BTreeSet, HashMap, HashSet};

use perry_hir::types::{LocalId, Type};
use perry_hir::{CompareOp, Expr, Function, LogicalOp, Module as HirModule, Param, Stmt, UnaryOp};

/// Marks a region function's name: `{parent}#region_{n}`. `sanitize` turns
/// the `#` into `_`, so the symbol is `..._{parent}_region_{n}`.
pub(crate) const REGION_MARK: &str = "#region_";

/// `PERRY_SPLIT=0` turns splitting off (keyed into the object cache through
/// the HIR it changes, and listed in compile_env).
fn enabled() -> bool {
    !matches!(
        std::env::var("PERRY_SPLIT").as_deref(),
        Ok("0") | Ok("off") | Ok("false")
    )
}

fn env_usize(name: &str, default: usize) -> usize {
    std::env::var(name)
        .ok()
        .and_then(|v| v.trim().parse::<usize>().ok())
        .filter(|&n| n > 0)
        .unwrap_or(default)
}

/// A body is split when it has more HIR nodes than this (statements and
/// expressions, nested closures excluded). The census (secret-tests
/// fnsize/README.md) puts a node at 10-80 IR instructions (idiomatic_1k
/// `big`: 7.6k nodes, 73k IR) and the superlinear LLVM regime above ~100k IR
/// for one function (66k IR: 3 s; 214k: 12 s; 500k: 421 s). 4000 nodes is
/// about 40k IR at idiomatic density: every function that can reach that
/// regime is split, ordinary large functions are not.
fn min_body_nodes() -> usize {
    env_usize("PERRY_SPLIT_MIN_NODES", 4000)
}

/// Target size of one region, in nodes (~15k IR at idiomatic density, inside
/// the linear LLVM regime with room for the denser generated code).
fn target_nodes() -> usize {
    env_usize("PERRY_SPLIT_TARGET_NODES", 1500)
}

/// At most this many values cross a region boundary by value in each
/// direction: a cut is made where few values are live (the owner rule), and a
/// range whose live-in or live-out set is larger stays in the parent (or, for
/// live-ins of a closure region, is captured instead).
const MAX_PARAMS: usize = 24;
const MAX_OUTS: usize = 8;
/// A run region holds at least this many nodes per value crossing its
/// boundary (idiomatic_1k's regions: ~1500 nodes for ~10 values).
const INTERFACE_NODES: usize = 40;
/// A closure region captures at most this many of the parent's bindings
/// (`PERRY_SPLIT_MAX_CAPTURES`): every capture is a word stored at each
/// call, so a closure region is kept as small an interface as a function
/// region.
fn max_captures() -> usize {
    env_usize("PERRY_SPLIT_MAX_CAPTURES", 64)
}

/// A `typeof`-guarded arm becomes its own typed region from this size on.
fn guard_min_nodes() -> usize {
    env_usize("PERRY_SPLIT_GUARD_NODES", 24)
}

/// A run of statements becomes a region from this size on.
fn chunk_min_nodes() -> usize {
    env_usize("PERRY_SPLIT_CHUNK_MIN_NODES", 200)
}

/// `PERRY_SPLIT_CLOSURES=1`: closure regions (ranges that share the
/// parent's cells become arrow closures that capture them). Off by default:
/// tsc's module body still computes a wrong value with them (an inlined
/// namespace IIFE's binding reads `undefined` inside a closure region), so
/// until that is found, a range that touches shared state stays in the
/// parent.
fn closure_regions_enabled() -> bool {
    matches!(
        std::env::var("PERRY_SPLIT_CLOSURES").as_deref(),
        Ok("1") | Ok("on") | Ok("true")
    )
}

/// The size limits and modes of one run (from the environment; tests pass
/// their own).
#[derive(Debug, Clone, Copy)]
pub(crate) struct Limits {
    pub min_body: usize,
    pub target: usize,
    pub guard_min: usize,
    pub chunk_min: usize,
    pub closures: bool,
    /// Sink hoisted function declarations to their first use.
    pub sink: bool,
    /// Nodes a run region must hold per value crossing its boundary.
    pub interface_nodes: usize,
    /// Give region parameters the parent's proofs (`PERRY_SPLIT_TYPED=0`:
    /// every parameter untyped, an A/B control).
    pub typed: bool,
    /// Stop after this many regions (`PERRY_SPLIT_MAX_REGIONS`, a bisection
    /// aid; unlimited by default).
    pub max_regions: usize,
}

impl Limits {
    pub(crate) fn from_env() -> Self {
        Self {
            min_body: min_body_nodes(),
            target: target_nodes(),
            guard_min: guard_min_nodes(),
            chunk_min: chunk_min_nodes(),
            closures: closure_regions_enabled(),
            interface_nodes: INTERFACE_NODES,
            sink: !matches!(
                std::env::var("PERRY_SPLIT_SINK").as_deref(),
                Ok("0") | Ok("off") | Ok("false")
            ),
            max_regions: env_usize("PERRY_SPLIT_MAX_REGIONS", usize::MAX),
            typed: !matches!(
                std::env::var("PERRY_SPLIT_TYPED").as_deref(),
                Ok("0") | Ok("off") | Ok("false")
            ),
        }
    }
}

fn report_enabled() -> bool {
    matches!(
        std::env::var("PERRY_SPLIT_REPORT").as_deref(),
        Ok("1") | Ok("on") | Ok("true")
    )
}

/// A function the splitter made.
pub(crate) fn is_region_function(f: &Function) -> bool {
    f.name.contains(REGION_MARK)
        && f.captures.is_empty()
        && !f.is_async
        && !f.is_generator
        && f.params
            .iter()
            .all(|p| p.default.is_none() && !p.is_rest && p.arguments_object.is_none())
}

fn is_proof_type(t: &Type) -> bool {
    matches!(
        t,
        Type::String | Type::Number | Type::Boolean | Type::Array(_)
    )
}

/// The parameter proofs of a region function: the types its only caller
/// proved for the arguments at the call.
pub(crate) fn region_param_proofs(f: &Function) -> HashMap<u32, Type> {
    if !is_region_function(f) {
        return HashMap::new();
    }
    f.params
        .iter()
        .filter(|p| is_proof_type(&p.ty))
        .map(|p| (p.id, p.ty.clone()))
        .collect()
}

/// Region parameters proven Number (normalized to a plain double at entry).
pub(crate) fn region_number_params(f: &Function) -> HashSet<u32> {
    if !is_region_function(f) {
        return HashSet::new();
    }
    f.params
        .iter()
        .filter(|p| matches!(p.ty, Type::Number))
        .map(|p| p.id)
        .collect()
}

// ---------------------------------------------------------------------------
// Deep visitors
// ---------------------------------------------------------------------------

fn for_each_child_list<'a>(s: &'a Stmt, f: &mut dyn FnMut(&'a [Stmt])) {
    match s {
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            f(then_branch);
            if let Some(e) = else_branch {
                f(e);
            }
        }
        Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => f(body),
        Stmt::For { init, body, .. } => {
            if let Some(i) = init {
                f(std::slice::from_ref(i.as_ref()));
            }
            f(body);
        }
        Stmt::Labeled { body, .. } => f(std::slice::from_ref(body.as_ref())),
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            f(body);
            if let Some(c) = catch {
                f(&c.body);
            }
            if let Some(fin) = finally {
                f(fin);
            }
        }
        Stmt::Switch { cases, .. } => {
            for c in cases {
                f(&c.body);
            }
        }
        _ => {}
    }
}

/// The expressions a statement holds directly (not those of nested lists).
fn for_each_own_expr<'a>(s: &'a Stmt, f: &mut dyn FnMut(&'a Expr)) {
    match s {
        Stmt::Let { init: Some(e), .. }
        | Stmt::Expr(e)
        | Stmt::Throw(e)
        | Stmt::Return(Some(e)) => f(e),
        Stmt::If { condition, .. }
        | Stmt::While { condition, .. }
        | Stmt::DoWhile { condition, .. } => f(condition),
        Stmt::For {
            condition, update, ..
        } => {
            if let Some(c) = condition {
                f(c);
            }
            if let Some(u) = update {
                f(u);
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
            }
        }
        _ => {}
    }
}

/// Visit every statement and expression under `stmts`. Closure bodies are
/// entered when `closures` is true; `on_expr` sees the closure node either way.
fn visit_stmts<'a>(
    stmts: &'a [Stmt],
    closures: bool,
    on_stmt: &mut dyn FnMut(&'a Stmt),
    on_expr: &mut dyn FnMut(&'a Expr),
) {
    for s in stmts {
        visit_stmt(s, closures, on_stmt, on_expr);
    }
}

fn visit_stmt<'a>(
    s: &'a Stmt,
    closures: bool,
    on_stmt: &mut dyn FnMut(&'a Stmt),
    on_expr: &mut dyn FnMut(&'a Expr),
) {
    on_stmt(s);
    for_each_own_expr(s, &mut |e| visit_expr(e, closures, on_stmt, on_expr));
    for_each_child_list(s, &mut |l| visit_stmts(l, closures, on_stmt, on_expr));
}

fn visit_expr<'a>(
    e: &'a Expr,
    closures: bool,
    on_stmt: &mut dyn FnMut(&'a Stmt),
    on_expr: &mut dyn FnMut(&'a Expr),
) {
    on_expr(e);
    if closures {
        if let Expr::Closure { body, .. } = e {
            visit_stmts(body, closures, on_stmt, on_expr);
        }
    }
    perry_hir::walker::walk_expr_children(e, &mut |c| visit_expr(c, closures, on_stmt, on_expr));
}

/// Every local id a statement list declares, at any depth, closures included
/// (lets, catch parameters, closure parameters, scoped temps).
fn declared_ids(stmts: &[Stmt], closures: bool) -> HashSet<u32> {
    let mut out = HashSet::new();
    let out_cell = std::cell::RefCell::new(&mut out);
    visit_stmts(
        stmts,
        closures,
        &mut |s| match s {
            Stmt::Let { id, .. } => {
                out_cell.borrow_mut().insert(*id);
            }
            Stmt::Try { catch: Some(c), .. } => {
                if let Some((id, _)) = &c.param {
                    out_cell.borrow_mut().insert(*id);
                }
            }
            _ => {}
        },
        &mut |e| match e {
            Expr::Closure { params, .. } if closures => {
                for p in params {
                    out_cell.borrow_mut().insert(p.id);
                }
            }
            Expr::ScopedTemp { id, .. } => {
                out_cell.borrow_mut().insert(*id);
            }
            _ => {}
        },
    );
    out
}

/// Reference counts of every local id under `stmts`, closures included (a
/// closure's body reads count, as do its capture lists).
fn ref_counts(stmts: &[Stmt], counts: &mut HashMap<u32, i64>, sign: i64) {
    let mut refs = Vec::new();
    let mut visited = HashSet::new();
    for s in stmts {
        perry_hir::collect_local_refs_stmt(s, &mut refs, &mut visited);
    }
    for id in refs {
        *counts.entry(id).or_insert(0) += sign;
    }
}

/// `Let` statements per id under `stmts` (not in nested closures).
fn let_counts(stmts: &[Stmt]) -> HashMap<u32, usize> {
    let mut out = HashMap::new();
    visit_stmts(
        stmts,
        false,
        &mut |s| {
            if let Stmt::Let { id, .. } = s {
                *out.entry(*id).or_insert(0) += 1;
            }
        },
        &mut |_| {},
    );
    out
}

/// Turn every declaration of an id in `ids` under `stmts` (not in nested
/// closures) into an assignment: these bindings belong to the parent, so a
/// declaration with an initializer assigns it and a bare one does nothing.
fn declarations_to_assignments(stmts: &mut Vec<Stmt>, ids: &HashMap<u32, u32>) {
    if ids.is_empty() {
        return;
    }
    let old = std::mem::take(stmts);
    for s in old {
        match s {
            Stmt::Let { id, init, .. } if ids.contains_key(&id) => {
                if let Some(e) = init {
                    stmts.push(Stmt::Expr(Expr::LocalSet(ids[&id], Box::new(e))));
                }
            }
            mut other => {
                match &mut other {
                    Stmt::If {
                        then_branch,
                        else_branch,
                        ..
                    } => {
                        declarations_to_assignments(then_branch, ids);
                        if let Some(e) = else_branch {
                            declarations_to_assignments(e, ids);
                        }
                    }
                    Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => {
                        declarations_to_assignments(body, ids)
                    }
                    Stmt::For { init, body, .. } => {
                        if let Some(i) = init {
                            if let Stmt::Let {
                                id, init: value, ..
                            } = i.as_mut()
                            {
                                if let Some(target) = ids.get(id).copied() {
                                    let set = value
                                        .take()
                                        .map(|e| Stmt::Expr(Expr::LocalSet(target, Box::new(e))));
                                    match set {
                                        Some(st) => **i = st,
                                        None => *i = Box::new(Stmt::Expr(Expr::Undefined)),
                                    }
                                }
                            }
                        }
                        declarations_to_assignments(body, ids);
                    }
                    Stmt::Labeled { body, .. } => {
                        let mut v = vec![std::mem::replace(body.as_mut(), Stmt::Break)];
                        declarations_to_assignments(&mut v, ids);
                        **body = if v.len() <= 1 {
                            v.pop().unwrap_or(Stmt::Expr(Expr::Undefined))
                        } else {
                            Stmt::If {
                                condition: Expr::Bool(true),
                                then_branch: v,
                                else_branch: None,
                            }
                        };
                    }
                    Stmt::Try {
                        body,
                        catch,
                        finally,
                    } => {
                        declarations_to_assignments(body, ids);
                        if let Some(c) = catch {
                            declarations_to_assignments(&mut c.body, ids);
                        }
                        if let Some(f) = finally {
                            declarations_to_assignments(f, ids);
                        }
                    }
                    Stmt::Switch { cases, .. } => {
                        for c in cases {
                            declarations_to_assignments(&mut c.body, ids);
                        }
                    }
                    _ => {}
                }
                stmts.push(other);
            }
        }
    }
}

fn ref_set(stmts: &[Stmt]) -> HashSet<u32> {
    let mut refs = Vec::new();
    let mut visited = HashSet::new();
    for s in stmts {
        perry_hir::collect_local_refs_stmt(s, &mut refs, &mut visited);
    }
    refs.into_iter().collect()
}

/// Ids written (`=`, `++`, compound assignment, `with` fallback) under
/// `stmts`, closures included.
fn written_ids(stmts: &[Stmt], out: &mut HashSet<u32>) {
    visit_stmts(stmts, true, &mut |_| {}, &mut |e| match e {
        Expr::LocalSet(id, _) | Expr::Update { id, .. } => {
            out.insert(*id);
        }
        // Codegen stores the class object into a class expression's
        // evaluation owner: a write the HIR does not spell out.
        Expr::ClassExprFresh {
            evaluation_owner: Some(id),
            ..
        } => {
            out.insert(*id);
        }
        Expr::WithSet { fallback, .. } => match fallback {
            perry_hir::WithSetFallback::Local(id)
            | perry_hir::WithSetFallback::SloppyImplicit(id) => {
                out.insert(*id);
            }
            _ => {}
        },
        _ => {}
    });
}

/// Ids referenced inside closures nested under `stmts` (their captures).
fn closure_captured_ids(stmts: &[Stmt]) -> HashSet<u32> {
    let mut out = HashSet::new();
    let out_cell = std::cell::RefCell::new(&mut out);
    visit_stmts(stmts, true, &mut |_| {}, &mut |e| {
        if let Expr::Closure {
            captures,
            mutable_captures,
            body,
            ..
        } = e
        {
            let mut o = out_cell.borrow_mut();
            o.extend(captures.iter().copied());
            o.extend(mutable_captures.iter().copied());
            o.extend(ref_set(body));
        }
    });
    out
}

// ---------------------------------------------------------------------------
// Module facts
// ---------------------------------------------------------------------------

struct ModuleFacts {
    limits: Limits,
    /// Ids written anywhere in the module after their declaration.
    writes: HashSet<u32>,
    /// Ids whose cell is preallocated (forward-referenced captures, TDZ).
    prealloc: HashSet<u32>,
    /// The preallocated cells that start in the temporal dead zone: their
    /// declaration initializes the cell and must stay a declaration.
    prealloc_tdz: HashSet<u32>,
    /// Classes whose construction reads or writes back locals through class
    /// capture parameters or a class environment.
    capturing_classes: HashSet<String>,
    /// Locals a class expression's evaluation writes behind the HIR's back
    /// (`ClassExprFresh::evaluation_owner`): they never cross a boundary.
    class_owners: HashSet<u32>,
}

fn all_module_bodies(hir: &HirModule) -> Vec<&[Stmt]> {
    let mut out: Vec<&[Stmt]> = vec![&hir.init];
    for f in &hir.functions {
        out.push(&f.body);
    }
    for c in &hir.classes {
        if let Some(ctor) = &c.constructor {
            out.push(&ctor.body);
        }
        for f in c
            .methods
            .iter()
            .chain(c.static_methods.iter())
            .chain(c.getters.iter().map(|(_, f)| f))
            .chain(c.setters.iter().map(|(_, f)| f))
            .chain(c.computed_members.iter().map(|m| &m.function))
        {
            out.push(&f.body);
        }
    }
    out
}

fn module_facts(hir: &HirModule, limits: Limits) -> ModuleFacts {
    let mut writes = HashSet::new();
    let mut prealloc = HashSet::new();
    let mut prealloc_tdz = HashSet::new();
    // A `var` is declared twice in HIR: a hoisted seed at the top of its
    // body and the declaration with its initializer at its source position.
    // The second declaration assigns the binding: a write.
    let mut decl_counts: HashMap<u32, usize> = HashMap::new();
    for body in all_module_bodies(hir) {
        visit_stmts(
            body,
            true,
            &mut |s| {
                if let Stmt::Let { id, .. } = s {
                    *decl_counts.entry(*id).or_insert(0) += 1;
                }
            },
            &mut |_| {},
        );
    }
    writes.extend(
        decl_counts
            .iter()
            .filter(|(_, n)| **n > 1)
            .map(|(id, _)| *id),
    );
    for body in all_module_bodies(hir) {
        written_ids(body, &mut writes);
        visit_stmts(
            body,
            true,
            &mut |s| match s {
                Stmt::PreallocateBoxes(ids) | Stmt::ReleaseBoxes(ids) => {
                    prealloc.extend(ids.iter().copied())
                }
                Stmt::PreallocateTdzBoxes(ids) => {
                    prealloc.extend(ids.iter().copied());
                    prealloc_tdz.extend(ids.iter().copied());
                }
                _ => {}
            },
            &mut |_| {},
        );
    }
    let mut class_owners = HashSet::new();
    for body in all_module_bodies(hir) {
        visit_stmts(body, true, &mut |_| {}, &mut |e| {
            if let Expr::ClassExprFresh {
                evaluation_owner: Some(id),
                ..
            } = e
            {
                class_owners.insert(*id);
            }
        });
    }
    let mut capturing_classes = HashSet::new();
    for c in &hir.classes {
        let cap_params = c.constructor.as_ref().is_some_and(|ctor| {
            ctor.params
                .iter()
                .any(|p| p.name.starts_with(perry_hir::cap_fields::CAP_FIELD_PREFIX))
        });
        let cap_fields = c
            .fields
            .iter()
            .chain(c.static_fields.iter())
            .any(|f| f.name.starts_with(perry_hir::cap_fields::CAP_FIELD_PREFIX));
        if cap_params || cap_fields {
            capturing_classes.insert(c.name.clone());
        }
    }
    ModuleFacts {
        limits,
        writes,
        prealloc,
        prealloc_tdz,
        capturing_classes,
        class_owners,
    }
}

fn max_local_id(hir: &HirModule) -> u32 {
    let mut max = 0u32;
    let mut note = |stmts: &[Stmt]| {
        for id in declared_ids(stmts, true) {
            max = max.max(id);
        }
        for id in ref_set(stmts) {
            max = max.max(id);
        }
        visit_stmts(
            stmts,
            true,
            &mut |s| match s {
                Stmt::PreallocateBoxes(ids)
                | Stmt::PreallocateTdzBoxes(ids)
                | Stmt::ReleaseBoxes(ids) => {
                    for id in ids {
                        max = max.max(*id);
                    }
                }
                _ => {}
            },
            &mut |_| {},
        );
    };
    for body in all_module_bodies(hir) {
        note(body);
    }
    let mut max = max;
    let mut fns: Vec<&Function> = hir.functions.iter().collect();
    for c in &hir.classes {
        fns.extend(c.constructor.iter());
        fns.extend(c.methods.iter());
        fns.extend(c.static_methods.iter());
        fns.extend(c.getters.iter().map(|(_, f)| f));
        fns.extend(c.setters.iter().map(|(_, f)| f));
        fns.extend(c.computed_members.iter().map(|m| &m.function));
    }
    for f in fns {
        for p in &f.params {
            max = max.max(p.id);
        }
        for id in &f.captures {
            max = max.max(*id);
        }
    }
    for id in hir.local_source_spans.keys() {
        max = max.max(*id);
    }
    max
}

// ---------------------------------------------------------------------------
// Statement admission
// ---------------------------------------------------------------------------

/// Why a statement (or a range) cannot move into a region.
fn frame_bound_expr(e: &Expr, facts: &ModuleFacts) -> Option<&'static str> {
    Some(match e {
        Expr::This | Expr::ModuleTopThis => "this",
        Expr::NewTarget => "new.target",
        Expr::SuperCall(_)
        | Expr::SuperCallSpread(_)
        | Expr::SuperMethodCall { .. }
        | Expr::SuperMethodCallSpread { .. }
        | Expr::SuperPropertyGet { .. }
        | Expr::SuperPropertySet { .. }
        | Expr::ObjectSuperPropertyGet { .. }
        | Expr::ObjectSuperPropertySet { .. }
        | Expr::ObjectSuperMethodCall { .. } => "super",
        Expr::Await(_) | Expr::Yield { .. } => "await/yield",
        Expr::WithGet { .. } | Expr::WithSet { .. } => "with",
        Expr::ClassMethodHome { .. }
        | Expr::ClassStaticSymbolSet { .. }
        | Expr::ClassEnvGet { .. }
        | Expr::ClassEnvSet { .. }
        | Expr::ClassEnvCurrent { .. }
        | Expr::ClassCaptureValue { .. }
        | Expr::ClassExprFresh { .. }
        | Expr::PrivateLexicalBrand(_)
        | Expr::PrivateLexicalBrandPush(_) => "class-environment",
        Expr::LinkGeneratorPrototype { .. }
        | Expr::CurrentStepClosure
        | Expr::AsyncFirstCall { .. }
        | Expr::AsyncGenResume { .. } => "generator-frame",
        Expr::Closure {
            captures_this,
            captures_new_target,
            ..
        } if *captures_this || *captures_new_target => "arrow-this",
        Expr::New { class_name, .. } if facts.capturing_classes.contains(class_name) => {
            "capturing-class"
        }
        Expr::ClassRef(name) if facts.capturing_classes.contains(name) => "capturing-class",
        // `new C()` of a class whose definition may run more than once,
        // recording the evaluation (the class binding, an ordinary local)
        // on the instance: movable when the class captures nothing.
        Expr::ClassEnvStamp { class_name, .. } if facts.capturing_classes.contains(class_name) => {
            "capturing-class"
        }
        _ => return None,
    })
}

/// Whether `s` can leave a range made of whole statements of one list other
/// than by falling through or throwing, or depends on the parent's frame.
fn stmt_is_pinned(s: &Stmt, facts: &ModuleFacts) -> bool {
    pin_reason(s, facts).is_some()
}

/// Why `s` stays in the parent, if it does.
fn pin_reason(s: &Stmt, facts: &ModuleFacts) -> Option<&'static str> {
    /// `top`: `s` is a statement of the range itself (not of a block in it).
    fn control(
        s: &Stmt,
        top: bool,
        loops: usize,
        switches: usize,
        labels: &mut Vec<String>,
    ) -> bool {
        let any = |l: &[Stmt], loops: usize, switches: usize, labels: &mut Vec<String>| {
            l.iter().any(|x| control(x, false, loops, switches, labels))
        };
        match s {
            Stmt::Return(_) => true,
            Stmt::Break => loops == 0 && switches == 0,
            Stmt::Continue => loops == 0,
            Stmt::LabeledBreak(l) | Stmt::LabeledContinue(l) => !labels.contains(l),
            // A block's own preallocation moves with the block; one at the
            // top of the range belongs to the parent's scope.
            Stmt::PreallocateBoxes(_) | Stmt::PreallocateTdzBoxes(_) => top,
            Stmt::ReleaseBoxes(_) => true,
            Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => {
                any(body, loops + 1, switches, labels)
            }
            Stmt::For { init, body, .. } => {
                init.as_ref()
                    .is_some_and(|i| control(i, false, loops, switches, labels))
                    || any(body, loops + 1, switches, labels)
            }
            Stmt::Switch { cases, .. } => cases
                .iter()
                .any(|c| any(&c.body, loops, switches + 1, labels)),
            Stmt::Labeled { label, body } => {
                labels.push(label.clone());
                let r = control(body, false, loops, switches, labels);
                labels.pop();
                r
            }
            Stmt::If {
                then_branch,
                else_branch,
                ..
            } => {
                any(then_branch, loops, switches, labels)
                    || else_branch
                        .as_ref()
                        .is_some_and(|e| any(e, loops, switches, labels))
            }
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                any(body, loops, switches, labels)
                    || catch
                        .as_ref()
                        .is_some_and(|c| any(&c.body, loops, switches, labels))
                    || finally
                        .as_ref()
                        .is_some_and(|f| any(f, loops, switches, labels))
            }
            _ => false,
        }
    }
    if control(s, true, 0, 0, &mut Vec::new()) {
        return Some("control-flow");
    }
    let mut bound = None;
    visit_stmt(s, false, &mut |_| {}, &mut |e| {
        if bound.is_none() {
            bound = frame_bound_expr(e, facts);
        }
    });
    bound
}

// ---------------------------------------------------------------------------
// Per-body context
// ---------------------------------------------------------------------------

struct BodyCtx<'a> {
    facts: &'a ModuleFacts,
    /// How many declarations (`Let`s) of each id the body holds.
    decl_counts: HashMap<u32, usize>,
    /// Ids declared exactly once in the body, with an initializer.
    single_init: HashSet<u32>,
    parent: String,
    is_strict: bool,
    /// Parameters of the body and the ids it captures (closures).
    params: HashSet<u32>,
    captures: HashSet<u32>,
    /// Ids declared in the body itself (not in nested closures).
    declared: HashSet<u32>,
    /// Ids nested closures of the body capture.
    captured: HashSet<u32>,
    /// The body's typed locals before splitting (String/Number/Array).
    typed: HashMap<u32, Type>,
    /// Names of locals for readable region parameters.
    names: HashMap<u32, String>,
    /// Reference counts over the whole body, kept current as ranges move.
    counts: HashMap<u32, i64>,
}

struct Shared<'a> {
    next_func: u32,
    next_local: u32,
    region_seq: usize,
    out: Vec<Function>,
    spans: &'a mut HashMap<LocalId, perry_hir::LocalSourceSpan>,
    report: Vec<String>,
    /// Display names of closure regions.
    closure_names: Vec<(u32, String)>,
}

impl Shared<'_> {
    fn fresh_local(&mut self, like: u32) -> u32 {
        self.next_local += 1;
        let id = self.next_local;
        if let Some(sp) = self.spans.get(&like).copied() {
            self.spans.insert(id, sp);
        }
        id
    }
}

/// A declaration that stays in the parent: its cell is preallocated and the
/// declaration initializes it (always without closure regions; with them,
/// only a temporal-dead-zone cell, whose declaration ends the dead zone).
fn declaration_stays(facts: &ModuleFacts, id: u32) -> bool {
    if facts.limits.closures {
        facts.prealloc_tdz.contains(&id)
    } else {
        facts.prealloc.contains(&id)
    }
}

/// A `var`'s hoisted seed (`let x;` at the top, a later declaration gives the
/// value) of a binding closures capture: it creates the parent's cell, so it
/// stays in the parent ahead of every closure that captures the binding.
fn is_shared_seed(ctx: &BodyCtx<'_>, s: &Stmt) -> bool {
    match s {
        Stmt::Let {
            id,
            init: None | Some(Expr::Undefined),
            ..
        } => {
            ctx.decl_counts.get(id).copied().unwrap_or(0) > 1
                && (ctx.captured.contains(id) || ctx.facts.prealloc.contains(id))
        }
        _ => false,
    }
}

fn shared_mutable(ctx: &BodyCtx<'_>, id: u32) -> bool {
    (ctx.captured.contains(&id) || ctx.captures.contains(&id)) && ctx.facts.writes.contains(&id)
}

fn type_name(t: &Type) -> &'static str {
    match t {
        Type::String => "string",
        Type::Number => "number",
        Type::Boolean => "boolean",
        Type::Array(_) => "array",
        _ => "any",
    }
}

/// `typeof v === "<t>"` (either order, `==` too) or a left-nested `&&` whose
/// first conjunct is one: the local and the proof for the true arm.
fn guard_proof(cond: &Expr) -> Option<(u32, Type)> {
    match cond {
        Expr::Logical {
            op: LogicalOp::And,
            left,
            ..
        } => guard_proof(left),
        Expr::Compare { op, left, right } if matches!(op, CompareOp::Eq | CompareOp::LooseEq) => {
            let (tested, lit) = match (left.as_ref(), right.as_ref()) {
                (Expr::TypeOf(x), Expr::String(s)) | (Expr::String(s), Expr::TypeOf(x)) => (x, s),
                _ => return None,
            };
            let Expr::LocalGet(id) = tested.as_ref() else {
                return None;
            };
            let ty = match lit.as_str() {
                "string" => Type::String,
                "number" => Type::Number,
                "boolean" => Type::Boolean,
                _ => return None,
            };
            Some((*id, ty))
        }
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Extraction
// ---------------------------------------------------------------------------

struct Extracted {
    calls: Vec<Stmt>,
}

/// Try to move `range` (whole statements of one list) into a region.
/// `dominating`: ids whose declaration has run when the range starts.
/// `proofs`: guard proofs that hold for the whole range.
fn try_extract(
    range: &[Stmt],
    ctx: &mut BodyCtx<'_>,
    sh: &mut Shared<'_>,
    dominating: &HashSet<u32>,
    proofs: &HashMap<u32, Type>,
    kind: &str,
) -> Option<Extracted> {
    // A value region: `range` is one `Stmt::Expr(e)` whose value the
    // region returns (a declaration's initializer, a returned value, the
    // right side of an assignment the parent keeps).
    let value = kind == "value";
    if range.is_empty() || range.iter().any(|s| stmt_is_pinned(s, ctx.facts)) {
        return None;
    }
    if sh.region_seq >= ctx.facts.limits.max_regions {
        return None;
    }
    let reject = |sh: &mut Shared<'_>, ctx: &BodyCtx<'_>, why: &str, id: u32| {
        if report_enabled() {
            sh.report.push(format!(
                "perry-split\treject\t{}\t{}\t{}\tnodes={}\tkind={}",
                sanitize_tsv(&ctx.parent),
                why,
                ctx.names
                    .get(&id)
                    .cloned()
                    .unwrap_or_else(|| format!("v{id}")),
                crate::collectors::count_body_nodes(range),
                kind
            ));
        }
    };
    let mut inside_decl = declared_ids(range, true);
    let mut range_counts = HashMap::new();
    ref_counts(range, &mut range_counts, 1);
    let mut range_writes = HashSet::new();
    written_ids(range, &mut range_writes);
    // A binding the parent also declares outside the range (a `var`'s
    // hoisted seed, a redeclaration) is the parent's: its declarations in
    // the range are assignments.
    let range_lets = let_counts(range);
    let mut outer_declared: HashSet<u32> = HashSet::new();
    for (id, n) in &range_lets {
        if ctx.decl_counts.get(id).copied().unwrap_or(0) > *n {
            outer_declared.insert(*id);
            inside_decl.remove(id);
            range_writes.insert(*id);
            range_counts.entry(*id).or_insert(0);
        }
    }
    // Top-level lets of the range: the only declarations that may be live-out.
    let top_lets: Vec<(u32, String, Type, bool)> = range
        .iter()
        .filter_map(|s| match s {
            Stmt::Let {
                id,
                name,
                ty,
                mutable,
                ..
            } => Some((*id, name.clone(), ty.clone(), *mutable)),
            _ => None,
        })
        .collect();
    let top_let_ids: HashSet<u32> = top_lets
        .iter()
        .map(|t| t.0)
        .filter(|id| !outer_declared.contains(id))
        .collect();

    let outside = |ctx: &BodyCtx<'_>, id: u32| -> bool {
        ctx.counts.get(&id).copied().unwrap_or(0) - range_counts.get(&id).copied().unwrap_or(0) > 0
    };

    // A closure region shares the parent's cells: the bindings that cannot
    // cross by value are captured, exactly as any closure of the parent
    // captures them (already boxed there: preallocated or written shared).
    let closures_ok = ctx.facts.limits.closures && !value;
    let mut capture_ids: BTreeSet<u32> = BTreeSet::new();
    // Top-level declarations of captured bindings: they become assignments
    // to the parent's cell; `decl_in_parent` ones also get a declaration in
    // the parent before the call (their cell is not preallocated).
    let mut convert_lets: HashSet<u32> = HashSet::new();
    let mut decl_in_parent: Vec<u32> = Vec::new();

    // A class expression's evaluation owner is written by codegen where the
    // class is evaluated: it stays in one body with all its uses.
    for (id, _) in &range_counts {
        if ctx.facts.class_owners.contains(id) && (!inside_decl.contains(id) || outside(ctx, *id)) {
            reject(sh, ctx, "class-expression-owner", *id);
            return None;
        }
    }

    // Declarations made inside the range and used after it.
    let mut out_decl: Vec<u32> = Vec::new();
    let mut inside_sorted: Vec<u32> = inside_decl.iter().copied().collect();
    inside_sorted.sort_unstable();
    for id in &inside_sorted {
        if !outside(ctx, *id) {
            continue;
        }
        if !top_let_ids.contains(id) {
            reject(sh, ctx, "nested-decl-used-after", *id);
            return None;
        }
        if ctx.facts.prealloc.contains(id) || shared_mutable(ctx, *id) {
            if !closures_ok || ctx.facts.prealloc_tdz.contains(id) {
                reject(sh, ctx, "shared-decl-used-after", *id);
                return None;
            }
            capture_ids.insert(*id);
            convert_lets.insert(*id);
            if !ctx.facts.prealloc.contains(id) {
                decl_in_parent.push(*id);
            }
            continue;
        }
        out_decl.push(*id);
    }
    // Live-ins (and the outer locals the range writes).
    let mut live_in: Vec<u32> = Vec::new();
    let mut out_outer: Vec<u32> = Vec::new();
    let mut ids: Vec<u32> = range_counts.keys().copied().collect();
    ids.sort_unstable();
    for id in ids {
        if inside_decl.contains(&id) {
            continue;
        }
        let parent_local = ctx.params.contains(&id) || ctx.declared.contains(&id);
        let capture = ctx.captures.contains(&id);
        if !parent_local && !capture {
            // A module-scope binding: a top-level function reaches it the
            // same way the parent does.
            continue;
        }
        if shared_mutable(ctx, id) {
            if closures_ok {
                capture_ids.insert(id);
                continue;
            }
            reject(sh, ctx, "shared-live-in", id);
            return None;
        }
        // A preallocated cell (a forward-captured binding) holds its value
        // once its declaration has run, and keeps it when nothing writes it.
        if ctx.facts.prealloc.contains(&id)
            && (ctx.facts.writes.contains(&id) || !dominating.contains(&id))
        {
            if closures_ok {
                capture_ids.insert(id);
                continue;
            }
            reject(sh, ctx, "prealloc-live-in", id);
            return None;
        }
        let captured = ctx.captured.contains(&id) || capture;
        if captured && !dominating.contains(&id) {
            // A capture read by value must already hold its value; a closure
            // region captures it the way the parent's closures do.
            if closures_ok {
                capture_ids.insert(id);
                continue;
            }
            reject(sh, ctx, "capture-not-dominating", id);
            return None;
        }
        live_in.push(id);
        if range_writes.contains(&id) && outside(ctx, id) {
            out_outer.push(id);
        }
    }
    out_decl.sort_unstable();
    if out_decl.len() + out_outer.len() > MAX_OUTS {
        reject(
            sh,
            ctx,
            "too-many-live-outs",
            out_decl.first().copied().unwrap_or(0),
        );
        return None;
    }
    if live_in.len() > MAX_PARAMS {
        // A closure region takes the read-only live-ins it has no proof for
        // as captures (the value at the call, exactly what a parameter
        // would carry); the written ones and the typed ones stay parameters.
        if !closures_ok {
            reject(sh, ctx, "too-many-live-ins", live_in[0]);
            return None;
        }
        let mut keep = Vec::new();
        for id in std::mem::take(&mut live_in) {
            let typed = proofs.contains_key(&id) || ctx.typed.contains_key(&id);
            if out_outer.contains(&id) || (typed && keep.len() < MAX_PARAMS) {
                keep.push(id);
            } else {
                capture_ids.insert(id);
            }
        }
        if keep.len() > MAX_PARAMS {
            reject(sh, ctx, "too-many-live-ins", keep[0]);
            return None;
        }
        live_in = keep;
    }
    // A captured binding must have its storage (its cell) in the parent when
    // the region closure is created, or the closure would get a cell of its
    // own: a parameter, a capture, a preallocated cell, or a declaration that
    // has already run.
    for id in &capture_ids {
        if convert_lets.contains(id) {
            continue;
        }
        let has_storage = ctx.params.contains(id)
            || ctx.captures.contains(id)
            || ctx.facts.prealloc.contains(id)
            || dominating.contains(id);
        if !has_storage {
            reject(sh, ctx, "capture-without-storage", *id);
            return None;
        }
    }
    // A run region must be large next to its interface: every value in or
    // out is a parameter or a returned slot the parent unpacks, so a range
    // that is mostly interface (a straight line of values all used at the
    // end) gains nothing from moving and costs at every call.
    if kind == "run" {
        let nodes = crate::collectors::count_body_nodes(range);
        let interface = live_in.len() + out_decl.len() + out_outer.len();
        if interface * ctx.facts.limits.interface_nodes > nodes {
            reject(
                sh,
                ctx,
                "interface-too-wide",
                live_in.first().copied().unwrap_or(0),
            );
            return None;
        }
    }
    if capture_ids.len() > max_captures() {
        reject(
            sh,
            ctx,
            "too-many-captures",
            capture_ids.iter().next().copied().unwrap_or(0),
        );
        return None;
    }
    if value && !(out_decl.is_empty() && out_outer.is_empty()) {
        reject(
            sh,
            ctx,
            "value-writes-outer",
            out_outer.first().copied().unwrap_or(0),
        );
        return None;
    }

    // Build the region.
    let mut remap: HashMap<u32, u32> = HashMap::new();
    let mut params: Vec<Param> = Vec::new();
    let mut param_desc: Vec<String> = Vec::new();
    for &id in &live_in {
        let fresh = sh.fresh_local(id);
        remap.insert(id, fresh);
        let proof = proofs
            .get(&id)
            .cloned()
            .or_else(|| {
                // A typed local holds its type at every READ; the call reads
                // it only when its one declaration (with a value) has run.
                // Only for a plain local of the parent: what closures capture
                // or the parent captures is typed under cell rules this
                // pass does not replay.
                (ctx.single_init.contains(&id)
                    && dominating.contains(&id)
                    && !ctx.captured.contains(&id)
                    && !ctx.captures.contains(&id)
                    && !ctx.facts.prealloc.contains(&id))
                .then(|| ctx.typed.get(&id).cloned())
                .flatten()
            })
            .filter(is_proof_type)
            .filter(|_| ctx.facts.limits.typed)
            .unwrap_or(Type::Any);
        let name = ctx
            .names
            .get(&id)
            .cloned()
            .unwrap_or_else(|| format!("v{id}"));
        param_desc.push(format!("{}:{}", name, type_name(&proof)));
        params.push(Param {
            id: fresh,
            name,
            ty: proof,
            default: None,
            decorators: Vec::new(),
            is_rest: false,
            arguments_object: None,
        });
    }
    for &id in &out_decl {
        let fresh = sh.fresh_local(id);
        remap.insert(id, fresh);
    }
    let mut body: Vec<Stmt> = range.to_vec();
    perry_hir::analysis::remap_local_ids_in_stmts(&mut body, &remap);
    // The remap rewrites uses only; a live-out declared here is declared
    // under its fresh id inside the region.
    for s in body.iter_mut() {
        if let Stmt::Let { id, .. } = s {
            if let Some(new_id) = remap.get(id) {
                *id = *new_id;
            }
        }
    }
    {
        // Declarations keep their original ids (the remap rewrites uses).
        let as_assignment: HashMap<u32, u32> = outer_declared
            .iter()
            .map(|id| (*id, remap.get(id).copied().unwrap_or(*id)))
            .collect();
        declarations_to_assignments(&mut body, &as_assignment);
    }
    // A binding declared here whose cell the parent preallocated at its
    // entry (a closure created before the declaration captures it) now
    // belongs to the region: the region preallocates it, at its own entry.
    let mut prealloc_in_range: HashSet<u32> = HashSet::new();
    visit_stmts(
        range,
        true,
        &mut |s| match s {
            Stmt::PreallocateBoxes(ids) | Stmt::PreallocateTdzBoxes(ids) => {
                prealloc_in_range.extend(ids.iter().copied())
            }
            _ => {}
        },
        &mut |_| {},
    );
    let mut own_cells: Vec<u32> = Vec::new();
    let mut own_tdz_cells: Vec<u32> = Vec::new();
    for id in &inside_sorted {
        if !ctx.facts.prealloc.contains(id)
            || prealloc_in_range.contains(id)
            || capture_ids.contains(id)
        {
            continue;
        }
        let local = remap.get(id).copied().unwrap_or(*id);
        if ctx.facts.prealloc_tdz.contains(id) {
            own_tdz_cells.push(local);
        } else {
            own_cells.push(local);
        }
    }
    if !own_tdz_cells.is_empty() {
        body.insert(0, Stmt::PreallocateTdzBoxes(own_tdz_cells));
    }
    if !own_cells.is_empty() {
        body.insert(0, Stmt::PreallocateBoxes(own_cells));
    }
    // A captured binding declared here is the parent's: its declaration
    // becomes an assignment to the parent's cell.
    let closure_region = !capture_ids.is_empty();
    if closure_region {
        body = body
            .into_iter()
            .filter_map(|s| match s {
                Stmt::Let { id, init, .. } if convert_lets.contains(&id) => {
                    init.map(|e| Stmt::Expr(Expr::LocalSet(id, Box::new(e))))
                }
                other => Some(other),
            })
            .collect();
    }
    let outs: Vec<u32> = out_decl.iter().chain(out_outer.iter()).copied().collect();
    if value {
        if let Some(Stmt::Expr(e)) = body.pop() {
            body.push(Stmt::Return(Some(e)));
        }
    }
    match outs.len() {
        0 => {}
        1 => body.push(Stmt::Return(Some(Expr::LocalGet(remap[&outs[0]])))),
        _ => body.push(Stmt::Return(Some(Expr::Array(
            outs.iter().map(|id| Expr::LocalGet(remap[id])).collect(),
            None,
        )))),
    }
    let func_id = sh.next_func + 1;
    sh.next_func = func_id;
    sh.region_seq += 1;
    let name = format!("{}{}{}", ctx.parent, REGION_MARK, sh.region_seq);
    let kind = if closure_region { "closure" } else { kind };

    // Report: where it came from, its parameter proofs, its typed locals.
    if report_enabled() {
        // Source position of each statement: the largest offset it carries
        // (its own lets and calls; an inlined helper's locals and calls carry
        // the helper's smaller offsets).
        let mut positions: Vec<u32> = Vec::new();
        for st in range {
            let mut pos = 0u32;
            if let Stmt::Let { id, .. } = st {
                if let Some(sp) = sh.spans.get(id) {
                    pos = pos.max(sp.start);
                }
            }
            visit_stmt(st, false, &mut |_| {}, &mut |e| match e {
                Expr::Call { byte_offset, .. } | Expr::New { byte_offset, .. } => {
                    pos = pos.max(*byte_offset);
                }
                _ => {}
            });
            if pos > 0 {
                positions.push(pos);
            }
        }
        let stmt_positions = positions
            .iter()
            .map(|p| p.to_string())
            .collect::<Vec<_>>()
            .join(",");
        // Inlined helpers carry their own (far away) offsets: drop outliers
        // more than four median deviations below the median.
        positions.sort_unstable();
        let (mut lo, mut hi) = (u32::MAX, 0u32);
        if !positions.is_empty() {
            let med = positions[positions.len() / 2] as i64;
            let mut dev: Vec<i64> = positions.iter().map(|p| (*p as i64 - med).abs()).collect();
            dev.sort_unstable();
            let mad = dev[dev.len() / 2].max(1);
            for p in &positions {
                if (*p as i64) >= med - 4 * mad {
                    lo = lo.min(*p);
                }
            }
            hi = *positions.last().unwrap();
        }
        let stable: HashMap<u32, Type> = params
            .iter()
            .filter(|p| is_proof_type(&p.ty))
            .map(|p| (p.id, p.ty.clone()))
            .collect();
        let tl = crate::collectors::typed_locals::collect_typed_locals(
            &body,
            &stable,
            &HashSet::new(),
            &HashMap::new(),
        );
        let mut typed_names: BTreeSet<String> = BTreeSet::new();
        let rev: HashMap<u32, u32> = remap.iter().map(|(a, b)| (*b, *a)).collect();
        let nm = |id: &u32| {
            let orig = rev.get(id).copied().unwrap_or(*id);
            ctx.names
                .get(&orig)
                .cloned()
                .unwrap_or_else(|| format!("v{orig}"))
        };
        for id in &tl.strings {
            typed_names.insert(format!("{}:string", nm(id)));
        }
        for id in &tl.numbers {
            typed_names.insert(format!("{}:number", nm(id)));
        }
        for id in &tl.arrays {
            typed_names.insert(format!("{}:array", nm(id)));
        }
        let span = if lo <= hi {
            format!("{lo}-{hi}")
        } else {
            "-".to_string()
        };
        sh.report.push(format!(
            "perry-split\tregion\t{}\t{}\tkind={}\tfid={}\tcaptures={}\tnodes={}\tspan={}\tparams={}\touts={}\ttyped_locals={}\tstmts={}",
            sanitize_tsv(&ctx.parent),
            sanitize_tsv(&name),
            kind,
            func_id,
            capture_ids.len(),
            crate::collectors::count_body_nodes(range),
            span,
            param_desc.join(","),
            outs.iter()
                .map(|id| ctx.names.get(id).cloned().unwrap_or_else(|| format!("v{id}")))
                .collect::<Vec<_>>()
                .join(","),
            typed_names.into_iter().collect::<Vec<_>>().join(","),
            stmt_positions,
        ));
    }

    if std::env::var("PERRY_SPLIT_DUMP_REGION")
        .ok()
        .is_some_and(|n| n == name)
    {
        eprintln!(
            "perry-split region dump {name}: params={params:#?} captures={capture_ids:?} convert={convert_lets:?} outs={outs:?} body={body:#?}"
        );
    }
    let callee = if closure_region {
        // Captured exactly like the parent's own closures capture these
        // bindings; the ones anything writes (or whose cell is preallocated)
        // are shared cells.
        let captures: Vec<u32> = capture_ids.iter().copied().collect();
        let mutable_captures: Vec<u32> = captures
            .iter()
            .copied()
            .filter(|id| {
                ctx.facts.writes.contains(id)
                    || ctx.facts.prealloc.contains(id)
                    || convert_lets.contains(id)
            })
            .collect();
        sh.closure_names.push((func_id, name));
        Expr::Closure {
            func_id,
            params,
            return_type: Type::Any,
            body,
            captures,
            mutable_captures,
            captures_this: false,
            captures_new_target: false,
            enclosing_class: None,
            is_arrow: true,
            is_async: false,
            is_generator: false,
            is_strict: ctx.is_strict,
        }
    } else {
        sh.out.push(Function {
            id: func_id,
            name,
            type_params: Vec::new(),
            params,
            return_type: Type::Any,
            body,
            is_async: false,
            is_generator: false,
            is_strict: ctx.is_strict,
            is_exported: false,
            captures: Vec::new(),
            decorators: Vec::new(),
            was_plain_async: false,
            was_unrolled: false,
        });
        Expr::FuncRef(func_id)
    };

    // The call and the unpacking of live-outs.
    let call = Expr::Call {
        callee: Box::new(callee),
        args: live_in.iter().map(|id| Expr::LocalGet(*id)).collect(),
        type_args: Vec::new(),
        byte_offset: 0,
    };
    // A live-out keeps the type the parent's typed locals gave it: the value
    // is the same, and `+v` / `String(v)` is the identity on it.
    let retype = |id: u32, e: Expr, ctx: &BodyCtx<'_>| -> Expr {
        // Only where the binding certainly holds a value of its type here:
        // its single declaration has run (it is in the range or before it).
        let assigned = ctx.single_init.contains(&id)
            && (top_let_ids.contains(&id) || dominating.contains(&id))
            && !ctx.captured.contains(&id)
            && !ctx.facts.prealloc.contains(&id);
        if !assigned {
            return e;
        }
        match ctx.typed.get(&id) {
            Some(Type::Number) => Expr::Unary {
                op: UnaryOp::Pos,
                operand: Box::new(e),
            },
            Some(Type::String) => Expr::StringCoerce(Box::new(e)),
            _ => e,
        }
    };
    let bind = |id: u32, value: Expr, ctx: &BodyCtx<'_>| -> Stmt {
        let value = retype(id, value, ctx);
        if let Some((_, name, ty, mutable)) = top_lets.iter().find(|t| t.0 == id) {
            Stmt::Let {
                id,
                name: name.clone(),
                ty: ty.clone(),
                mutable: *mutable,
                init: Some(value),
            }
        } else {
            Stmt::Expr(Expr::LocalSet(id, Box::new(value)))
        }
    };
    let mut calls = Vec::new();
    for id in &decl_in_parent {
        if let Some((_, name, ty, _)) = top_lets.iter().find(|t| t.0 == *id) {
            calls.push(Stmt::Let {
                id: *id,
                name: name.clone(),
                ty: ty.clone(),
                mutable: true,
                init: None,
            });
        }
    }
    match outs.len() {
        0 if value => {
            ref_counts(range, &mut ctx.counts, -1);
            let c = vec![Stmt::Expr(call)];
            ref_counts(&c, &mut ctx.counts, 1);
            return Some(Extracted { calls: c });
        }
        0 => calls.push(Stmt::Expr(call)),
        1 => calls.push(bind(outs[0], call, ctx)),
        _ => {
            let tmp = sh.fresh_local(u32::MAX);
            calls.push(Stmt::Let {
                id: tmp,
                name: "__region_out".to_string(),
                ty: Type::Any,
                mutable: false,
                init: Some(call),
            });
            for (k, id) in outs.iter().enumerate() {
                let get = Expr::IndexGet {
                    object: Box::new(Expr::LocalGet(tmp)),
                    index: Box::new(Expr::Number(k as f64)),
                };
                calls.push(bind(*id, get, ctx));
            }
        }
    }
    // Keep the body's reference and declaration counts current.
    ref_counts(range, &mut ctx.counts, -1);
    ref_counts(&calls, &mut ctx.counts, 1);
    for (id, n) in &range_lets {
        if let Some(c) = ctx.decl_counts.get_mut(id) {
            *c = c.saturating_sub(*n);
        }
    }
    for (id, n) in let_counts(&calls) {
        *ctx.decl_counts.entry(id).or_insert(0) += n;
    }
    Some(Extracted { calls })
}

fn sanitize_tsv(s: &str) -> String {
    s.chars()
        .map(|c| if c == '\t' || c == '\n' { ' ' } else { c })
        .collect()
}

/// Extract `list[a..b]`; on failure, halves down to the minimum chunk size.
/// Returns the replacement statements for `list[a..b]`.
fn extract_or_halve(
    list: &[Stmt],
    weights: &[usize],
    a: usize,
    b: usize,
    ctx: &mut BodyCtx<'_>,
    sh: &mut Shared<'_>,
    dominating_before: &HashSet<u32>,
    proofs: &HashMap<u32, Type>,
) -> Vec<Stmt> {
    let w: usize = weights[a..b].iter().sum();
    if w < ctx.facts.limits.chunk_min || a >= b {
        return list[a..b].to_vec();
    }
    // One statement far above the target (a giant switch or literal) gains
    // nothing from moving: the region would be the same giant, plus a call.
    if b - a == 1 && w > 2 * ctx.facts.limits.target {
        return list[a..b].to_vec();
    }
    // Lets of list[..a] dominate the range too.
    let mut dom = dominating_before.clone();
    for s in &list[..a] {
        if let Stmt::Let {
            id, init: Some(_), ..
        } = s
        {
            dom.insert(*id);
        }
    }
    if let Some(x) = try_extract(&list[a..b], ctx, sh, &dom, proofs, "run") {
        return x.calls;
    }
    if b - a < 2 {
        return list[a..b].to_vec();
    }
    // Halve at the weight midpoint. The right half first, so the left half
    // sees the right half's call when it counts outside references.
    let mut acc = 0usize;
    let mut mid = a + 1;
    for (i, wi) in weights.iter().enumerate().take(b).skip(a) {
        acc += wi;
        if acc * 2 >= w {
            mid = (i + 1).min(b - 1).max(a + 1);
            break;
        }
    }
    let right = extract_or_halve(list, weights, mid, b, ctx, sh, dominating_before, proofs);
    let left = extract_or_halve(list, weights, a, mid, ctx, sh, dominating_before, proofs);
    let mut out = left;
    out.extend(right);
    out
}

/// For each gap `i` of `list` (before statement `i`; `list.len() + 1` gaps),
/// how many of the list's own declarations made before the gap are used at
/// or after it.
fn live_across(list: &[Stmt]) -> Vec<usize> {
    let n = list.len();
    let mut decl_at: HashMap<u32, usize> = HashMap::new();
    for (j, s) in list.iter().enumerate() {
        if let Stmt::Let { id, .. } = s {
            decl_at.insert(*id, j);
        }
    }
    let mut last_use: HashMap<u32, usize> = HashMap::new();
    for (j, s) in list.iter().enumerate() {
        for id in ref_set(std::slice::from_ref(s)) {
            if decl_at.contains_key(&id) {
                last_use.insert(id, j);
            }
        }
    }
    let mut diff = vec![0i64; n + 2];
    for (id, d) in &decl_at {
        if let Some(&u) = last_use.get(id) {
            if u > *d {
                diff[d + 1] += 1;
                diff[u + 1] -= 1;
            }
        }
    }
    let mut out = vec![0usize; n + 1];
    let mut cur = 0i64;
    for (i, slot) in out.iter_mut().enumerate() {
        cur += diff[i];
        *slot = cur.max(0) as usize;
    }
    out
}

/// Split one statement list. `dominating`: ids declared before the list
/// starts (enclosing lists, parameters, captures).
fn split_list(
    list: &mut Vec<Stmt>,
    ctx: &mut BodyCtx<'_>,
    sh: &mut Shared<'_>,
    dominating: &HashSet<u32>,
    proofs: &HashMap<u32, Type>,
) {
    let target = ctx.facts.limits.target;
    // 1. Inside statements: typed guard arms become their own regions, and
    //    big arms/blocks (never loop bodies) are split in turn.
    let mut dom = dominating.clone();
    for i in 0..list.len() {
        let weight = crate::collectors::count_body_nodes(std::slice::from_ref(&list[i]));
        match &mut list[i] {
            Stmt::If {
                condition,
                then_branch,
                else_branch,
            } => {
                let guard = guard_proof(condition);
                let mut arm_proofs = proofs.clone();
                if let Some((id, ty)) = &guard {
                    arm_proofs.insert(*id, ty.clone());
                }
                let then_w = crate::collectors::count_body_nodes(then_branch);
                if then_w > target {
                    split_list(then_branch, ctx, sh, &dom, &arm_proofs);
                }
                if guard.is_some() && then_w >= ctx.facts.limits.guard_min {
                    let arm = std::mem::take(then_branch);
                    match try_extract(&arm, ctx, sh, &dom, &arm_proofs, "guard") {
                        Some(x) => *then_branch = x.calls,
                        None => *then_branch = arm,
                    }
                }
                if let Some(e) = else_branch {
                    if crate::collectors::count_body_nodes(e) > target {
                        split_list(e, ctx, sh, &dom, proofs);
                    }
                }
            }
            Stmt::Try {
                body,
                catch,
                finally,
            } if weight > target => {
                if crate::collectors::count_body_nodes(body) > target {
                    split_list(body, ctx, sh, &dom, proofs);
                }
                if let Some(c) = catch {
                    if crate::collectors::count_body_nodes(&c.body) > target {
                        let mut d = dom.clone();
                        if let Some((id, _)) = &c.param {
                            d.insert(*id);
                        }
                        split_list(&mut c.body, ctx, sh, &d, proofs);
                    }
                }
                if let Some(f) = finally {
                    if crate::collectors::count_body_nodes(f) > target {
                        split_list(f, ctx, sh, &dom, proofs);
                    }
                }
            }
            Stmt::Switch { cases, .. } if weight > target => {
                for c in cases.iter_mut() {
                    if crate::collectors::count_body_nodes(&c.body) > target {
                        split_list(&mut c.body, ctx, sh, &dom, proofs);
                    }
                }
            }
            Stmt::Labeled { body, .. } if weight > target => {
                // A labeled plain block: `label: { ... }` is lowered as a
                // labeled `if (true)`-like block in some paths; only lists
                // that run once per execution are split, so recurse into an
                // `If` body only.
                if let Stmt::If { then_branch, .. } = body.as_mut() {
                    if crate::collectors::count_body_nodes(then_branch) > target {
                        split_list(then_branch, ctx, sh, &dom, proofs);
                    }
                }
            }
            _ => {}
        }
        // A statement that stays in the parent (it returns, or declares a
        // preallocated cell, or assigns a shared binding) can still give its
        // big value away: the value becomes a region the parent calls.
        let stays = stmt_is_pinned(&list[i], ctx.facts)
            || matches!(&list[i], Stmt::Let { id, .. } if declaration_stays(ctx.facts, *id))
            || (!ctx.facts.limits.closures
                && matches!(&list[i], Stmt::Expr(Expr::LocalSet(id, _)) if shared_mutable(ctx, *id)));
        if stays {
            let slot: Option<&mut Expr> = match &mut list[i] {
                Stmt::Let { init: Some(e), .. } | Stmt::Return(Some(e)) => Some(e),
                Stmt::Expr(Expr::LocalSet(_, e)) => Some(e.as_mut()),
                _ => None,
            };
            if let Some(e) = slot {
                if crate::collectors::count_body_nodes(&[Stmt::Expr(e.clone())])
                    >= ctx.facts.limits.chunk_min
                {
                    let range = vec![Stmt::Expr(std::mem::replace(e, Expr::Undefined))];
                    match try_extract(&range, ctx, sh, &dom, proofs, "value") {
                        Some(mut x) => {
                            if let Some(Stmt::Expr(call)) = x.calls.pop() {
                                *e = call;
                            }
                        }
                        None => {
                            if let Some(Stmt::Expr(orig)) = range.into_iter().next() {
                                *e = orig;
                            }
                        }
                    }
                }
            }
        }
        // A `var`'s hoisted seed (`let x;` at the top) does not give it a
        // value; only a declaration with an initializer does.
        if let Stmt::Let {
            id, init: Some(_), ..
        } = &list[i]
        {
            dom.insert(*id);
        }
    }

    // 2. Runs of statements of this list, cut into chunks of ~target nodes.
    let weights: Vec<usize> = list
        .iter()
        .map(|s| crate::collectors::count_body_nodes(std::slice::from_ref(s)))
        .collect();
    let total: usize = weights.iter().sum();
    if total < ctx.facts.limits.chunk_min {
        return;
    }
    // A declaration whose cell is preallocated (closures created earlier
    // capture it) stays where it is: its later readers may move.
    let pinned: Vec<bool> = list
        .iter()
        .map(|s| {
            stmt_is_pinned(s, ctx.facts)
                || matches!(s, Stmt::Let { id, .. } if declaration_stays(ctx.facts, *id))
                || is_shared_seed(ctx, s)
        })
        .collect();
    if report_enabled() {
        let mut why: std::collections::BTreeMap<&'static str, (usize, usize)> = Default::default();
        for (k, s) in list.iter().enumerate() {
            if !pinned[k] {
                continue;
            }
            let r = pin_reason(s, ctx.facts).unwrap_or("tdz-declaration");
            let e = why.entry(r).or_default();
            e.0 += 1;
            e.1 += weights[k];
        }
        for (r, (n, w)) in why {
            sh.report.push(format!(
                "perry-split\tpinned\t{}\t{}\tstmts={}\tnodes={}",
                sanitize_tsv(&ctx.parent),
                r,
                n,
                w
            ));
        }
    }
    // Values live across each gap (gap `i` is before statement `i`): the
    // declarations of this list made before the gap and used after it. Cuts
    // go where this is smallest.
    let across = live_across(list);
    let mut chunks: Vec<(usize, usize)> = Vec::new();
    let mut i = 0;
    while i < list.len() {
        if pinned[i] {
            i += 1;
            continue;
        }
        let start = i;
        let mut acc = 0usize;
        let mut best: Option<(usize, usize)> = None; // (live, gap)
        while i < list.len() && !pinned[i] {
            acc += weights[i];
            i += 1;
            if acc >= target / 2 {
                let live = across[i];
                if best.is_none_or(|(b, _)| live < b) {
                    best = Some((live, i));
                }
            }
            if acc >= target + target / 2 {
                break;
            }
        }
        let end = match best {
            Some((_, gap)) if i < list.len() && !pinned[i] => gap,
            _ => i,
        };
        chunks.push((start, end));
        i = end;
    }
    // Fold a small tail into the chunk before it when they touch.
    let mut merged: Vec<(usize, usize)> = Vec::new();
    for c in chunks {
        let w: usize = weights[c.0..c.1].iter().sum();
        if let Some(last) = merged.last_mut() {
            let lw: usize = weights[last.0..last.1].iter().sum();
            if last.1 == c.0 && w < target / 4 && lw + w < target + target / 2 {
                last.1 = c.1;
                continue;
            }
        }
        merged.push(c);
    }
    // Right to left, so indices before a chunk stay valid.
    for (a, b) in merged.into_iter().rev() {
        let replacement = extract_or_halve(list, &weights, a, b, ctx, sh, dominating, proofs);
        list.splice(a..b, replacement);
    }
}

fn names_of(params: &[Param], body: &[Stmt]) -> HashMap<u32, String> {
    let mut names = HashMap::new();
    for p in params {
        names.insert(p.id, p.name.clone());
    }
    visit_stmts(
        body,
        true,
        &mut |s| match s {
            Stmt::Let { id, name, .. } => {
                names.insert(*id, name.clone());
            }
            Stmt::Try { catch: Some(c), .. } => {
                if let Some((id, n)) = &c.param {
                    names.insert(*id, n.clone());
                }
            }
            _ => {}
        },
        &mut |_| {},
    );
    names
}

/// Sink each hoisted function declaration of `body` (a top-level `let f =
/// function ...` that nothing rebinds and no earlier code can reach) down to
/// just before the first statement that refers to it. Creating a closure has
/// no effect of its own, and every reference to `f` (direct or from a closure
/// created earlier) is in a statement at or after the new position, so
/// nothing observes the move; a region that uses `f` can then own it.
fn sink_hoisted_functions(body: &mut Vec<Stmt>, facts: &ModuleFacts) {
    let is_candidate = |s: &Stmt| -> Option<u32> {
        match s {
            Stmt::Let {
                id,
                init: Some(Expr::Closure { .. }),
                ..
            } if !facts.writes.contains(id) && !facts.prealloc.contains(id) => Some(*id),
            _ => None,
        }
    };
    let mut entries: Vec<(Stmt, HashSet<u32>)> = std::mem::take(body)
        .into_iter()
        .map(|s| {
            let r = ref_set(std::slice::from_ref(&s));
            (s, r)
        })
        .collect();
    // Last first, so a later declaration that refers to an earlier one has
    // already reached its own place.
    let mut i = entries.len();
    while i > 0 {
        i -= 1;
        let Some(id) = is_candidate(&entries[i].0) else {
            continue;
        };
        let Some(first_use) = (i + 1..entries.len()).find(|&j| entries[j].1.contains(&id)) else {
            continue;
        };
        if first_use > i + 1 {
            let e = entries.remove(i);
            entries.insert(first_use - 1, e);
        }
    }
    *body = entries.into_iter().map(|(s, _)| s).collect();
}

/// Split one body (a function's or a closure's) when it is large enough.
#[allow(clippy::too_many_arguments)]
fn split_body(
    body: &mut Vec<Stmt>,
    params: &[Param],
    captures: &[u32],
    parent: &str,
    is_strict: bool,
    is_async: bool,
    is_generator: bool,
    facts: &ModuleFacts,
    sh: &mut Shared<'_>,
) {
    if is_async
        || is_generator
        || params
            .iter()
            .any(|p| p.arguments_object.is_some() || p.is_rest && p.name == "arguments")
    {
        return;
    }
    let nodes = crate::collectors::count_body_nodes(body);
    if nodes <= facts.limits.min_body {
        return;
    }
    let dump = std::env::var("PERRY_SPLIT_DUMP")
        .ok()
        .is_some_and(|n| n == parent);
    if dump {
        eprintln!("perry-split dump BEFORE {parent}: {body:#?}");
    }
    // The sinking only serves a region that can then own a declaration: a
    // body that ends up with no region keeps its original order (moving a
    // closure's creation is not free on its own).
    let unsunk = facts.limits.sink.then(|| body.clone());
    if facts.limits.sink {
        sink_hoisted_functions(body, facts);
    }
    let declared = declared_ids(body, false);
    let captured = closure_captured_ids(body);
    let boxed: HashSet<u32> = captured
        .iter()
        .filter(|id| facts.writes.contains(id))
        .copied()
        .chain(facts.prealloc.iter().copied())
        .collect();
    let tl = crate::collectors::typed_locals::collect_typed_locals(
        body,
        &HashMap::new(),
        &boxed,
        &HashMap::new(),
    );
    let mut typed: HashMap<u32, Type> = HashMap::new();
    typed.extend(tl.strings.iter().map(|id| (*id, Type::String)));
    typed.extend(tl.numbers.iter().map(|id| (*id, Type::Number)));
    typed.extend(
        tl.arrays
            .iter()
            .map(|id| (*id, Type::Array(Box::new(Type::Any)))),
    );
    let mut counts = HashMap::new();
    ref_counts(body, &mut counts, 1);
    let single_init: HashSet<u32> = {
        let lc = let_counts(body);
        let mut with_init = HashSet::new();
        visit_stmts(
            body,
            false,
            &mut |s| {
                if let Stmt::Let {
                    id, init: Some(_), ..
                } = s
                {
                    with_init.insert(*id);
                }
            },
            &mut |_| {},
        );
        with_init
            .into_iter()
            .filter(|id| lc.get(id) == Some(&1))
            .collect()
    };
    let mut ctx = BodyCtx {
        facts,
        parent: parent.to_string(),
        is_strict,
        params: params.iter().map(|p| p.id).collect(),
        captures: captures.iter().copied().collect(),
        declared,
        captured,
        typed,
        names: names_of(params, body),
        counts,
        decl_counts: let_counts(body),
        single_init,
    };
    let before = sh.out.len() + sh.closure_names.len();
    let dominating: HashSet<u32> = ctx
        .params
        .iter()
        .chain(ctx.captures.iter())
        .copied()
        .collect();
    split_list(body, &mut ctx, sh, &dominating, &HashMap::new());
    if sh.out.len() + sh.closure_names.len() == before {
        if let Some(original) = unsunk {
            *body = original;
        }
    }
    if dump {
        eprintln!("perry-split dump AFTER {parent}: {body:#?}");
    }
    if report_enabled() {
        sh.report.push(format!(
            "perry-split\tbody\t{}\tnodes={}\tregions={}\tafter={}",
            sanitize_tsv(parent),
            nodes,
            sh.out.len() + sh.closure_names.len() - before,
            crate::collectors::count_body_nodes(body)
        ));
    }
}

/// Closures nested in `stmts`, innermost first, then the body itself.
fn split_closures_in(
    stmts: &mut [Stmt],
    names: &HashMap<u32, String>,
    facts: &ModuleFacts,
    sh: &mut Shared<'_>,
) {
    fn in_expr(
        e: &mut Expr,
        names: &HashMap<u32, String>,
        facts: &ModuleFacts,
        sh: &mut Shared<'_>,
    ) {
        if let Expr::Closure {
            func_id,
            params,
            body,
            captures,
            is_async,
            is_generator,
            is_strict,
            ..
        } = e
        {
            split_closures_in(body, names, facts, sh);
            let parent = names
                .get(func_id)
                .filter(|n| !n.is_empty())
                .cloned()
                .unwrap_or_else(|| format!("closure{func_id}"));
            let caps = captures.clone();
            split_body(
                body,
                params,
                &caps,
                &parent,
                *is_strict,
                *is_async,
                *is_generator,
                facts,
                sh,
            );
            return;
        }
        perry_hir::walker::walk_expr_children_mut(e, &mut |c| in_expr(c, names, facts, sh));
    }
    fn in_stmt(
        s: &mut Stmt,
        names: &HashMap<u32, String>,
        facts: &ModuleFacts,
        sh: &mut Shared<'_>,
    ) {
        match s {
            Stmt::Let { init: Some(e), .. }
            | Stmt::Expr(e)
            | Stmt::Throw(e)
            | Stmt::Return(Some(e)) => in_expr(e, names, facts, sh),
            Stmt::If {
                condition,
                then_branch,
                else_branch,
            } => {
                in_expr(condition, names, facts, sh);
                split_closures_in(then_branch, names, facts, sh);
                if let Some(e) = else_branch {
                    split_closures_in(e, names, facts, sh);
                }
            }
            Stmt::While { condition, body } | Stmt::DoWhile { condition, body } => {
                in_expr(condition, names, facts, sh);
                split_closures_in(body, names, facts, sh);
            }
            Stmt::For {
                init,
                condition,
                update,
                body,
            } => {
                if let Some(i) = init {
                    in_stmt(i, names, facts, sh);
                }
                if let Some(c) = condition {
                    in_expr(c, names, facts, sh);
                }
                if let Some(u) = update {
                    in_expr(u, names, facts, sh);
                }
                split_closures_in(body, names, facts, sh);
            }
            Stmt::Labeled { body, .. } => in_stmt(body, names, facts, sh),
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                split_closures_in(body, names, facts, sh);
                if let Some(c) = catch {
                    split_closures_in(&mut c.body, names, facts, sh);
                }
                if let Some(f) = finally {
                    split_closures_in(f, names, facts, sh);
                }
            }
            Stmt::Switch {
                discriminant,
                cases,
            } => {
                in_expr(discriminant, names, facts, sh);
                for c in cases {
                    if let Some(t) = &mut c.test {
                        in_expr(t, names, facts, sh);
                    }
                    split_closures_in(&mut c.body, names, facts, sh);
                }
            }
            _ => {}
        }
    }
    for s in stmts.iter_mut() {
        in_stmt(s, names, facts, sh);
    }
}

/// Split every oversized function and closure body of `hir`. Returns the
/// number of regions made.
pub fn split_large_bodies(hir: &mut HirModule) -> usize {
    if !enabled() {
        return 0;
    }
    split_large_bodies_with(hir, Limits::from_env())
}

/// [`split_large_bodies`] under explicit limits.
pub(crate) fn split_large_bodies_with(hir: &mut HirModule, limits: Limits) -> usize {
    // Cheap exit: nothing anywhere is big enough.
    let min = limits.min_body;
    let any_big = {
        let mut big = false;
        for body in all_module_bodies(hir) {
            if big {
                break;
            }
            if crate::collectors::count_body_nodes(body) > min {
                big = true;
                break;
            }
            visit_stmts(body, true, &mut |_| {}, &mut |e| {
                if !big {
                    if let Expr::Closure { body, .. } = e {
                        big |= crate::collectors::count_body_nodes(body) > min;
                    }
                }
            });
        }
        big
    };
    if !any_big {
        return 0;
    }
    let facts = module_facts(hir, limits);
    let next_func = super::entry_outline::max_func_id(hir);
    let next_local = max_local_id(hir);
    let names = hir.closure_display_names.clone();
    let mut init = std::mem::take(&mut hir.init);
    let mut functions = std::mem::take(&mut hir.functions);
    let mut spans = std::mem::take(&mut hir.local_source_spans);
    let mut sh = Shared {
        next_func,
        next_local,
        region_seq: 0,
        out: Vec::new(),
        spans: &mut spans,
        report: Vec::new(),
        closure_names: Vec::new(),
    };
    split_closures_in(&mut init, &names, &facts, &mut sh);
    for f in functions.iter_mut() {
        split_closures_in(&mut f.body, &names, &facts, &mut sh);
        let params = f.params.clone();
        let caps = f.captures.clone();
        let name = f.name.clone();
        split_body(
            &mut f.body,
            &params,
            &caps,
            &name,
            f.is_strict,
            f.is_async || f.was_plain_async,
            f.is_generator,
            &facts,
            &mut sh,
        );
    }
    let made = sh.out.len() + sh.closure_names.len();
    let report = std::mem::take(&mut sh.report);
    let closure_names = std::mem::take(&mut sh.closure_names);
    functions.extend(std::mem::take(&mut sh.out));
    drop(sh);
    hir.init = init;
    hir.functions = functions;
    hir.local_source_spans = spans;
    for (id, name) in closure_names {
        hir.closure_display_names.insert(id, name);
    }
    if report_enabled() {
        for line in report {
            eprintln!("{}\t{}", line, hir.name);
        }
    }
    made
}
