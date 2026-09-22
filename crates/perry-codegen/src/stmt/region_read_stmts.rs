//! Step 4b, stage 2, slice 2 — a read region formed across STATEMENTS
//! (#10884).
//!
//! Slice 1 guarded a run of reads that already sat inside one `+` tree. The
//! same program spelled across statements is the larger population: on a tsc
//! compile slice 1 covered 28 regions / 56 reads while the census counted
//! **88 runs / 205 reads** in this shape, which is what this slice takes.
//!
//! ```text
//! const a = o.a; const b = o.b; const c = o.c;   // one run, three reads
//! ```
//!
//! # Why this cannot be slice 1 with a different matcher
//!
//! [`crate::stmt::masked_window_region`] — the existing straight-line
//! statement-run speculation — refuses `Stmt::Let` outright, and its reason is
//! this slice's whole design problem: it emits a fast copy and a slow copy of
//! the run, and a `Let` lowered once per copy allocates an entry alloca PER
//! COPY, so `ctx.locals[id]` ends up naming the last copy's slot and every
//! post-region read sees only that one. Duplicating the run is not available
//! here, because `Let`s are this slice's population.
//!
//! Restructuring into "load every value, phi, then bind" is not available
//! either: in the bail arm the earlier values would sit in registers across
//! later reads, and a generic read can reach a getter, allocate and move the
//! heap — the unrooted-across-safepoint hazard of `gc-rooting-invariant.md`
//! case 3.
//!
//! # The structure: declare once, assign in both arms
//!
//! 1. Declare each binding first, through the ordinary `Stmt::Let` path with
//!    no initialiser, so the slot, its type registration and its shadow-slot
//!    binding are exactly what today's code makes. One slot, dominating both
//!    arms.
//! 2. Fast arm: R1 once, then per read a slot load **stored immediately into
//!    that binding's slot**, so each value is rooted before the next read.
//! 3. Bail arm: the same reads in source order through today's lowering, each
//!    assigning the same slot (`Expr::LocalSet`).
//! 4. Merge. No phi — both arms write the same rooted slots.
//!
//! # R3 is not needed here, and that widens the slice
//!
//! Slice 1 had to prove every leaf a primitive Number because the fold hoisted
//! reads above the additions. This slice hoists nothing across an operator:
//! the reads happen in source order and the values are bound as they are. So
//! there is no type condition at all, and a `string`- or object-valued field
//! qualifies where slice 1 had to decline it. The soundness argument is only:
//! one guard proves the shape for the whole run, no user code can run between
//! the reads (every key in the word is an own data property, so no getter can
//! be reached), and nothing allocates between them, so the unmasked pointer
//! cannot go stale.
//!
//! # What ends a run
//!
//! The same R1–R4 events as slice 1 — a call, a store, an allocation, an
//! unverified operator — which mechanically means: any statement that is not a
//! `Stmt::Let` of a static-key read on the same receiver local.

use anyhow::Result;
use perry_hir::{Expr, Stmt};

use crate::expr::region_guard::{
    self, emit_miss, emit_prime, emit_r1, emit_slot_loads, state_globals, Entry, MAX_KEYS,
};
use crate::expr::{lower_expr, FnCtx};
use crate::types::DOUBLE;

/// One binding in the run: the statement's local, and which of the region's
/// keys it reads.
struct Bind {
    id: u32,
    key: usize,
}

pub(crate) struct StmtRun<'a> {
    receiver: u32,
    keys: Vec<&'a str>,
    binds: Vec<Bind>,
    /// Statements this run consumes — always `binds.len()`, named separately
    /// because the caller advances by it.
    pub(crate) len: usize,
}

/// Would this binding's storage be a plain double slot, AND does declaring it
/// without its initialiser cost nothing?
///
/// Only a plain slot can be assigned by a bare store in the fast arm, and the
/// answer must be decidable BEFORE anything is lowered: an undecidable binding
/// declines the whole run and the statements lower unchanged, rather than
/// being discovered after the slots exist, when backing out would mean
/// emitting a shape of `Let` the ordinary path never emits.
///
/// The second half of the question is the one measurement had to teach me.
/// `let_stmt` refines a declared `Any` from the INITIALISER
/// (`refine_type_from_init`), and this slice declares the binding without one,
/// so a binding whose init refines would lose that type and every later use of
/// it would deoptimise. Measured on four `string`-valued fields: 670 -> 2694
/// instructions per iteration, a 4x REGRESSION, while the region itself was
/// working exactly as designed. Declining a refinable binding keeps today's
/// facts intact; carrying the refinement through the declaration is what a
/// later slice has to do to reach that population.
fn binding_is_plain_slot(
    ctx: &FnCtx<'_>,
    id: u32,
    ty: &perry_hir::types::Type,
    init: &Expr,
) -> bool {
    // An `Any` binding of a property read takes `let_stmt`'s plain path: the
    // i32, canonical-string, POD, typed-array and scalar-replacement tiers all
    // key off an initialiser shape or a declared type this is not. A typed
    // binding is not WRONG here, it is simply not proven to be a plain slot at
    // this point, so this slice leaves it to a later one and the census counts
    // it as still uncovered.
    matches!(ty, perry_hir::types::Type::Any)
        && crate::type_analysis::refine_type_from_init(ctx, init).is_none()
        && !ctx.boxed_vars.contains(&id)
        && !ctx.prealloc_boxes.contains(&id)
        && !ctx.tdz_boxes.contains(&id)
        && !ctx.module_globals.contains_key(&id)
        && !ctx.pod_records.contains_key(&id)
        && !ctx.spec_ta_bindings.contains_key(&id)
        && !ctx.integer_locals.contains(&id)
        && !ctx.local_slot_reps.contains_key(&id)
}

/// The maximal run of same-receiver static-key reads at the head of `stmts`.
pub(crate) fn try_match<'a>(ctx: &FnCtx<'_>, stmts: &'a [Stmt]) -> Option<StmtRun<'a>> {
    if !region_guard::emission_allowed() {
        return None;
    }
    scan(stmts, |id, ty, init| {
        binding_is_plain_slot(ctx, id, ty, init)
    })
}

/// The matcher proper, taking "is this binding a plain slot?" as a function so
/// the run rules can be tested without building a `FnCtx`.
fn scan<'a>(
    stmts: &'a [Stmt],
    plain: impl Fn(u32, &perry_hir::types::Type, &Expr) -> bool,
) -> Option<StmtRun<'a>> {
    let mut receiver: Option<u32> = None;
    let mut keys: Vec<&'a str> = Vec::new();
    let mut binds: Vec<Bind> = Vec::new();

    for stmt in stmts {
        let Stmt::Let {
            id,
            ty,
            init: Some(init),
            ..
        } = stmt
        else {
            break;
        };
        let Expr::PropertyGet {
            object, property, ..
        } = init
        else {
            break;
        };
        let Expr::LocalGet(r) = object.as_ref() else {
            break;
        };
        match receiver {
            None => receiver = Some(*r),
            Some(prev) if prev == *r => {}
            Some(_) => break,
        }
        // The receiver must not be one of the run's own bindings, and a
        // binding must not be re-declared inside the run: either would make
        // the single entry guard cover a receiver it did not prove.
        if *r == *id || binds.iter().any(|b| b.id == *id) {
            break;
        }
        if !plain(*id, ty, init) {
            break;
        }
        let key = match keys.iter().position(|k| *k == property.as_str()) {
            Some(i) => i,
            None => {
                if keys.len() == MAX_KEYS {
                    break;
                }
                keys.push(property.as_str());
                keys.len() - 1
            }
        };
        binds.push(Bind { id: *id, key });
    }

    // One read already pays one guard; there is nothing to share below two.
    if binds.len() < 2 {
        return None;
    }
    Some(StmtRun {
        receiver: receiver?,
        keys,
        len: binds.len(),
        binds,
    })
}

/// Lower the run: declare every binding, then one guard for all of them.
pub(crate) fn lower(ctx: &mut FnCtx<'_>, stmts: &[Stmt], run: &StmtRun<'_>) -> Result<()> {
    region_guard::note_stmt_region(run.binds.len() as u64);

    // 1. Declare each binding with no initialiser, through the ordinary path,
    //    so the slot and its registrations are the ones today's code makes.
    for (bind, stmt) in run.binds.iter().zip(stmts.iter()) {
        let Stmt::Let {
            id,
            name,
            ty,
            mutable,
            ..
        } = stmt
        else {
            unreachable!("try_match admitted only Stmt::Let");
        };
        debug_assert_eq!(*id, bind.id);
        super::lower_stmt(
            ctx,
            &Stmt::Let {
                id: *id,
                name: name.clone(),
                ty: ty.clone(),
                mutable: *mutable,
                init: None,
            },
        )?;
    }
    let slots: Vec<String> = run
        .binds
        .iter()
        .map(|b| ctx.locals.get(&b.id).cloned())
        .collect::<Option<Vec<_>>>()
        .expect("a declared plain binding has a slot");

    let sites = state_globals(ctx);
    let fast_idx = ctx.new_block("region.stmt.fast");
    let miss_idx = ctx.new_block("region.stmt.miss");
    let prime_idx = ctx.new_block("region.stmt.prime");
    let generic_idx = ctx.new_block("region.stmt.generic");
    let merge_idx = ctx.new_block("region.stmt.merge");
    let fast_l = ctx.block_label(fast_idx);
    let miss_l = ctx.block_label(miss_idx);
    let prime_l = ctx.block_label(prime_idx);
    let generic_l = ctx.block_label(generic_idx);
    let merge_l = ctx.block_label(merge_idx);

    // 2. R1 once for the whole run.
    let recv = lower_expr(ctx, &Expr::LocalGet(run.receiver))?;
    let entry: Entry = emit_r1(ctx, &recv, &sites, &fast_l, &miss_l, &generic_l);

    // 3. Fast arm: R2, and each value into its own slot as it is loaded, so a
    //    loaded pointer is rooted before the next load runs.
    ctx.current_block = fast_idx;
    let values = emit_slot_loads(ctx, &entry, run.keys.len());
    for (bind, slot) in run.binds.iter().zip(slots.iter()) {
        let value = values[bind.key].clone();
        ctx.block().store(DOUBLE, &value, slot);
    }
    ctx.block().br(&merge_l);

    // 4. Miss: prime at most a bounded number of times, then retire.
    ctx.current_block = miss_idx;
    let tries = emit_miss(ctx, &sites, &prime_l, &generic_l);
    ctx.current_block = prime_idx;
    emit_prime(ctx, &sites, &entry, &tries, &run.keys, &generic_l);

    // 5. Generic copy: the same reads, in source order, assigning the same
    //    slots — the code this region replaces.
    ctx.current_block = generic_idx;
    crate::expr::emit_versioned_loop_callback_deopt(ctx);
    {
        let _suppressed = region_guard::Suppressed::enter();
        for stmt in stmts.iter().take(run.len) {
            let Stmt::Let {
                id,
                init: Some(init),
                ..
            } = stmt
            else {
                unreachable!("try_match admitted only initialised Stmt::Let");
            };
            super::lower_stmt(
                ctx,
                &Stmt::Expr(Expr::LocalSet(*id, Box::new(init.clone()))),
            )?;
        }
    }
    ctx.block().br(&merge_l);

    ctx.current_block = merge_idx;
    Ok(())
}

#[cfg(test)]
mod tests {
    use perry_hir::types::Type;
    use perry_hir::{Expr, Stmt};

    use super::scan;

    fn read_let(id: u32, recv: u32, key: &str) -> Stmt {
        Stmt::Let {
            id,
            name: format!("v{id}"),
            ty: Type::Any,
            mutable: false,
            init: Some(Expr::PropertyGet {
                object: Box::new(Expr::LocalGet(recv)),
                property: key.to_string(),
                byte_offset: 0,
            }),
        }
    }

    fn plain_always(_: u32, ty: &Type, _: &Expr) -> bool {
        matches!(ty, Type::Any)
    }

    #[test]
    fn consecutive_reads_of_one_receiver_form_one_run() {
        let stmts = vec![
            read_let(10, 1, "a"),
            read_let(11, 1, "b"),
            read_let(12, 1, "c"),
        ];
        let run = scan(&stmts, plain_always).expect("three reads of one receiver are a run");
        assert_eq!(run.len, 3, "the run consumes all three statements");
        assert_eq!(run.keys, vec!["a", "b", "c"]);
    }

    /// One read already pays one guard, so there is nothing to share.
    #[test]
    fn a_single_read_is_not_a_run() {
        assert!(scan(&[read_let(10, 1, "a")], plain_always).is_none());
    }

    /// A repeated key is one slot in the word, read twice.
    #[test]
    fn a_repeated_key_shares_its_slot() {
        let stmts = vec![
            read_let(10, 1, "a"),
            read_let(11, 1, "b"),
            read_let(12, 1, "a"),
        ];
        let run = scan(&stmts, plain_always).unwrap();
        assert_eq!(run.keys, vec!["a", "b"], "two distinct keys");
        assert_eq!(run.len, 3, "but three bindings");
    }

    /// A second receiver needs a second guard: the run ends where it appears.
    #[test]
    fn a_different_receiver_ends_the_run() {
        let stmts = vec![
            read_let(10, 1, "a"),
            read_let(11, 1, "b"),
            read_let(12, 2, "a"),
        ];
        let run = scan(&stmts, plain_always).unwrap();
        assert_eq!(run.len, 2);
    }

    /// Any statement that is not such a read is an R1-R4 event: a call, a store,
    /// an allocation or an unverified operator all arrive here as "not a read".
    #[test]
    fn a_non_read_statement_ends_the_run() {
        let call = Stmt::Expr(Expr::Call {
            callee: Box::new(Expr::LocalGet(7)),
            args: Vec::new(),
            type_args: Vec::new(),
            byte_offset: 0,
        });
        let stmts = vec![
            read_let(10, 1, "a"),
            read_let(11, 1, "b"),
            call,
            read_let(12, 1, "c"),
        ];
        let run = scan(&stmts, plain_always).unwrap();
        assert_eq!(run.len, 2, "the call ends the run before the third read");
    }

    /// One word addresses five keys; the sixth distinct key ends the run rather
    /// than silently dropping a read out of it.
    #[test]
    fn the_sixth_distinct_key_ends_the_run() {
        let stmts: Vec<Stmt> = ["a", "b", "c", "d", "e", "f"]
            .iter()
            .enumerate()
            .map(|(i, k)| read_let(10 + i as u32, 1, k))
            .collect();
        let run = scan(&stmts, plain_always).unwrap();
        assert_eq!(run.len, 5);
        assert_eq!(run.keys.len(), 5);
    }

    /// Reading into the receiver's own binding would make one guard cover a
    /// receiver it did not prove.
    #[test]
    fn a_binding_that_is_the_receiver_ends_the_run() {
        let stmts = vec![
            read_let(10, 1, "a"),
            read_let(11, 1, "b"),
            read_let(1, 1, "c"),
        ];
        let run = scan(&stmts, plain_always).unwrap();
        assert_eq!(run.len, 2);
    }

    /// A binding whose storage is not a plain slot declines the whole run: the
    /// fast arm assigns it with a bare store, which only a plain slot accepts.
    #[test]
    fn a_binding_that_is_not_a_plain_slot_declines() {
        let stmts = vec![read_let(10, 1, "a"), read_let(11, 1, "b")];
        assert!(scan(&stmts, |_, _, _| false).is_none());
    }
}
