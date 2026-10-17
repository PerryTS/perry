//! The region guard's shared pieces (#10884): the per-region state word and
//! its bounded attempt counter, the kill switch, and the census.
//!
//! A region is a single-entry run of accesses over which one receiver's
//! `(unmasked pointer, ShapeId)` pair is held, entered through ONE shape
//! compare, whose failure leaves for a generic copy of the whole run and never
//! rejoins it (design doc §L7.1–L7.3). The loop region (`stmt/region_loop`)
//! is the one remaining user. Slice 1 (a read run inside one `+` tree,
//! `expr/region_read_run`) was deleted with the read caches (object model
//! step 5, phase 2): its reads take the ordinary probe.
//!
//! # Supplier
//!
//! The expected ShapeId is learned (supplier (b), §L14.18.4): a per-region
//! atomic word primed on a miss by `js_region_guard_prime`. The id and every
//! key's slot live in ONE word so a concurrent prime can never pair one
//! shape's id with another's slots. This word is a per-site memo; it goes
//! when object model step 2c's slot agreement makes the loop region
//! unnecessary.

use std::cell::Cell;

use crate::expr::FnCtx;

/// Keys one word can address. Must equal
/// `perry_runtime::object::shapes::REGION_GUARD_MAX_KEYS`.
pub(crate) const MAX_KEYS: usize = 5;
/// `REGION_GUARD_WORD_EMPTY`: low half `u32::MAX`, never a live ShapeId.
const EMPTY_WORD: &str = "4294967295";

thread_local! {
    static REGIONS_EXPR: Cell<u64> = const { Cell::new(0) };
    static READS_EXPR: Cell<u64> = const { Cell::new(0) };
    static REGIONS_STMT: Cell<u64> = const { Cell::new(0) };
    static READS_STMT: Cell<u64> = const { Cell::new(0) };
}

/// `PERRY_REGION_READS=0` — the kill switch both slices honour, so an A/B can
/// switch the feature off in ONE compiler instead of comparing two builds
/// (§L7.6.2: two builds of the same program differ by ~1.2% on tsc).
pub(crate) fn disabled() -> bool {
    matches!(
        std::env::var("PERRY_REGION_READS").as_deref(),
        Ok("0") | Ok("off") | Ok("false")
    )
}

/// One region's state: the learned word and its bounded attempt counter.
pub(crate) struct Sites {
    pub(crate) word_g: String,
    pub(crate) tries_g: String,
}

pub(crate) fn state_globals(ctx: &mut FnCtx<'_>) -> Sites {
    let site = ctx.ic_site_counter;
    ctx.ic_site_counter += 1;
    let base = crate::expr::inline_cache_global_name(ctx, site);
    let word_g = format!("@{base}_region");
    let tries_g = format!("@{base}_region_tries");
    ctx.typed_parse_rodata.push(format!(
        "{word_g} = private global i64 {EMPTY_WORD}, align 8"
    ));
    ctx.typed_parse_rodata
        .push(format!("{tries_g} = private global i32 0, align 4"));
    Sites { word_g, tries_g }
}

/// `PERRY_REGION_DIAG=1`: per module, what each slice formed and how many
/// statement-level runs the static census found — so "what is still uncovered"
/// is a measured number rather than an estimate. Slice 1's PR reported 28
/// regions / 56 reads covered on a tsc compile against 88 runs / 205 reads
/// uncovered; slice 2 is aimed at the second pair.
pub(crate) struct ModuleDiag {
    census: Option<(u64, u64)>,
    name: String,
}

impl ModuleDiag {
    pub(crate) fn start(hir: &perry_hir::Module) -> Self {
        REGIONS_EXPR.with(|c| c.set(0));
        READS_EXPR.with(|c| c.set(0));
        REGIONS_STMT.with(|c| c.set(0));
        READS_STMT.with(|c| c.set(0));
        let on = std::env::var("PERRY_REGION_DIAG").ok().as_deref() == Some("1");
        ModuleDiag {
            census: on.then(|| statement_run_census(hir)),
            name: hir.name.clone(),
        }
    }
}

impl Drop for ModuleDiag {
    fn drop(&mut self) {
        if let Some((runs, reads)) = self.census {
            let l = crate::stmt::region_loop::take_stats();
            eprintln!(
                "[perry region] module={} loop_regions={} body_regions={} bare_reads={} bare_stores={} f_discarded={} loops_refused={}",
                self.name, l[0], l[1], l[2], l[3], l[4], l[5]
            );
            eprintln!(
                "[perry region] module={} regions={} reads_covered={} stmt_regions={} stmt_reads_covered={} statement_runs_seen={} statement_reads_seen={}",
                self.name,
                REGIONS_EXPR.with(|c| c.get()),
                READS_EXPR.with(|c| c.get()),
                REGIONS_STMT.with(|c| c.get()),
                READS_STMT.with(|c| c.get()),
                runs,
                reads
            );
        }
    }
}

/// Runs of two or more consecutive same-receiver reads bound across
/// STATEMENTS. This is the population slice 2 targets; the difference between
/// it and `stmt_regions` is what slice 2 still declines.
///
/// It walks CLOSURE bodies too. Leaving them out undercounted a real program
/// by 6.5x — a CJS bundle puts nearly all of its code inside the factory
/// closure, so tsc reported 88 candidate runs while the matcher was forming
/// 568 regions. A census that cannot see where the code lives is not a
/// census.
fn statement_run_census(hir: &perry_hir::Module) -> (u64, u64) {
    let mut acc = (0u64, 0u64);
    census_with_closures(&hir.init, &mut acc);
    for f in &hir.functions {
        census_with_closures(&f.body, &mut acc);
    }
    for c in &hir.classes {
        for m in c.methods.iter().chain(c.static_methods.iter()) {
            census_with_closures(&m.body, &mut acc);
        }
        if let Some(ctor) = &c.constructor {
            census_with_closures(&ctor.body, &mut acc);
        }
    }
    acc
}

/// A statement list and every closure body inside it. A CJS bundle keeps
/// nearly all of its code in the factory closure, so a census that stops at
/// the statement list undercounts it — tsc reported 88 candidate runs while
/// the matcher was forming 568 regions.
fn census_with_closures(stmts: &[perry_hir::Stmt], acc: &mut (u64, u64)) {
    census_stmts(stmts, acc);
    let mut seen = std::collections::HashSet::new();
    let mut closures = Vec::new();
    crate::collectors::collect_closures_in_stmts(stmts, &mut seen, &mut closures);
    for (_, expr) in &closures {
        if let perry_hir::Expr::Closure { body, .. } = expr {
            census_stmts(body, acc);
        }
    }
}

/// The receiver a `const x = r.k;` statement reads, if it is one.
pub(crate) fn let_read_receiver(stmt: &perry_hir::Stmt) -> Option<u32> {
    let perry_hir::Stmt::Let {
        init: Some(perry_hir::Expr::PropertyGet { object, .. }),
        ..
    } = stmt
    else {
        return None;
    };
    match object.as_ref() {
        perry_hir::Expr::LocalGet(id) => Some(*id),
        _ => None,
    }
}

fn census_stmts(stmts: &[perry_hir::Stmt], acc: &mut (u64, u64)) {
    use perry_hir::Stmt;
    let mut run_receiver: Option<u32> = None;
    let mut run_len = 0u64;
    let flush = |len: u64, acc: &mut (u64, u64)| {
        if len >= 2 {
            acc.0 += 1;
            acc.1 += len;
        }
    };
    for stmt in stmts {
        match let_read_receiver(stmt) {
            Some(r) if run_receiver == Some(r) => run_len += 1,
            Some(r) => {
                flush(run_len, acc);
                run_receiver = Some(r);
                run_len = 1;
            }
            None => {
                flush(run_len, acc);
                run_receiver = None;
                run_len = 0;
            }
        }
        match stmt {
            Stmt::If {
                then_branch,
                else_branch,
                ..
            } => {
                census_stmts(then_branch, acc);
                if let Some(eb) = else_branch {
                    census_stmts(eb, acc);
                }
            }
            Stmt::For { body, .. } | Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => {
                census_stmts(body, acc)
            }
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                census_stmts(body, acc);
                if let Some(c) = catch {
                    census_stmts(&c.body, acc);
                }
                if let Some(f) = finally {
                    census_stmts(f, acc);
                }
            }
            Stmt::Switch { cases, .. } => {
                for case in cases {
                    census_stmts(&case.body, acc);
                }
            }
            _ => {}
        }
    }
    flush(run_len, acc);
}
