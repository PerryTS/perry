//! Module-entry outlining — analysis + gate (#8595, first increment).
//!
//! The module top level is lowered into a single LLVM function (`@main` /
//! `perry_module_init`). For a large minified bundle that one function is
//! enormous (the Claude Code `cli.js` entry is ~68 MB of IR, ~13,170 GC-root
//! slots), which is simultaneously pathological for `rewrite-statepoints-for-gc`
//! (relocation fan-out, #8583), instruction selection (#4880), and register
//! allocation. The fix is to outline the entry body into many small functions.
//!
//! This module is the **analysis half only** — it computes how the entry body
//! WOULD chunk and which top-level `let`s cross a chunk boundary (and therefore
//! must be globalized so the chunks can share them), and reports it. It does
//! **not** transform anything yet: the transform is the correctness-critical
//! part (eval order, TDZ, hoisting, top-level await) and lands separately once
//! it can be validated end-to-end. The reusable pieces here — the chunk
//! boundary rule and the cross-chunk reference set — are exactly what that
//! transform will consume to decide chunk boundaries and drive globalization.
//!
//! Nothing here changes codegen output. `PERRY_OUTLINE_ENTRY_REPORT=1` prints
//! the analysis; the transform gate `PERRY_OUTLINE_ENTRY` exists but is inert
//! until the transform lands.

use std::collections::HashSet;

use perry_hir::Module as HirModule;

use crate::collectors::{collect_let_ids, collect_ref_ids_in_stmts};

/// Default target number of top-level statements per outlined chunk. Chosen so
/// a chunk's live-root × safepoint product stays well under the RS4GC fan-out
/// regime (#8583); tuned with the transform, so it is only a reporting knob
/// today. Overridable with `PERRY_OUTLINE_ENTRY_CHUNK_STMTS`.
const DEFAULT_CHUNK_STMTS: usize = 200;

fn target_chunk_stmts() -> usize {
    std::env::var("PERRY_OUTLINE_ENTRY_CHUNK_STMTS")
        .ok()
        .and_then(|v| v.trim().parse::<usize>().ok())
        .filter(|&n| n > 0)
        .unwrap_or(DEFAULT_CHUNK_STMTS)
}

/// Whether the entry-outlining TRANSFORM is enabled. Inert in this increment
/// (no transform exists yet); present so the transform can gate on it without a
/// second flag churn. `PERRY_OUTLINE_ENTRY=1`/`on`/`true` turns it on.
pub(crate) fn entry_outlining_enabled() -> bool {
    matches!(
        std::env::var("PERRY_OUTLINE_ENTRY").as_deref(),
        Ok("1") | Ok("on") | Ok("true")
    )
}

fn report_requested() -> bool {
    matches!(
        std::env::var("PERRY_OUTLINE_ENTRY_REPORT").as_deref(),
        Ok("1") | Ok("on") | Ok("true")
    )
}

/// Result of analysing whether/how a module entry body would outline.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct EntryOutlineAnalysis {
    /// Number of top-level statements in `hir.init`.
    pub total_stmts: usize,
    /// Number of chunks the body would split into at the current target.
    pub chunk_count: usize,
    /// Top-level `let`s defined in one chunk and referenced from another —
    /// the bindings the transform must globalize so chunks share state. (The
    /// existing `emit_module_globals` escape rule already globalizes any let
    /// referenced from a separate function body, so once chunks are functions
    /// these are globalized for free; this counts them for reporting.)
    pub cross_chunk_lets: usize,
    /// `Some(reason)` if the transform would decline to outline this body even
    /// when enabled — the body is not a safe candidate.
    pub gated_out: Option<&'static str>,
}

impl EntryOutlineAnalysis {
    /// Whether this body is a candidate the transform would act on (large
    /// enough to be worth splitting and not gated out).
    pub fn is_candidate(&self) -> bool {
        self.gated_out.is_none() && self.chunk_count > 1
    }
}

/// Chunk the top-level statement list into contiguous ranges of
/// `target`-ish statements. Boundaries fall ONLY between top-level statements,
/// never inside a compound statement, so a top-level `if`/`for`/`try` (and all
/// its control flow) stays wholly within one chunk. Returns the half-open
/// `[start, end)` index ranges.
fn chunk_ranges(total: usize, target: usize) -> Vec<(usize, usize)> {
    if total == 0 {
        return Vec::new();
    }
    let mut ranges = Vec::new();
    let mut start = 0;
    while start < total {
        let end = (start + target).min(total);
        ranges.push((start, end));
        start = end;
    }
    ranges
}

/// Analyse the entry body of `hir` for outlining, using the env-configured
/// chunk target.
pub(crate) fn analyze_entry_outlining(hir: &HirModule) -> EntryOutlineAnalysis {
    analyze_entry_outlining_with_target(hir, target_chunk_stmts())
}

/// Pure analysis for an explicit chunk target — the testable core (no env).
fn analyze_entry_outlining_with_target(hir: &HirModule, target: usize) -> EntryOutlineAnalysis {
    let stmts = &hir.init;
    let total_stmts = stmts.len();
    let ranges = chunk_ranges(total_stmts, target);
    let chunk_count = ranges.len();

    // A top-level await splits the init across an async suspension; chunking
    // across it is a distinct, harder transform, so such bodies are gated out
    // initially. (Other gates — Script-scope `this`, generators — are added
    // alongside the transform that needs them.)
    let gated_out = if hir.has_top_level_await {
        Some("top-level await")
    } else {
        None
    };

    // Cross-chunk lets: for each chunk collect the `let`s it DEFINES and the
    // ids it REFERENCES (same collectors `emit_module_globals` uses). A let is
    // cross-chunk if any chunk other than its definer references it. Only
    // counted when there is more than one chunk — with one chunk nothing
    // crosses.
    let cross_chunk_lets = if chunk_count > 1 {
        let mut defs: Vec<HashSet<u32>> = Vec::with_capacity(chunk_count);
        let mut refs: Vec<HashSet<u32>> = Vec::with_capacity(chunk_count);
        for &(start, end) in &ranges {
            let slice = &stmts[start..end];
            let mut d = HashSet::new();
            collect_let_ids(slice, &mut d);
            defs.push(d);
            let mut r = HashSet::new();
            collect_ref_ids_in_stmts(slice, &mut r);
            refs.push(r);
        }
        let mut crossing: HashSet<u32> = HashSet::new();
        for (ci, d) in defs.iter().enumerate() {
            for &id in d {
                let referenced_elsewhere = refs
                    .iter()
                    .enumerate()
                    .any(|(ri, r)| ri != ci && r.contains(&id));
                if referenced_elsewhere {
                    crossing.insert(id);
                }
            }
        }
        crossing.len()
    } else {
        0
    };

    EntryOutlineAnalysis {
        total_stmts,
        chunk_count,
        cross_chunk_lets,
        gated_out,
    }
}

/// Print the analysis when `PERRY_OUTLINE_ENTRY_REPORT` is set. No effect on
/// codegen. Called once per module from `compile_module`.
pub(crate) fn report_entry_outlining(hir: &HirModule) {
    if !report_requested() {
        return;
    }
    let a = analyze_entry_outlining(hir);
    // When the transform is enabled it runs earlier (in the HIR phase, see
    // `outline_entry_module`), so by the time codegen calls this the body is
    // already rewritten — the numbers below then describe the post-transform
    // init (hoisted declarations + chunk calls). With the transform off, they
    // describe the original body, which is the useful measurement.
    let transform = if entry_outlining_enabled() {
        " (PERRY_OUTLINE_ENTRY set — transform already applied; figures are post-transform)"
    } else {
        ""
    };
    match a.gated_out {
        Some(reason) => eprintln!(
            "[perry] entry-outline: {}: {} top-level stmts; NOT a candidate ({}){}",
            hir.name, a.total_stmts, reason, transform
        ),
        None => eprintln!(
            "[perry] entry-outline: {}: {} top-level stmts → {} chunk(s) of ~{}, {} cross-chunk let(s) to globalize; candidate={}{}",
            hir.name,
            a.total_stmts,
            a.chunk_count,
            target_chunk_stmts(),
            a.cross_chunk_lets,
            a.is_candidate(),
            transform
        ),
    }
}

/// Outcome of attempting to outline a module entry body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OutlineOutcome {
    /// Outlined into `chunks` chunk functions.
    Outlined { chunks: usize },
    /// Left unchanged; `&'static str` says why (fail-safe fallback).
    Skipped(&'static str),
}

/// Largest `FuncId` used anywhere in `hir` — over `functions`,
/// `script_global_functions`, `exported_functions`, and every nested closure
/// `func_id` in a top-level or function body. New chunk ids are minted strictly
/// above this so they can never collide with an existing function or closure.
fn max_func_id(hir: &HirModule) -> u32 {
    let mut max = 0u32;
    for f in &hir.functions {
        max = max.max(f.id);
    }
    for (_, id) in &hir.script_global_functions {
        max = max.max(*id);
    }
    for (_, id) in &hir.exported_functions {
        max = max.max(*id);
    }
    // Nested closures carry their own `func_id`; a new chunk id must clear
    // those too. `collect_closures_in_stmts` walks stmts + exprs and yields
    // every closure id — run it over the init body and every function body.
    let collect_max_closure = |stmts: &[perry_hir::Stmt], max: &mut u32| {
        let mut seen = std::collections::HashSet::new();
        let mut out: Vec<(perry_hir::types::FuncId, perry_hir::Expr)> = Vec::new();
        crate::collectors::collect_closures_in_stmts(stmts, &mut seen, &mut out);
        for (id, _) in out {
            *max = (*max).max(id);
        }
    };
    collect_max_closure(&hir.init, &mut max);
    for f in &hir.functions {
        collect_max_closure(&f.body, &mut max);
    }
    max
}

/// A top-level statement the transform can safely relocate into a chunk
/// function without changing semantics or breaking an `hir.init` scan.
///
/// Deliberately narrow for this first increment: a plain expression statement,
/// or a `let`/`const` binding a SINGLE local to an initializer (split into a
/// hoisted bare declaration plus a `LocalSet` in the chunk). Anything else —
/// destructuring, `var`, top-level control flow, class/enum/import decls — makes
/// the whole body ineligible (the transform bails and the entry compiles
/// unchanged). Extending this set (and the `hir.init` scans that must follow
/// statements into chunks) is the follow-up that reaches real bundles.
fn classify_top_level(stmt: &perry_hir::Stmt) -> Option<TopLevelKind> {
    use perry_hir::Stmt;
    match stmt {
        Stmt::Expr(_) => Some(TopLevelKind::Expr),
        Stmt::Let {
            id, init: Some(_), ..
        } => Some(TopLevelKind::SimpleLet(*id)),
        Stmt::Let { init: None, .. } => Some(TopLevelKind::BareLet),
        _ => None,
    }
}

enum TopLevelKind {
    Expr,
    SimpleLet(u32),
    BareLet,
}

/// Module features whose codegen scans read `hir.init` directly and would
/// therefore miss statements relocated into chunks. Until each such scan is
/// taught to follow chunk calls, a module with any of them is ineligible.
fn has_init_scan_coupling(hir: &HirModule) -> Option<&'static str> {
    if !hir.exports.is_empty() || !hir.exported_functions.is_empty() {
        return Some("module has exports");
    }
    if !hir.script_global_functions.is_empty() {
        return Some("script-global function hoisting");
    }
    if hir.references_global_this {
        return Some("references globalThis");
    }
    None
}

/// How many chunk functions the interleaving would emit for `stmts` at
/// `target` — a run of relocatable statements becomes ceil(run/target) chunks,
/// and a must-stay statement (an unclassifiable shape) ends the current run.
/// Used as a pre-scan so eligibility is decided before any mutation.
fn count_prospective_chunks(stmts: &[perry_hir::Stmt], target: usize) -> usize {
    let mut chunks = 0usize;
    let mut run = 0usize;
    let flush = |run: &mut usize, chunks: &mut usize| {
        if *run > 0 {
            *chunks += run.div_ceil(target.max(1));
            *run = 0;
        }
    };
    for stmt in stmts {
        match classify_top_level(stmt) {
            // A bare declaration is hoisted, not executed in a chunk.
            Some(TopLevelKind::BareLet) => {}
            Some(TopLevelKind::Expr) | Some(TopLevelKind::SimpleLet(_)) => run += 1,
            None => flush(&mut run, &mut chunks),
        }
    }
    flush(&mut run, &mut chunks);
    chunks
}

/// Attempt to outline `hir`'s entry body (#8595). Fail-safe: returns
/// `Skipped(reason)` and leaves `hir` untouched unless the whole body is
/// provably safe to relocate; callers proceed with the ordinary single-function
/// entry lowering in that case. Only runs when `PERRY_OUTLINE_ENTRY` is set.
pub fn outline_entry_module(hir: &mut HirModule) -> OutlineOutcome {
    if !entry_outlining_enabled() {
        return OutlineOutcome::Skipped("PERRY_OUTLINE_ENTRY not set");
    }
    outline_entry_module_with_target(hir, target_chunk_stmts())
}

/// Env-free core of [`outline_entry_module`] — the testable seam.
fn outline_entry_module_with_target(hir: &mut HirModule, target: usize) -> OutlineOutcome {
    let analysis = analyze_entry_outlining_with_target(hir, target);
    if let Some(reason) = analysis.gated_out {
        return OutlineOutcome::Skipped(reason);
    }
    if !analysis.is_candidate() {
        return OutlineOutcome::Skipped("not a candidate (too small)");
    }
    // Coupling bail: some codegen scans read `hir.init` directly and would
    // miss statements moved into chunks. Until each is taught to follow chunk
    // calls, a module with one is ineligible. (Empirically, outlining exports /
    // globalThis / process.env-literals produces correct output on toy entries,
    // so these are candidates for relaxation once validated against the gap
    // suite — see #8595.)
    if let Some(reason) = has_init_scan_coupling(hir) {
        return OutlineOutcome::Skipped(reason);
    }

    // Pre-scan: decide eligibility before mutating. Outlining is worthwhile
    // only if the interleaving would emit more than one chunk.
    if count_prospective_chunks(&hir.init, target) <= 1 {
        return OutlineOutcome::Skipped("would not split into multiple chunks");
    }

    let mut next_id = max_func_id(hir) + 1;
    let module_name = hir.name.clone();
    let original = std::mem::take(&mut hir.init);

    // Hoisted bare declarations go to the FRONT of the new init so
    // `emit_module_globals` still sees them as top-level `let`s and globalizes
    // exactly those referenced across chunks (its existing escape rule).
    let mut hoisted: Vec<perry_hir::Stmt> = Vec::new();
    // The rewritten body: chunk calls interleaved with any statement that had
    // to stay inline, in original execution order.
    let mut new_body: Vec<perry_hir::Stmt> = Vec::new();
    let mut chunk_fns: Vec<perry_hir::Function> = Vec::new();
    // The current run of relocatable statements accumulating into a chunk.
    let mut run: Vec<perry_hir::Stmt> = Vec::new();

    // Emit the accumulated run as a chunk function and append its call, unless
    // empty. `flush` is a closure over the mutable state via explicit params to
    // keep the borrow checker happy.
    fn flush(
        run: &mut Vec<perry_hir::Stmt>,
        chunk_fns: &mut Vec<perry_hir::Function>,
        new_body: &mut Vec<perry_hir::Stmt>,
        next_id: &mut u32,
        module_name: &str,
    ) {
        if run.is_empty() {
            return;
        }
        let fn_id = *next_id;
        *next_id += 1;
        let ci = chunk_fns.len();
        chunk_fns.push(perry_hir::Function {
            id: fn_id,
            name: format!("__perry_entry_chunk_{module_name}_{ci}"),
            type_params: Vec::new(),
            params: Vec::new(),
            return_type: perry_hir::types::Type::Void,
            body: std::mem::take(run),
            is_async: false,
            is_generator: false,
            is_strict: true,
            is_exported: false,
            captures: Vec::new(),
            decorators: Vec::new(),
            was_plain_async: false,
            was_unrolled: false,
        });
        new_body.push(perry_hir::Stmt::Expr(perry_hir::Expr::Call {
            callee: Box::new(perry_hir::Expr::FuncRef(fn_id)),
            args: Vec::new(),
            type_args: Vec::new(),
            byte_offset: 0,
        }));
    }

    for stmt in original {
        match classify_top_level(&stmt) {
            Some(TopLevelKind::Expr) | Some(TopLevelKind::BareLet) => {
                if let perry_hir::Stmt::Let { .. } = &stmt {
                    // A bare `let x;` is a pure declaration — hoist it (so it is
                    // globalized) and add nothing executable to the run.
                    hoisted.push(stmt);
                } else {
                    run.push(stmt);
                }
            }
            Some(TopLevelKind::SimpleLet(id)) => {
                if let perry_hir::Stmt::Let {
                    id: lid,
                    name,
                    ty,
                    mutable,
                    init: Some(init),
                } = stmt
                {
                    hoisted.push(perry_hir::Stmt::Let {
                        id: lid,
                        name,
                        ty,
                        mutable,
                        init: None,
                    });
                    run.push(perry_hir::Stmt::Expr(perry_hir::Expr::LocalSet(
                        id,
                        Box::new(init),
                    )));
                } else {
                    unreachable!("SimpleLet classification implies Let with init");
                }
            }
            None => {
                // A statement we cannot safely relocate (control flow, etc.):
                // end the current chunk run and keep this statement inline, at
                // its original position, so eval order and any `hir.init` scan
                // that reads it are preserved.
                flush(
                    &mut run,
                    &mut chunk_fns,
                    &mut new_body,
                    &mut next_id,
                    &module_name,
                );
                new_body.push(stmt);
            }
        }
        if run.len() >= target {
            flush(
                &mut run,
                &mut chunk_fns,
                &mut new_body,
                &mut next_id,
                &module_name,
            );
        }
    }
    flush(
        &mut run,
        &mut chunk_fns,
        &mut new_body,
        &mut next_id,
        &module_name,
    );

    let chunks = chunk_fns.len();
    hir.functions.extend(chunk_fns);
    let mut rebuilt = hoisted;
    rebuilt.extend(new_body);
    hir.init = rebuilt;
    OutlineOutcome::Outlined { chunks }
}

#[cfg(test)]
mod tests {
    use super::*;
    use perry_hir::types::Type;
    use perry_hir::{Expr, Module, Stmt};

    fn let_stmt(id: u32, name: &str, init: Expr) -> Stmt {
        Stmt::Let {
            id,
            name: name.to_string(),
            ty: Type::Any,
            mutable: false,
            init: Some(init),
        }
    }

    fn empty_closure_with_id(func_id: u32, body: Vec<Stmt>) -> Expr {
        Expr::Closure {
            func_id,
            params: vec![],
            return_type: Type::Any,
            body,
            captures: vec![],
            mutable_captures: vec![],
            captures_this: false,
            captures_new_target: false,
            enclosing_class: None,
            is_arrow: false,
            is_async: false,
            is_generator: false,
            is_strict: false,
        }
    }

    fn module_with_init(init: Vec<Stmt>) -> Module {
        let mut m = Module::new("test_mod");
        m.init = init;
        m
    }

    #[test]
    fn chunk_ranges_split_contiguously() {
        assert_eq!(chunk_ranges(0, 3), Vec::<(usize, usize)>::new());
        assert_eq!(chunk_ranges(3, 3), vec![(0, 3)]);
        assert_eq!(chunk_ranges(7, 3), vec![(0, 3), (3, 6), (6, 7)]);
    }

    #[test]
    fn small_body_is_a_single_chunk_and_not_a_candidate() {
        let m = module_with_init(vec![
            Stmt::Expr(Expr::Number(1.0)),
            Stmt::Expr(Expr::Number(2.0)),
        ]);
        let a = analyze_entry_outlining_with_target(&m, 200);
        assert_eq!(a.chunk_count, 1);
        assert_eq!(a.cross_chunk_lets, 0);
        assert!(!a.is_candidate());
    }

    #[test]
    fn cross_chunk_let_is_counted() {
        // chunk size 1: `let x = 1` in chunk 0, `x` read in chunk 1 -> crosses.
        let m = module_with_init(vec![
            let_stmt(0, "x", Expr::Number(1.0)),
            Stmt::Expr(Expr::LocalGet(0)),
        ]);
        let a = analyze_entry_outlining_with_target(&m, 1);
        assert_eq!(a.chunk_count, 2);
        assert_eq!(
            a.cross_chunk_lets, 1,
            "x is defined in chunk 0 and read in chunk 1"
        );
        assert!(a.is_candidate());
    }

    #[test]
    fn a_let_used_only_within_its_own_chunk_does_not_cross() {
        // Two lets, chunk size 2: both defined+used inside their chunk -> none cross.
        let m = module_with_init(vec![
            let_stmt(0, "x", Expr::Number(1.0)),
            Stmt::Expr(Expr::LocalGet(0)),
            let_stmt(1, "y", Expr::Number(2.0)),
            Stmt::Expr(Expr::LocalGet(1)),
        ]);
        let a = analyze_entry_outlining_with_target(&m, 2);
        assert_eq!(a.chunk_count, 2);
        assert_eq!(
            a.cross_chunk_lets, 0,
            "x and y are each confined to their own chunk"
        );
    }

    #[test]
    fn top_level_await_gates_the_body_out() {
        let mut m = module_with_init(vec![
            let_stmt(0, "x", Expr::Number(1.0)),
            Stmt::Expr(Expr::LocalGet(0)),
        ]);
        m.has_top_level_await = true;
        let a = analyze_entry_outlining_with_target(&m, 1);
        assert_eq!(a.gated_out, Some("top-level await"));
        assert!(!a.is_candidate(), "a gated-out body is never a candidate");
    }
    #[test]
    fn transform_splits_lets_and_emits_ordered_chunk_calls() {
        // let x = 1 (chunk 0); read x + let y = 2 (chunk 1); read y (chunk 2)
        let mut m = module_with_init(vec![
            let_stmt(0, "x", Expr::Number(1.0)),
            Stmt::Expr(Expr::LocalGet(0)),
            let_stmt(1, "y", Expr::Number(2.0)),
            Stmt::Expr(Expr::LocalGet(1)),
        ]);
        let before_fns = m.functions.len();
        let outcome = outline_entry_module_with_target(&mut m, 2);
        assert_eq!(outcome, OutlineOutcome::Outlined { chunks: 2 });
        // two chunk functions added
        assert_eq!(m.functions.len(), before_fns + 2);
        // new init: hoisted bare decls for x and y, then two ordered chunk calls
        let bare_lets = m
            .init
            .iter()
            .filter(|s| matches!(s, Stmt::Let { init: None, .. }))
            .count();
        assert_eq!(bare_lets, 2, "both lets hoisted as bare declarations");
        let calls: Vec<u32> = m
            .init
            .iter()
            .filter_map(|s| match s {
                Stmt::Expr(Expr::Call { callee, .. }) => match callee.as_ref() {
                    Expr::FuncRef(id) => Some(*id),
                    _ => None,
                },
                _ => None,
            })
            .collect();
        assert_eq!(calls.len(), 2, "two ordered chunk calls");
        assert_eq!(
            calls[0], m.functions[before_fns].id,
            "call 0 targets chunk 0"
        );
        assert_eq!(
            calls[1],
            m.functions[before_fns + 1].id,
            "call 1 targets chunk 1"
        );
        // chunk 0 holds `x = 1` (a LocalSet), no bare let
        let chunk0 = &m.functions[before_fns].body;
        assert!(chunk0
            .iter()
            .any(|s| matches!(s, Stmt::Expr(Expr::LocalSet(0, _)))));
    }

    #[test]
    fn minted_chunk_ids_clear_existing_function_and_closure_ids() {
        let mut m = module_with_init(vec![
            let_stmt(0, "x", Expr::Number(1.0)),
            Stmt::Expr(Expr::LocalGet(0)),
        ]);
        // an existing function with a high id, and a closure with an even higher id
        m.functions.push(perry_hir::Function {
            id: 500,
            name: "f".into(),
            type_params: vec![],
            params: vec![],
            return_type: Type::Void,
            body: vec![Stmt::Expr(empty_closure_with_id(9000, vec![]))],
            is_async: false,
            is_generator: false,
            is_strict: true,
            is_exported: false,
            captures: vec![],
            decorators: vec![],
            was_plain_async: false,
            was_unrolled: false,
        });
        let base = m.functions.len();
        let outcome = outline_entry_module_with_target(&mut m, 1);
        assert!(matches!(outcome, OutlineOutcome::Outlined { .. }));
        for f in &m.functions[base..] {
            assert!(
                f.id > 9000,
                "chunk id {} must clear the closure id 9000",
                f.id
            );
        }
    }

    #[test]
    fn transform_interleaves_chunks_around_a_must_stay_statement() {
        // A top-level `if` cannot be relocated; the transform outlines the
        // relocatable runs on either side of it and keeps the `if` inline, in
        // order. target=1 so each relocatable statement is its own chunk.
        let mut m = module_with_init(vec![
            let_stmt(0, "x", Expr::Number(1.0)), // chunk
            Stmt::If {
                condition: Expr::Bool(true),
                then_branch: vec![Stmt::Expr(Expr::LocalGet(0))],
                else_branch: None,
            }, // must-stay, inline
            let_stmt(1, "y", Expr::Number(2.0)), // chunk
            Stmt::Expr(Expr::LocalGet(1)),       // chunk
        ]);
        let fns_before = m.functions.len();
        let outcome = outline_entry_module_with_target(&mut m, 1);
        assert!(
            matches!(outcome, OutlineOutcome::Outlined { .. }),
            "runs around the if are outlined, not bailed: {outcome:?}"
        );
        let if_pos = m
            .init
            .iter()
            .position(|s| matches!(s, Stmt::If { .. }))
            .expect("the top-level if is kept inline");
        let call_positions: Vec<usize> = m
            .init
            .iter()
            .enumerate()
            .filter_map(|(i, s)| match s {
                Stmt::Expr(Expr::Call { callee, .. })
                    if matches!(callee.as_ref(), Expr::FuncRef(_)) =>
                {
                    Some(i)
                }
                _ => None,
            })
            .collect();
        assert!(
            call_positions.iter().any(|&i| i < if_pos),
            "a chunk call precedes the if (the `x` run)"
        );
        assert!(
            call_positions.iter().any(|&i| i > if_pos),
            "a chunk call follows the if (the `y` run)"
        );
        assert!(
            m.functions.len() > fns_before + 1,
            "more than one chunk function emitted"
        );
    }

    #[test]
    fn transform_bails_when_the_module_has_exports() {
        let mut m = module_with_init(vec![
            let_stmt(0, "x", Expr::Number(1.0)),
            Stmt::Expr(Expr::LocalGet(0)),
        ]);
        m.exported_functions.push(("g".into(), 42));
        let outcome = outline_entry_module_with_target(&mut m, 1);
        assert_eq!(outcome, OutlineOutcome::Skipped("module has exports"));
    }
}
