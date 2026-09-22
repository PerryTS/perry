//! The region guard itself (#10884) — one entry, shared by every slice.
//!
//! A region is a single-entry run of accesses over which one receiver's
//! `(unmasked pointer, ShapeId)` pair is held, entered through ONE shape
//! compare, whose failure leaves for a generic copy of the whole run and never
//! rejoins it (design doc §L7.1–L7.3). What varies between slices is only
//! WHERE the run is found and what is done with the loaded values:
//!
//! * slice 1 (`crate::expr::region_read_run`) — a run inside one `+` tree;
//! * slice 2 (`crate::stmt::region_read_stmts`) — a run across statements.
//!
//! Everything else — the state word, R1, R2, the bounded prime, the miss
//! edges — lives here, so the two slices cannot drift into two guards with two
//! soundness arguments. The emitted sequence is:
//!
//! ```text
//! [R1] guard   tag test + unmask + ONE ShapeId compare      ─┐ the only two
//! [R2] load    every key's slot, from one atomic region word │ bail edges,
//!              (a slice may then verify, then use)          ─┘ before any effect
//! ```
//!
//! # Supplier
//!
//! The expected ShapeId is learned (supplier (b), §L14.18.4): a per-region
//! atomic word primed on a miss by `js_region_guard_prime`. The id and every
//! key's slot live in ONE word so a concurrent prime can never pair one
//! shape's id with another's slots. A link-time constant (step 4) would
//! replace the word load and nothing else.
//!
//! # The miss price, measured
//!
//! A region that never hits costs **~19 instructions per execution**: 17 for
//! this prologue (it must materialise the operands before the compare can
//! exist) and 3 for the retirement bookkeeping, which every miss pays even
//! after priming retires. Measured with the kill switch on an `Object.create`
//! receiver, whose fields spill and therefore can never pack a word:
//! `ocr2` 75 → 94, `ocr4` 450 → 470, against `lit4` (same source, literal
//! receiver) 216 → 53. Price a guard by everything that must execute to reach
//! its branch, not by the branch (§L7.6.3).

use std::cell::Cell;

use crate::expr::FnCtx;
use crate::nanbox::POINTER_MASK_I64;
use crate::types::{DOUBLE, I32, I64, PTR};

/// Keys one word can address. Must equal
/// `perry_runtime::object::shapes::REGION_GUARD_MAX_KEYS`.
pub(crate) const MAX_KEYS: usize = 5;
/// Must equal the runtime's slot width.
const SLOT_BITS: u32 = 6;
/// `REGION_GUARD_WORD_EMPTY`: low half `u32::MAX`, never a live ShapeId.
const EMPTY_WORD: &str = "4294967295";
/// Primes attempted per region before it stops trying (process lifetime).
const PRIME_ATTEMPTS: &str = "8";
/// A small-handle band sits under the pointer tag; its ids are not addresses.
const SMALL_HANDLE_MAX: &str = "1048575";

thread_local! {
    /// Non-zero while a generic copy is being lowered. The generic copy lowers
    /// the SAME code through the ordinary dispatch, which would otherwise form
    /// the same region again inside itself.
    static SUPPRESS: Cell<u32> = const { Cell::new(0) };
    static REGIONS_EXPR: Cell<u64> = const { Cell::new(0) };
    static READS_EXPR: Cell<u64> = const { Cell::new(0) };
    static REGIONS_STMT: Cell<u64> = const { Cell::new(0) };
    static READS_STMT: Cell<u64> = const { Cell::new(0) };
}

pub(crate) struct Suppressed;

impl Suppressed {
    pub(crate) fn enter() -> Self {
        SUPPRESS.with(|s| s.set(s.get() + 1));
        Suppressed
    }
}

impl Drop for Suppressed {
    fn drop(&mut self) {
        SUPPRESS.with(|s| s.set(s.get() - 1));
    }
}

pub(crate) fn suppressed() -> bool {
    SUPPRESS.with(|s| s.get()) > 0
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

/// A region is not formed while a profiling build is recording guard pass/fail
/// on the per-access towers: a read served before them would change a signal
/// that must stay byte-identical.
pub(crate) fn emission_allowed() -> bool {
    !suppressed() && !disabled() && !crate::expr::typed_feedback_emission_enabled()
}

pub(crate) fn note_expr_region(reads: u64) {
    REGIONS_EXPR.with(|c| c.set(c.get() + 1));
    READS_EXPR.with(|c| c.set(c.get() + reads));
}

pub(crate) fn note_stmt_region(reads: u64) {
    REGIONS_STMT.with(|c| c.set(c.get() + 1));
    READS_STMT.with(|c| c.set(c.get() + reads));
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

/// What R1 proved and what R2 needs: the unmasked receiver pointer (held in a
/// register for the whole region — reads 2..n re-pay none of the tag test),
/// the loaded word, and the receiver's own ShapeId for the prime path.
pub(crate) struct Entry {
    pub(crate) handle: String,
    pub(crate) word: String,
    pub(crate) sid: String,
}

/// R1, emitted from the CURRENT block. On a hit control reaches `hit_l`; a
/// shape mismatch goes to `miss_l`; a receiver that is not a heap object at
/// all goes straight to `generic_l`.
pub(crate) fn emit_r1(
    ctx: &mut FnCtx<'_>,
    recv: &str,
    sites: &Sites,
    hit_l: &str,
    miss_l: &str,
    generic_l: &str,
) -> Entry {
    let handle_idx = ctx.new_block("region.handle");
    let r1_idx = ctx.new_block("region.r1");
    let handle_l = ctx.block_label(handle_idx);
    let r1_l = ctx.block_label(r1_idx);

    // R1, part 1: the receiver is a heap object pointer.
    let bits = ctx.block().bitcast_double_to_i64(recv);
    let top = ctx.block().lshr(I64, &bits, "48");
    let is_ptr = ctx.block().icmp_eq(I64, &top, "32765"); // 0x7FFD, the pointer tag
    ctx.block().cond_br(&is_ptr, &handle_l, generic_l);

    ctx.current_block = handle_idx;
    let handle = ctx.block().and(I64, &bits, POINTER_MASK_I64);
    let real = ctx.block().icmp_ugt(I64, &handle, SMALL_HANDLE_MAX);
    ctx.block().cond_br(&real, &r1_l, generic_l);

    // R1, part 2: ONE shape compare against the learned region word. By
    // #10828's rule 3 only a GC_TYPE_OBJECT carrying that shape can match, so
    // this compare is the whole receiver classification.
    ctx.current_block = r1_idx;
    let word = ctx.block().load_atomic_monotonic(I64, &sites.word_g, 8);
    let expected = ctx.block().trunc(I64, &word, I32);
    let sid_addr = ctx.block().add(I64, &handle, "4");
    let sid_ptr = ctx.block().inttoptr(I64, &sid_addr);
    let sid = ctx.block().load(I32, &sid_ptr);
    let hit = ctx.block().icmp_eq(I32, &sid, &expected);
    ctx.block().cond_br(&hit, hit_l, miss_l);

    Entry { handle, word, sid }
}

/// R2: every key's slot, addressed from the same unmasked pointer and the same
/// word. Uniform addressing is what lets LLVM turn the run into one
/// `vgatherqpd` (§L7.6.1).
pub(crate) fn emit_slot_loads(ctx: &mut FnCtx<'_>, entry: &Entry, keys: usize) -> Vec<String> {
    let header = crate::target_layout::object_header_size_bytes(ctx.target_triple).to_string();
    let fields = ctx.block().add(I64, &entry.handle, &header);
    let fields_ptr = ctx.block().inttoptr(I64, &fields);
    let mut values = Vec::with_capacity(keys);
    for i in 0..keys {
        let shift = (32 + SLOT_BITS * i as u32).to_string();
        let shifted = ctx.block().lshr(I64, &entry.word, &shift);
        let slot = ctx.block().and(I64, &shifted, "63");
        let field_ptr = ctx.block().gep(DOUBLE, &fields_ptr, &[(I64, &slot)]);
        values.push(ctx.block().load(DOUBLE, &field_ptr));
    }
    values
}

/// The miss block: prime at most `PRIME_ATTEMPTS` times for the life of the
/// process, so a polymorphic or spill-located site stops paying for it.
pub(crate) fn emit_miss(
    ctx: &mut FnCtx<'_>,
    sites: &Sites,
    prime_l: &str,
    generic_l: &str,
) -> String {
    let tries = ctx.block().load(I32, &sites.tries_g);
    let may_prime = ctx.block().icmp_ult(I32, &tries, PRIME_ATTEMPTS);
    ctx.block().cond_br(&may_prime, prime_l, generic_l);
    tries
}

/// The prime block: bump the counter and hand the runtime the site word plus
/// the receiver's own shape and this region's keys.
///
/// The runtime packs AND publishes (`js_region_guard_prime`): a cache word's
/// store belongs to the code that owns its memory ordering, the same split the
/// property IC uses. Emitting the store here instead cost a real program —
/// `store atomic` parses in the textual backend but NOT in perry's native IR
/// construction, which every large module takes, so tsc failed codegen in 20
/// of 50 units while every fixture built.
pub(crate) fn emit_prime(
    ctx: &mut FnCtx<'_>,
    sites: &Sites,
    entry: &Entry,
    tries: &str,
    keys: &[&str],
    generic_l: &str,
) {
    let next_tries = ctx.block().add(I32, tries, "1");
    ctx.block().store(I32, &next_tries, &sites.tries_g);
    let mut key_bits: Vec<String> = Vec::with_capacity(MAX_KEYS);
    for i in 0..MAX_KEYS {
        if let Some(key) = keys.get(i) {
            let idx = ctx.strings.intern(key);
            let handle_global = format!("@{}", ctx.strings.entry(idx).handle_global);
            let boxed = ctx.block().load(DOUBLE, &handle_global);
            key_bits.push(ctx.block().bitcast_double_to_i64(&boxed));
        } else {
            key_bits.push("0".to_string());
        }
    }
    let n = keys.len().to_string();
    let word_ptr = sites.word_g.clone();
    ctx.block().call(
        I64,
        "js_region_guard_prime",
        &[
            (PTR, &word_ptr),
            (I32, &entry.sid),
            (I32, &n),
            (I64, &key_bits[0]),
            (I64, &key_bits[1]),
            (I64, &key_bits[2]),
            (I64, &key_bits[3]),
            (I64, &key_bits[4]),
        ],
    );
    ctx.block().br(generic_l);
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
