//! LIFE G1: the death points of the census's lifetime claims (check builds).
//!
//! Each claim of `collectors::life_claims` becomes a call at its death point:
//! * a `frame` local at the end of the statement list that binds it (its
//!   scope, one loop iteration), at each `break` / `continue` that leaves
//!   that list, and before each `return`: the slot is read,
//!   cleared (the slot a release frame would reuse), and its old value goes
//!   to `perry_verify_facts_life_dead` as an integer, so no statepoint roots
//!   it;
//! * an argument birth whose callee fate is Dropped: the lowered birth is
//!   registered (`perry_verify_facts_life_arg`, a token in an entry alloca),
//!   and right after the call, on the normal edge, the token is declared dead
//!   (`perry_verify_facts_life_arg_dead`).
//!
//! The runtime (`wpfacts_check/life.rs`) reports a collection that reaches a
//! dead object, with the holder. Without `PERRY_VERIFY_FACTS=1` the census
//! yields no claims and nothing here emits.
use std::cell::RefCell;

use perry_hir::{Expr, Stmt};

use super::{Fact, MODULE};
use crate::collectors::life_claims::ClaimBirth;
use crate::expr::FnCtx;
use crate::types::{DOUBLE, I64};

thread_local! {
    /// Argument births of the calls being lowered: (birth expression
    /// address, token slot).
    static WATCH: RefCell<Vec<(usize, String)>> = const { RefCell::new(Vec::new()) };
    /// The statement lists being lowered that bind claimed `frame` locals:
    /// (owning `FnCtx` address, `loop_targets` length when the list began,
    /// the list's claimed `let` ids). A `break` / `continue` to loop target
    /// `i` leaves every list of its body that began after target `i`.
    static LISTS: RefCell<Vec<(usize, usize, Vec<u32>)>> = const { RefCell::new(Vec::new()) };
}

/// The sampling rate of the exact check at death: the first death of each
/// claim site and every Nth after it (`PERRY_VERIFY_FACTS_LIFE_EVERY`,
/// default 4096; 1 checks every death).
fn every() -> u64 {
    std::env::var("PERRY_VERIFY_FACTS_LIFE_EVERY")
        .ok()
        .and_then(|v| v.parse().ok())
        .filter(|&n: &u64| n > 0)
        .unwrap_or(4096)
}

/// The claim record of one death point: the facts claim record plus the
/// birth, the death and the fate.
fn site(
    ctx: &mut FnCtx<'_>,
    b: &ClaimBirth,
    death: &str,
    byte_offset: Option<u32>,
) -> (String, String) {
    let (name, ordinal) = MODULE.with(|m| {
        let m = m.borrow();
        let m = m.as_ref().expect("verification module initialized");
        (m.name.clone(), m.ordinal)
    });
    let predicate = if death == "call" {
        "dead after call"
    } else {
        "dead after scope"
    };
    // One record format: a bet's line is rewritten to `PERRY_BET_FAILED` by
    // the runtime from `proof`.
    let marker = "PERRY_FALSE_PROOF";
    let metadata = serde_json::json!({
        "marker": marker, "module": name, "byte_offset": byte_offset,
        "key": format!("{} {} {}", b.kind, b.class, b.offset), "verdict": "Lifetime",
        "predicate": predicate, "proven_receivers": [],
        "birth": {"kind": b.kind, "class": b.class, "offset": b.offset},
        "death": {"at": death, "body": ctx.func.name},
        "fate": b.fate, "proof": b.proof,
        "copy": {"body": ctx.func.name, "site": null, "emission": ordinal},
        "compile_sha": option_env!("PERRY_BUILD_GIT_SHA").unwrap_or("unknown"),
    });
    super::covered(
        Fact::Lifetime,
        || serde_json::json!({"birth": b.kind, "death": death, "proof": b.proof}),
    );
    let mut bytes = serde_json::to_vec(&metadata).expect("static verification metadata");
    assert_eq!(bytes.pop(), Some(b'}'));
    super::raw_bytes(ctx.strings.module_prefix(), &bytes)
}

/// Frame local `id` dies here (`death`: `scope` or `return`).
fn frame_death(ctx: &mut FnCtx<'_>, id: u32, death: &str) {
    let Some(b) = ctx.life_claims.frame.get(&id).cloned() else {
        return;
    };
    if ctx.boxed_vars.contains(&id) || ctx.module_globals.contains_key(&id) {
        return;
    }
    let Some(slot) = ctx.locals.get(&id).cloned() else {
        return;
    };
    if ctx.block().is_terminated() {
        return;
    }
    let undef = crate::nanbox::double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
    let old = ctx.block().load(DOUBLE, &slot);
    ctx.block().store(DOUBLE, &undef, &slot);
    crate::expr::emit_shadow_slot_update_for_expr(ctx, id, &undef, &Expr::Undefined);
    let bits = ctx.block().bitcast_double_to_i64(&old);
    let (pointer, len) = site(ctx, &b, death, None);
    let every = every().to_string();
    ctx.block().call_void(
        "perry_verify_facts_life_dead",
        &[
            (I64, &bits),
            (I64, "0"),
            (I64, &every),
            (I64, &pointer),
            (I64, &len),
        ],
    );
}

fn owner(ctx: &FnCtx<'_>) -> usize {
    ctx as *const FnCtx<'_> as *const () as usize
}

/// The statement list `stmts` begins: it is an exit scope for `break` /
/// `continue` when it binds a claimed local. Paired with `list_end`.
pub(crate) fn list_begin(ctx: &mut FnCtx<'_>, stmts: &[Stmt]) {
    if ctx.life_claims.frame.is_empty() {
        return;
    }
    let ids: Vec<u32> = stmts
        .iter()
        .filter_map(|s| match s {
            Stmt::Let { id, .. } if ctx.life_claims.frame.contains_key(id) => Some(*id),
            _ => None,
        })
        .collect();
    let entry = (owner(ctx), ctx.loop_targets.len(), ids);
    LISTS.with(|l| l.borrow_mut().push(entry));
}

/// The statement list `stmts` ends; on its normal edge (`lowered`) its
/// `frame` locals die.
pub(crate) fn list_end(ctx: &mut FnCtx<'_>, stmts: &[Stmt], lowered: bool) {
    if ctx.life_claims.frame.is_empty() {
        return;
    }
    LISTS.with(|l| l.borrow_mut().pop());
    if !lowered {
        return;
    }
    for s in stmts {
        if let Stmt::Let { id, .. } = s {
            frame_death(ctx, *id, "scope");
        }
    }
}

/// A `break` / `continue` to loop target `target` (an index into
/// `loop_targets`) leaves the lists of that loop's (or switch's) body: their
/// `frame` locals bound so far die. A target that is not a loop target (a
/// labelled block) emits nothing: a missed check, never a wrong one.
pub(crate) fn at_exit(ctx: &mut FnCtx<'_>, target: Option<usize>) {
    if ctx.life_claims.frame.is_empty() {
        return;
    }
    let Some(target) = target else {
        return;
    };
    let me = owner(ctx);
    let ids: Vec<u32> = LISTS.with(|l| {
        l.borrow()
            .iter()
            .filter(|(o, depth, _)| *o == me && *depth > target)
            .flat_map(|(_, _, ids)| ids.iter().copied())
            .collect()
    });
    for id in ids {
        frame_death(ctx, id, "exit");
    }
}

/// A `return` of `value` leaves the body: every `frame` local bound so far
/// dies. The death calls can collect, so `value` is rooted across them and
/// the relocated value is returned.
pub(crate) fn at_return(ctx: &mut FnCtx<'_>, value: String) -> anyhow::Result<String> {
    if ctx.life_claims.frame.is_empty() || !ctx.inline_ctor_return.is_empty() {
        return Ok(value);
    }
    let ids: Vec<u32> = ctx.life_claims.frame.keys().copied().collect();
    if !ids.iter().any(|id| ctx.locals.contains_key(id)) {
        return Ok(value);
    }
    crate::rooting::with_scoped_values(ctx, &[(&value, None)], |ctx, reads| {
        for id in ids {
            frame_death(ctx, id, "return");
        }
        crate::expr::lower_expr(ctx, &reads[0])
    })
}

/// A direct call at `(start, end)` is about to be lowered: its claimed
/// argument births get a token slot each. Returns the (slot, site) pairs
/// `end_call` declares dead.
pub(crate) fn watch_call(
    ctx: &mut FnCtx<'_>,
    start: u32,
    end: u32,
    args: &[Expr],
) -> Vec<(String, String, String)> {
    if ctx.life_claims.dropped_args.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    for (i, a) in args.iter().enumerate() {
        let Some(b) = ctx.life_claims.dropped_args.get(&(start, end, i)).cloned() else {
            continue;
        };
        let slot = ctx.func.alloca_entry(I64);
        ctx.func.entry_allocas_push_store(I64, "0", &slot);
        let (pointer, len) = site(ctx, &b, "call", Some(start.saturating_sub(1)));
        WATCH.with(|w| {
            w.borrow_mut()
                .push((a as *const Expr as usize, slot.clone()))
        });
        out.push((slot, pointer, len));
    }
    out
}

/// A watched argument birth was lowered to `value`: register it.
pub(crate) fn birth_lowered(ctx: &mut FnCtx<'_>, e: &Expr, value: &str) {
    if ctx.life_claims.dropped_args.is_empty() {
        return;
    }
    let at = e as *const Expr as usize;
    let Some(slot) = WATCH.with(|w| {
        w.borrow()
            .iter()
            .rev()
            .find(|(p, _)| *p == at)
            .map(|(_, s)| s.clone())
    }) else {
        return;
    };
    if ctx.block().is_terminated() {
        return;
    }
    let bits = ctx.block().bitcast_double_to_i64(value);
    let token = ctx
        .block()
        .call(I64, "perry_verify_facts_life_arg", &[(I64, &bits)]);
    ctx.block().store(I64, &token, &slot);
}

/// The watched call returned `value` normally: its claimed arguments die.
/// The death calls can collect, so `value` is rooted across them and the
/// relocated value is returned.
pub(crate) fn end_call(
    ctx: &mut FnCtx<'_>,
    watched: Vec<(String, String, String)>,
    value: String,
) -> anyhow::Result<String> {
    if watched.is_empty() {
        return Ok(value);
    }
    WATCH.with(|w| {
        let mut w = w.borrow_mut();
        for (slot, _, _) in &watched {
            if let Some(i) = w.iter().rposition(|(_, s)| s == slot) {
                w.remove(i);
            }
        }
    });
    if ctx.block().is_terminated() {
        return Ok(value);
    }
    let every = every().to_string();
    crate::rooting::with_scoped_values(ctx, &[(&value, None)], |ctx, reads| {
        for (slot, pointer, len) in watched {
            let token = ctx.block().load(I64, &slot);
            ctx.block().store(I64, "0", &slot);
            ctx.block().call_void(
                "perry_verify_facts_life_arg_dead",
                &[(I64, &token), (I64, &every), (I64, &pointer), (I64, &len)],
            );
        }
        crate::expr::lower_expr(ctx, &reads[0])
    })
}
