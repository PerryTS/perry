//! LIFE I1: the hooks of the lifetime measurement (`PERRY_LIFE_MEASURE` at
//! compile time; instrumented runtime variant only).
//!
//! * every census birth that is lowered calls `perry_life_birth` with its
//!   value, its site key (the census row's `module, function, kind, class,
//!   offset`) and its innermost loop;
//! * a body's activation: `perry_life_enter` where its statement list begins,
//!   `perry_life_return` at each `return` (the returned value rooted across
//!   it) and at the normal end of the list;
//! * the end of an iteration of a loop that encloses a birth:
//!   `perry_life_iter` at the loop's back-edge poll point (normal end of the
//!   body and `continue`) and after the loop (`break`, normal exit);
//! * the end of a turn: `perry_life_turn` after each microtask checkpoint of
//!   the entry's event loop.
//!
//! The runtime (`wpfacts_check/life_measure.rs`) counts births per site,
//! samples some and probes them at each boundary. Without the knob the census
//! records no birth and nothing here emits.
use perry_hir::{Expr, Stmt};

use crate::expr::FnCtx;
use crate::types::I64;

/// The site id the binary carries for site key `key`: FNV-1a 64 of the key
/// text. The compile writes `id<TAB>key` lines to the file the compile-time
/// knob names (a value other than `1`), so the run's TSV joins by id.
pub(crate) fn site_id(key: &str) -> u64 {
    key.bytes().fold(0xcbf2_9ce4_8422_2325u64, |h, b| (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3))
}

/// Appends the site keys of one body to the compile-time site file.
pub(crate) fn write_sites<'a>(keys: impl Iterator<Item = &'a str>) {
    use std::io::Write;
    use std::sync::{Mutex, OnceLock};
    static SEEN: OnceLock<Mutex<std::collections::HashSet<u64>>> = OnceLock::new();
    let Some(path) = std::env::var("PERRY_LIFE_MEASURE").ok().filter(|p| !p.is_empty() && p != "1") else {
        return;
    };
    let mut seen = SEEN.get_or_init(Default::default).lock().expect("life measure sites");
    let mut text = String::new();
    for k in keys {
        let id = site_id(k);
        if seen.insert(id) {
            text.push_str(&format!("{id}\t{k}\n"));
        }
    }
    if text.is_empty() {
        return;
    }
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
        let _ = f.write_all(text.as_bytes());
    }
}

fn on(ctx: &FnCtx<'_>) -> bool {
    ctx.life_claims.body != 0
}

/// A census birth was lowered to `value`.
pub(crate) fn birth(ctx: &mut FnCtx<'_>, e: &Expr, value: &str) {
    if !on(ctx) {
        return;
    }
    let Some((key, loop_body)) = ctx.life_claims.births.get(&(e as *const Expr as usize)).cloned() else {
        return;
    };
    if ctx.block().is_terminated() {
        return;
    }
    let site = (site_id(&key) as i64).to_string();
    let bits = ctx.block().bitcast_double_to_i64(value);
    let loop_id = (loop_body as u64 as i64).to_string();
    ctx.block().call_void("perry_life_birth", &[(I64, &bits), (I64, &site), (I64, &loop_id)]);
}

/// The statement list `stmts` begins: the body's own list starts its activation.
pub(crate) fn list_begin(ctx: &mut FnCtx<'_>, stmts: &[Stmt]) {
    if on(ctx) && stmts.as_ptr() as usize == ctx.life_claims.body && !ctx.block().is_terminated() {
        ctx.block().call_void("perry_life_enter", &[]);
    }
}

/// The statement list `stmts` ended normally: the body's own list returns.
pub(crate) fn list_end(ctx: &mut FnCtx<'_>, stmts: &[Stmt], lowered: bool) {
    if lowered
        && on(ctx)
        && stmts.as_ptr() as usize == ctx.life_claims.body
        && ctx.inline_ctor_return.is_empty()
        && !ctx.block().is_terminated()
    {
        ctx.block().call_void("perry_life_return", &[]);
    }
}

/// A `return` of `value` leaves the body. The hook can collect, so `value`
/// is rooted across it and the relocated value is returned.
pub(crate) fn at_return(ctx: &mut FnCtx<'_>, value: String) -> anyhow::Result<String> {
    if !on(ctx) || !ctx.inline_ctor_return.is_empty() || ctx.block().is_terminated() {
        return Ok(value);
    }
    if value.is_empty() {
        ctx.block().call_void("perry_life_return", &[]);
        return Ok(value);
    }
    crate::rooting::with_scoped_values(ctx, &[(&value, None)], |ctx, reads| {
        ctx.block().call_void("perry_life_return", &[]);
        crate::expr::lower_expr(ctx, &reads[0])
    })
}

/// An iteration of the loop with body `body` ends (back-edge or loop exit).
pub(crate) fn iteration(ctx: &mut FnCtx<'_>, body: &[Stmt]) {
    let at = body.as_ptr() as usize;
    if !on(ctx) || body.is_empty() || !ctx.life_claims.loops.contains(&at) || ctx.block().is_terminated() {
        return;
    }
    let loop_id = (at as u64).to_string();
    ctx.block().call_void("perry_life_iter", &[(I64, &loop_id)]);
}

/// The entry's event loop finished a microtask checkpoint with an unwound
/// JS stack: the turn ends.
pub(crate) fn turn(ctx: &mut FnCtx<'_>) {
    if super::life_measure_on() && !ctx.block().is_terminated() {
        ctx.block().call_void("perry_life_turn", &[]);
    }
}
