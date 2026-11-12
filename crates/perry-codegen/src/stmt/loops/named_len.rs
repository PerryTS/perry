//! What a store through another name may do to the Array a loop walks
//! (wpfacts `BindingFacts::named_length`, `perry_wpfacts` `named_lengths`).
//!
//! `for (let i = 0; i < a.length; i++) body` keeps `a.length` in a slot
//! when the loop's walker (`classify_for_length_hoist`) finds no store in
//! `body` that can change it. The walker reads the text: a store through
//! `a` it can judge, and a local the text makes an alias of `a` (`const b
//! = a`) it treats as `a`. Whether any other receiver, as `b` of `b.pop()`,
//! `this.items` of `this.items[n] = v` or `G` of `G.length = 0`, may be the
//! Array `a` holds is not in the text (`a = k ? G : F`, two parameters
//! bound to one Array, a field read, a call's result): the walker used to
//! assume it was not, and the loop ran on with a stale length.
//!
//! Now it asks (`Others`):
//! - the facts give `a` a named length: in this function every event that
//!   may change the length of an Array `a` may hold has a receiver read
//!   from `a`. No other store reaches it (`Cleared`);
//! - no such fact: every other receiver may be that Array (`Feared`). A
//!   store through one is then what the same store through `a` would be:
//!   `b[i] = v` under `i < a.length` stays inside whichever Array `b` is,
//!   anything else ends the hoist;
//! - a receiver whose length nothing can change (a sealed buffer, a proven
//!   native view) has nothing to ask, nor has any receiver with
//!   `PERRY_FACTS_NAMED_LENGTHS=0`, which is the walker of before
//!   (`Unasked`).
//!
//! The walker runs with `Feared` first: a loop it keeps that way leans on
//! no fact. Only a loop it would refuse is walked again with the others
//! cleared, and is kept when the facts do clear them: a loop that consumes
//! the fact (`LengthHoist::named`).
//!
//! Check mode (`PERRY_VERIFY_FACTS=1`): such a loop compares the kept
//! length with the live one before every run of its body ("named length",
//! exit 86), and a loop whose condition this file does not lower (a
//! versioned clone) does not consume the fact. `PERRY_ARRAY_BASE_DIAG=1`
//! prints one `[perry named len]` line per walk that asked.

use super::*;
use perry_hir::Expr;

/// `PERRY_FACTS_NAMED_LENGTHS=0` turns the question off.
pub(crate) fn enabled() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| {
        !matches!(
            std::env::var("PERRY_FACTS_NAMED_LENGTHS").as_deref(),
            Ok("0") | Ok("off") | Ok("false")
        )
    })
}

/// What a store through a name the text does not make an alias of the
/// guarded receiver may do (module docs).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Others {
    Unasked,
    Feared,
    Cleared,
}

/// The names of a loop's guarded receiver.
pub(super) struct Names {
    /// The locals the text makes aliases of it (itself among them).
    pub(super) ids: std::collections::HashSet<u32>,
    pub(super) others: Others,
    /// The walker asks of every read, store and operator of the loop
    /// whether it runs program code (`crate::quiet`): code that is not
    /// the loop's may change the receiver's length whatever the loop's own
    /// stores do. Not asked for a receiver whose length nothing changes,
    /// inside a versioned clone (dropped when a call survived in its
    /// emitted blocks), nor with `PERRY_FACTS_QUIET_SITES=0`.
    pub(super) quiet: bool,
}

/// Local `id` has a named length by the facts.
fn named(ctx: &FnCtx<'_>, id: u32) -> bool {
    !ctx.wpfacts_off
        && !ctx.wpfacts.interp_guard
        && !crate::expr::typed_feedback_emission_enabled()
        && ctx.wpfacts.named_length_locals.contains(&id)
}

pub(super) fn diag_on() -> bool {
    std::env::var("PERRY_ARRAY_BASE_DIAG").as_deref() == Ok("1")
}

/// One `[perry quiet]` line per loop the walker refuses for a site that
/// may run program code alone (`refused:site`), or for its bound's own
/// read (`refused:length-read`).
pub(super) fn quiet_diag(ctx: &FnCtx<'_>, arr: u32, verdict: &str) {
    if diag_on() {
        eprintln!(
            "[perry quiet] fn={} array={arr} verdict={verdict}",
            ctx.func.name
        );
    }
}

pub(super) fn diag(ctx: &FnCtx<'_>, arr: u32, verdict: &str) {
    if diag_on() {
        eprintln!(
            "[perry named len] fn={} array={arr} verdict={verdict}",
            ctx.func.name
        );
    }
}

impl Names {
    /// The names of `arr_id` with `ids` its aliases by the text, before the
    /// facts are asked. `ask_quiet`: the loop is no versioned clone, so the
    /// walker asks whether its sites run program code.
    pub(super) fn new(
        ctx: &FnCtx<'_>,
        arr_id: u32,
        ids: std::collections::HashSet<u32>,
        ask_quiet: bool,
    ) -> Self {
        let object = Expr::LocalGet(arr_id);
        let fixed = ctx.sealed_buffer_locals.contains(&arr_id)
            || crate::expr::proven_view_receiver(ctx, &object).is_some();
        let others = match enabled() && !fixed {
            true => Others::Feared,
            false => Others::Unasked,
        };
        Names {
            ids,
            others,
            quiet: ask_quiet && crate::quiet::enabled() && !fixed,
        }
    }

    /// The walker refused the loop with every other receiver feared: the
    /// names with the others cleared, and whether the facts clear them
    /// (`None`: nothing was feared, the refusal stands).
    pub(super) fn ask(&mut self, ctx: &FnCtx<'_>, arr_id: u32) -> Option<bool> {
        if self.others != Others::Feared {
            return None;
        }
        self.others = Others::Cleared;
        Some(named(ctx, arr_id))
    }

    /// The names as the facts leave them, for a walk that only reports.
    pub(super) fn asked(mut self, ctx: &FnCtx<'_>, arr_id: u32) -> Self {
        if self.others == Others::Feared && named(ctx, arr_id) {
            self.others = Others::Cleared;
        }
        self
    }

    /// A store whose receiver is local `id` may be a store into the
    /// guarded receiver.
    pub(super) fn local_may_be(&self, ctx: &FnCtx<'_>, arr_id: u32, id: u32) -> bool {
        self.others == Others::Feared || local_may_alias_guarded_array(ctx, arr_id, id, &self.ids)
    }

    /// A store whose receiver is `object`, which is no local that
    /// `local_may_be` the guarded receiver, may be a store into it all the
    /// same: any receiver when the others are feared; with the fact, an
    /// expression that hands the guarded local's value on (a sequence, an
    /// assignment), which the facts count as a read of its name.
    pub(super) fn store_may_reach(&self, ctx: &FnCtx<'_>, arr_id: u32, object: &Expr) -> bool {
        match self.others {
            Others::Unasked => false,
            Others::Feared => true,
            Others::Cleared => {
                !matches!(object, Expr::LocalGet(_))
                    && expr_may_resolve_to_guarded_array_alias(ctx, arr_id, object, &self.ids)
            }
        }
    }
}

/// A loop that keeps `arr`'s length by a fact: the consumption record,
/// and in check mode the test, in the loop's condition block, that the
/// length kept in `slot` is still the live one. `named`: the loop leans on
/// the named length; otherwise on its sites being quiet (`crate::quiet`:
/// program code run by one of them could have changed the length).
pub(super) fn witness(
    ctx: &mut FnCtx<'_>,
    arr: u32,
    slot: Option<&str>,
    named: bool,
) -> Result<()> {
    let fact = match named {
        true => crate::verify_facts::Fact::NamedLength,
        false => crate::verify_facts::Fact::QuietSites,
    };
    crate::facts_consumed::used();
    crate::verify_facts::covered(
        fact,
        || serde_json::json!({ "local": arr, "use": "length" }),
    );
    // A clone whose caller made the trip count has no slot; in check mode
    // it does not consume the fact (`lower_for_after_init_impl`).
    let Some(slot) = slot.filter(|_| crate::verify_facts::enabled()) else {
        return Ok(());
    };
    let kept = ctx.block().load(DOUBLE, slot);
    let arr_box = lower_expr(ctx, &Expr::LocalGet(arr))?;
    let live = ctx.block().call(
        DOUBLE,
        "js_value_length_property_f64",
        &[(DOUBLE, &arr_box)],
    );
    let same = ctx.block().fcmp("oeq", &live, &kept);
    let (miss, next) = match named {
        true => ("namedlen.verify_miss", "namedlen.admitted"),
        false => ("quiet.verify_miss", "quiet.admitted"),
    };
    let miss = ctx.new_block(miss);
    let next = ctx.new_block(next);
    let (miss_l, next_l) = (ctx.block_label(miss), ctx.block_label(next));
    crate::verify_facts::tick(ctx);
    ctx.block().cond_br(&same, &next_l, &miss_l);
    ctx.current_block = miss;
    let what = match named {
        true => format!("local {arr} changes its length through its own name only"),
        false => format!("a loop over local {arr} runs no program code at its sites"),
    };
    let site = crate::verify_facts::site_arg(ctx, &what, fact, &[]);
    let _ = crate::verify_facts::check(
        ctx,
        I32,
        "perry_wpfacts_check_miss",
        &[(DOUBLE, &arr_box)],
        &site,
    );
    ctx.block().unreachable();
    ctx.current_block = next;
    Ok(())
}
