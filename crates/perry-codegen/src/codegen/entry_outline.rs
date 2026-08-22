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
    let transform = if entry_outlining_enabled() {
        " (PERRY_OUTLINE_ENTRY set, but the transform is not implemented yet — analysis only)"
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
}
