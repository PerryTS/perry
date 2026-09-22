//! Step 4b, stage 1, slice 1 — a read region formed over a `+` tree (#10884).
//!
//! # What a region is
//!
//! A region is defined by the CFG, not by an expression form: a single-entry
//! run of accesses over which one receiver's `(unmasked pointer, ShapeId)` pair
//! is held, entered through ONE shape compare, whose failure leaves for a
//! generic copy of the whole run and never rejoins it (design doc §L7.1–L7.3).
//!
//! This file forms the first slice of that: the runs that already sit inside
//! one `+` tree (`h += o.a + o.b + o.c`), because that is where the leaves are
//! already collected. The same program spelled across statements
//! (`const a = o.a; const b = o.b; h += a + b;`) is the next slice and is NOT
//! formed here — it is measured as a negative control so the gap is known.
//!
//! # The rule (design doc §L7.3), and why it is sound
//!
//! ```text
//! [R1] guard   tag test + unmask + ONE ShapeId compare      ─┐ the only two
//! [R2] load    every key's slot, from one atomic region word │ bail edges,
//! [R3] verify  every leaf value is a primitive Number        ─┘ before any effect
//! [R4] use     fold the tree with `fadd`
//! ```
//!
//! **The region does not compute the right answer when an operand is
//! unfriendly; it declines before computing one.** Hoisting every leaf above
//! the additions is exactly what #10904 did wrong. It is legal here *because*
//! R3 proves every leaf is a Number, so no addition can reach `ToPrimitive`
//! and therefore no user code can run between a leaf's source position and
//! where it was read. The proof is ordered load → check → use: a failed check
//! discards the loaded values and lowers the tree afresh in the generic copy,
//! in source order, so a wrong hoist is never observed.
//!
//! Discarding and re-evaluating is only legal because every leaf admitted here
//! is effect-free to evaluate: a read of the guarded receiver (the shape proves
//! it is an own data property, so no getter), a local, or a numeric literal.
//! That is precisely what #10921 could not assume for an arbitrary tree.
//!
//! # Supplier
//!
//! The expected ShapeId is learned (supplier (b), §L14.18.4): a per-region
//! atomic word primed on a miss by `js_region_guard_prime`. The id and every
//! key's slot live in ONE word so a concurrent prime can never pair one shape's
//! id with another's slots. A link-time constant (step 4) would replace the
//! word load and nothing else.
//!
//! # The miss side is today's code
//!
//! Every failure edge lands in the generic copy, which is the post-#10921
//! lowering of the same tree. A mispredicted region therefore costs a few
//! compares on top of what the tree costs without regions — never a cliff
//! (#10503's 18× is what a miss into the by-name ladder costs; this never goes
//! there). Priming is bounded to `PRIME_ATTEMPTS` per region for the life of
//! the process, so a polymorphic or inherited-read site stops paying for it.

use std::cell::Cell;

use anyhow::Result;
use perry_hir::{BinaryOp, Expr};

use super::{lower_expr, FnCtx};
use crate::nanbox::POINTER_MASK_I64;
use crate::types::{DOUBLE, I1, I32, I64, PTR};

/// Must equal `perry_runtime::object::shapes::REGION_GUARD_MAX_KEYS`.
const MAX_KEYS: usize = 5;
/// Must equal the runtime's slot width.
const SLOT_BITS: u32 = 6;
/// `REGION_GUARD_WORD_EMPTY`: low half `u32::MAX`, never a live ShapeId.
const EMPTY_WORD: &str = "4294967295";
/// Primes attempted per region before it stops trying (process lifetime).
const PRIME_ATTEMPTS: &str = "8";
/// A small-handle band sits under the pointer tag; its ids are not addresses.
const SMALL_HANDLE_MAX: &str = "1048575";

thread_local! {
    /// Non-zero while the generic copy of a region is being lowered. The
    /// generic copy lowers the SAME tree through the ordinary dispatch, which
    /// would otherwise form the same region again inside itself.
    static SUPPRESS: Cell<u32> = const { Cell::new(0) };
    static REGIONS_FORMED: Cell<u64> = const { Cell::new(0) };
    static READS_COVERED: Cell<u64> = const { Cell::new(0) };
}

struct Suppressed;
impl Suppressed {
    fn enter() -> Self {
        SUPPRESS.with(|s| s.set(s.get() + 1));
        Suppressed
    }
}
impl Drop for Suppressed {
    fn drop(&mut self) {
        SUPPRESS.with(|s| s.set(s.get() - 1));
    }
}

fn disabled() -> bool {
    matches!(
        std::env::var("PERRY_REGION_READS").as_deref(),
        Ok("0") | Ok("off") | Ok("false")
    )
}

enum Leaf<'a> {
    /// A read of the region's receiver; the index names its key.
    Region(usize),
    /// A local or a numeric literal — effect-free to evaluate twice.
    Other(&'a Expr),
}

struct Plan<'a> {
    receiver: u32,
    keys: Vec<&'a str>,
    leaves: Vec<Leaf<'a>>,
}

fn add_leaves<'a>(expr: &'a Expr, out: &mut Vec<&'a Expr>) {
    if let Expr::Binary {
        op: BinaryOp::Add,
        left,
        right,
    } = expr
    {
        add_leaves(left, out);
        add_leaves(right, out);
    } else {
        out.push(expr);
    }
}

/// Slice 1's admission: every leaf is effect-free to evaluate, and at least two
/// of them read ONE local receiver by a static key.
fn plan(expr: &Expr) -> Option<Plan<'_>> {
    let mut leaves = Vec::new();
    add_leaves(expr, &mut leaves);
    let mut receiver: Option<u32> = None;
    let mut keys: Vec<&str> = Vec::new();
    let mut region_reads = 0usize;
    let mut out = Vec::with_capacity(leaves.len());
    for leaf in leaves {
        match leaf {
            Expr::PropertyGet {
                object, property, ..
            } => {
                let Expr::LocalGet(id) = object.as_ref() else {
                    return None;
                };
                match receiver {
                    None => receiver = Some(*id),
                    Some(r) if r == *id => {}
                    Some(_) => return None,
                }
                let key = match keys.iter().position(|k| *k == property.as_str()) {
                    Some(i) => i,
                    None => {
                        keys.push(property.as_str());
                        keys.len() - 1
                    }
                };
                region_reads += 1;
                out.push(Leaf::Region(key));
            }
            Expr::LocalGet(_) | Expr::Number(_) | Expr::Integer(_) => out.push(Leaf::Other(leaf)),
            _ => return None,
        }
    }
    if region_reads < 2 || keys.len() > MAX_KEYS {
        return None;
    }
    Some(Plan {
        receiver: receiver?,
        keys,
        leaves: out,
    })
}

/// Rebuild the tree's shape with `fadd`, consuming leaf values in leaf order.
fn fold(ctx: &mut FnCtx<'_>, expr: &Expr, values: &[String], next: &mut usize) -> String {
    if let Expr::Binary {
        op: BinaryOp::Add,
        left,
        right,
    } = expr
    {
        let l = fold(ctx, left, values, next);
        let r = fold(ctx, right, values, next);
        return ctx.block().fadd(&l, &r);
    }
    let v = values[*next].clone();
    *next += 1;
    v
}

/// Lower `expr` as a read region, or return `None` to use the ordinary path.
pub(crate) fn try_lower_region_add_tree(
    ctx: &mut FnCtx<'_>,
    expr: &Expr,
) -> Result<Option<String>> {
    if ctx.canonical_read_generic || SUPPRESS.with(|s| s.get()) > 0 || disabled() {
        return Ok(None);
    }
    // Profiling builds record guard pass/fail on the per-access towers; a read
    // served before them would change a signal that must stay byte-identical.
    if crate::expr::typed_feedback_emission_enabled() {
        return Ok(None);
    }
    let Some(plan) = plan(expr) else {
        return Ok(None);
    };
    REGIONS_FORMED.with(|c| c.set(c.get() + 1));
    READS_COVERED.with(|c| {
        c.set(
            c.get()
                + plan
                    .leaves
                    .iter()
                    .filter(|l| matches!(l, Leaf::Region(_)))
                    .count() as u64,
        )
    });

    // Region state: one atomic word (id + slots) and a prime-attempt counter.
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

    // The non-receiver leaves first. They are effect-free, and lowering them
    // before the receiver means nothing that could allocate runs between the
    // receiver's unmask and its slot loads.
    let mut other_values: Vec<Option<String>> = Vec::with_capacity(plan.leaves.len());
    let mut other_needs_test: Vec<bool> = Vec::with_capacity(plan.leaves.len());
    for leaf in &plan.leaves {
        match leaf {
            Leaf::Other(e) => {
                other_values.push(Some(lower_expr(ctx, e)?));
                other_needs_test.push(!crate::type_analysis::expr_produces_canonical_raw_f64(
                    ctx, e,
                ));
            }
            Leaf::Region(_) => {
                other_values.push(None);
                other_needs_test.push(true);
            }
        }
    }

    let handle_idx = ctx.new_block("region.handle");
    let r1_idx = ctx.new_block("region.r1");
    let r2_idx = ctx.new_block("region.r2");
    let fold_idx = ctx.new_block("region.fold");
    let miss_idx = ctx.new_block("region.miss");
    let prime_idx = ctx.new_block("region.prime");
    let generic_idx = ctx.new_block("region.generic");
    let merge_idx = ctx.new_block("region.merge");
    let handle_l = ctx.block_label(handle_idx);
    let r1_l = ctx.block_label(r1_idx);
    let r2_l = ctx.block_label(r2_idx);
    let fold_l = ctx.block_label(fold_idx);
    let miss_l = ctx.block_label(miss_idx);
    let prime_l = ctx.block_label(prime_idx);
    let generic_l = ctx.block_label(generic_idx);
    let merge_l = ctx.block_label(merge_idx);

    // R1, part 1: the receiver is a heap object pointer.
    let recv = lower_expr(ctx, &Expr::LocalGet(plan.receiver))?;
    let bits = ctx.block().bitcast_double_to_i64(&recv);
    let top = ctx.block().lshr(I64, &bits, "48");
    let is_ptr = ctx.block().icmp_eq(I64, &top, "32765"); // 0x7FFD, the pointer tag
    ctx.block().cond_br(&is_ptr, &handle_l, &generic_l);

    ctx.current_block = handle_idx;
    let handle = ctx.block().and(I64, &bits, POINTER_MASK_I64);
    let real = ctx.block().icmp_ugt(I64, &handle, SMALL_HANDLE_MAX);
    ctx.block().cond_br(&real, &r1_l, &generic_l);

    // R1, part 2: ONE shape compare against the learned region word. By
    // #10828's rule 3 only a GC_TYPE_OBJECT carrying that shape can match, so
    // this compare is the whole receiver classification.
    ctx.current_block = r1_idx;
    let word_ptr = word_g.clone();
    let word = ctx.block().load_atomic_monotonic(I64, &word_ptr, 8);
    let expected = ctx.block().trunc(I64, &word, I32);
    let sid_addr = ctx.block().add(I64, &handle, "4");
    let sid_ptr = ctx.block().inttoptr(I64, &sid_addr);
    let sid = ctx.block().load(I32, &sid_ptr);
    let hit = ctx.block().icmp_eq(I32, &sid, &expected);
    ctx.block().cond_br(&hit, &r2_l, &miss_l);

    // R2 + R3: every key's slot from the same word, then prove every leaf is a
    // Number before any addition runs.
    ctx.current_block = r2_idx;
    let header = crate::target_layout::object_header_size_bytes(ctx.target_triple).to_string();
    let fields = ctx.block().add(I64, &handle, &header);
    let fields_ptr = ctx.block().inttoptr(I64, &fields);
    let mut key_values: Vec<String> = Vec::with_capacity(plan.keys.len());
    for i in 0..plan.keys.len() {
        let shift = (32 + SLOT_BITS * i as u32).to_string();
        let shifted = ctx.block().lshr(I64, &word, &shift);
        let slot = ctx.block().and(I64, &shifted, "63");
        let field_ptr = ctx.block().gep(DOUBLE, &fields_ptr, &[(I64, &slot)]);
        key_values.push(ctx.block().load(DOUBLE, &field_ptr));
    }
    let mut leaf_values: Vec<String> = Vec::with_capacity(plan.leaves.len());
    for (i, leaf) in plan.leaves.iter().enumerate() {
        leaf_values.push(match leaf {
            Leaf::Region(k) => key_values[*k].clone(),
            Leaf::Other(_) => other_values[i].clone().expect("lowered above"),
        });
    }
    let mut all_num: Option<String> = None;
    for (value, needs) in leaf_values.iter().zip(other_needs_test.iter()) {
        if !needs {
            continue;
        }
        let is_num = crate::stmt::emit_js_value_is_number(ctx, value);
        all_num = Some(match all_num {
            Some(prev) => ctx.block().and(I1, &prev, &is_num),
            None => is_num,
        });
    }
    match all_num {
        Some(cond) => ctx.block().cond_br(&cond, &fold_l, &generic_l),
        None => ctx.block().br(&fold_l),
    }

    // R4: nothing can call user code now, so the tree folds to `fadd`s.
    ctx.current_block = fold_idx;
    let fast = fold(ctx, expr, &leaf_values, &mut 0);
    let fast_end = ctx.block().label.clone();
    ctx.block().br(&merge_l);

    // Miss: prime at most PRIME_ATTEMPTS times for the life of the process.
    ctx.current_block = miss_idx;
    let tries = ctx.block().load(I32, &tries_g);
    let may_prime = ctx.block().icmp_ult(I32, &tries, PRIME_ATTEMPTS);
    ctx.block().cond_br(&may_prime, &prime_l, &generic_l);

    ctx.current_block = prime_idx;
    let next_tries = ctx.block().add(I32, &tries, "1");
    ctx.block().store(I32, &next_tries, &tries_g);
    let mut key_bits: Vec<String> = Vec::with_capacity(MAX_KEYS);
    for i in 0..MAX_KEYS {
        if let Some(key) = plan.keys.get(i) {
            let idx = ctx.strings.intern(key);
            let handle_global = format!("@{}", ctx.strings.entry(idx).handle_global);
            let boxed = ctx.block().load(DOUBLE, &handle_global);
            key_bits.push(ctx.block().bitcast_double_to_i64(&boxed));
        } else {
            key_bits.push("0".to_string());
        }
    }
    let n = plan.keys.len().to_string();
    // The runtime packs AND publishes: a cache word's store belongs to the
    // code that owns its memory ordering (`js_region_guard_prime`), the same
    // split the property IC uses. Emitting the store here instead cost a real
    // program: `store atomic` parses in the textual backend but not in
    // perry's native IR construction, which every large module takes, so tsc
    // failed codegen in 20 of 50 units while every fixture built.
    ctx.block().call(
        I64,
        "js_region_guard_prime",
        &[
            (PTR, &word_ptr),
            (I32, &sid),
            (I32, &n),
            (I64, &key_bits[0]),
            (I64, &key_bits[1]),
            (I64, &key_bits[2]),
            (I64, &key_bits[3]),
            (I64, &key_bits[4]),
        ],
    );
    ctx.block().br(&generic_l);

    // Generic copy: the same tree through the ordinary dispatch, in source
    // order — the code this region replaces.
    ctx.current_block = generic_idx;
    crate::expr::emit_versioned_loop_callback_deopt(ctx);
    let slow = {
        let _suppressed = Suppressed::enter();
        lower_expr(ctx, expr)?
    };
    let slow_end = ctx.block().label.clone();
    ctx.block().br(&merge_l);

    ctx.current_block = merge_idx;
    Ok(Some(
        ctx.block()
            .phi(DOUBLE, &[(&fast, &fast_end), (&slow, &slow_end)]),
    ))
}

/// `PERRY_REGION_DIAG=1`: per module, how many regions slice 1 formed, how many
/// reads they cover, and — the number that sizes slice 2 on real code — how
/// many runs of two or more consecutive same-receiver reads sit across
/// STATEMENTS where this slice does not reach them.
pub(crate) struct ModuleDiag {
    census: Option<(u64, u64)>,
    name: String,
}

impl ModuleDiag {
    pub(crate) fn start(hir: &perry_hir::Module) -> Self {
        REGIONS_FORMED.with(|c| c.set(0));
        READS_COVERED.with(|c| c.set(0));
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
                "[perry region] module={} regions={} reads_covered={} statement_runs_uncovered={} statement_reads_uncovered={}",
                self.name,
                REGIONS_FORMED.with(|c| c.get()),
                READS_COVERED.with(|c| c.get()),
                runs,
                reads
            );
        }
    }
}

fn statement_run_census(hir: &perry_hir::Module) -> (u64, u64) {
    let mut acc = (0u64, 0u64);
    census_stmts(&hir.init, &mut acc);
    for f in &hir.functions {
        census_stmts(&f.body, &mut acc);
    }
    for c in &hir.classes {
        for m in c
            .methods
            .iter()
            .chain(c.static_methods.iter())
            .chain(c.constructor.iter())
        {
            census_stmts(&m.body, &mut acc);
        }
    }
    acc
}

/// A statement-level read: `let x = <local>.<key>`.
fn let_read_receiver(stmt: &perry_hir::Stmt) -> Option<u32> {
    let perry_hir::Stmt::Let {
        init: Some(Expr::PropertyGet { object, .. }),
        ..
    } = stmt
    else {
        return None;
    };
    match object.as_ref() {
        Expr::LocalGet(id) => Some(*id),
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

#[cfg(test)]
mod tests {
    use super::*;

    fn read(recv: u32, key: &str) -> Expr {
        Expr::PropertyGet {
            object: Box::new(Expr::LocalGet(recv)),
            property: key.to_string(),
            byte_offset: 0,
        }
    }
    fn add(l: Expr, r: Expr) -> Expr {
        Expr::Binary {
            op: BinaryOp::Add,
            left: Box::new(l),
            right: Box::new(r),
        }
    }

    #[test]
    fn two_reads_of_one_receiver_form_a_region() {
        let e = add(Expr::LocalGet(9), add(read(1, "a"), read(1, "b")));
        let p = plan(&e).expect("h + (o.a + o.b) is a slice-1 region");
        assert_eq!(p.receiver, 1);
        assert_eq!(p.keys, vec!["a", "b"]);
    }

    /// A repeated key is one slot, read twice.
    #[test]
    fn a_repeated_key_shares_its_slot() {
        let e = add(add(read(1, "c"), read(1, "a")), read(1, "c"));
        let p = plan(&e).unwrap();
        assert_eq!(p.keys, vec!["c", "a"]);
    }

    /// One read is not a run: the ordinary tower serves it unchanged.
    #[test]
    fn a_single_read_is_not_a_region() {
        assert!(plan(&add(Expr::LocalGet(9), read(1, "a"))).is_none());
    }

    /// Two receivers need two guards; slice 1 takes one.
    #[test]
    fn two_receivers_are_declined() {
        assert!(plan(&add(read(1, "a"), read(2, "a"))).is_none());
    }

    /// A call leaf is not effect-free, so re-evaluating it in the generic copy
    /// after a failed check could run it twice. Must decline.
    #[test]
    fn a_leaf_that_is_not_effect_free_is_declined() {
        let call = Expr::Call {
            callee: Box::new(Expr::LocalGet(7)),
            args: Vec::new(),
            type_args: Vec::new(),
            byte_offset: 0,
        };
        assert!(plan(&add(add(read(1, "a"), read(1, "b")), call)).is_none());
    }

    #[test]
    fn more_keys_than_one_word_holds_are_declined() {
        let mut e = read(1, "k0");
        for k in ["k1", "k2", "k3", "k4", "k5"] {
            e = add(e, read(1, k));
        }
        assert!(plan(&e).is_none());
    }
}
