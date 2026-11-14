//! The verdict of a by-name method site (wpfacts `method_sites`).
//!
//! A method call whose receiver's STATIC type names a built-in kind is
//! lowered to that kind's body by name (`lower_array_method`, the folded
//! `Expr::Array*` nodes). The annotation is no proof of the value: behind
//! `a: Float64Array` there may be an ordinary object with its own `fill`, a
//! class instance, a subclass instance, `null`. The whole-program facts
//! judge each site by the values its receiver holds; this module is how
//! codegen asks, with the key both sides compute
//! (`perry_hir::folded_call::Site` and the method name).
//!
//! Typed arrays: a site whose receiver is annotated a typed array keeps its
//! typed-array lowering only when the facts prove the receiver holds typed
//! arrays the realm bore and nothing else
//! (`perry_hir::folded_call::SITE_TYPED_ARRAY`). Every other such site is
//! the ordinary method call: the real `Get` and `Call`, which runs an own or
//! inherited method of any receiver and throws for a missing member or a
//! `null` / `undefined` receiver. The fact says nothing about the member (an
//! own property or a prototype write on a real typed array is not seen by
//! it).
//!
//! `PERRY_FACTS_TYPED_ARRAY_SITES=0` trusts the annotation, as before.
//!
//! Arrays, the rule: a by-name Array method site runs the built-in with no
//! test of its receiver exactly when the site's whole-program verdict is
//! `NativeMember` over the Array family
//! (`perry_hir::folded_call::SITE_ARRAY_MEMBER`: every receiver value an
//! Array exotic object, the member the realm's on each). Every other site
//! whose receiver is annotated an Array is the guarded tier: one admission
//! test of the receiver's header, whose miss is the ordinary method call.
//!
//! `PERRY_FACTS_ARRAY_METHOD_SITES=0` guards every site at run time, as
//! before.

use crate::expr::FnCtx;
use perry_hir::folded_call::{Site, SITE_ARRAY_MEMBER, SITE_TYPED_ARRAY};
use perry_hir::Expr;

/// `PERRY_FACTS_TYPED_ARRAY_SITES=0` (`off`, `false`) keeps the typed-array
/// lowering of every annotated receiver. Read per call; a compile-cache
/// input.
pub(crate) fn typed_array_sites_enabled() -> bool {
    !matches!(
        std::env::var("PERRY_FACTS_TYPED_ARRAY_SITES").as_deref(),
        Ok("0") | Ok("off") | Ok("false")
    )
}

/// Does the STATIC type of `object` select the typed-array flavour of the
/// by-name Array lowering? The ten element-typed kinds `is_array_expr`
/// admits; `Uint8Array` and `Uint8ClampedArray` are dispatched on the value
/// by the runtime and are not lowered from their annotation.
pub(crate) fn annotated_typed_array(ctx: &FnCtx<'_>, object: &Expr) -> bool {
    crate::type_analysis::is_typed_array_expr(ctx, object)
        && crate::type_analysis::is_array_expr(ctx, object)
}

/// Do the facts prove that the receiver of the method site `site` named
/// `name` holds only typed arrays? No site, no facts or no record: no.
pub(crate) fn typed_array_receiver_proven(ctx: &FnCtx<'_>, site: Option<Site>, name: &str) -> bool {
    let Some(site) = site else {
        return false;
    };
    !ctx.wpfacts_off
        && ctx
            .wpfacts
            .method_sites
            .get(&(site, name.to_string()))
            .is_some_and(|flags| flags & SITE_TYPED_ARRAY != 0)
}

/// Is the method call `object.name(..)` at `site` one whose receiver is
/// annotated a typed array and not proven to hold one? Such a call is the
/// ordinary method call.
///
/// A proven site keeps its lowering and, in check mode, tests its claim on
/// the receiver value first ([`witness_typed_array`]). The test needs the
/// value before the lowering evaluates it, so check mode consumes the fact
/// only for a receiver that is read again for free (a local, `this`); any
/// other receiver is the ordinary call there.
pub(crate) fn unproven_typed_array_call(
    ctx: &mut FnCtx<'_>,
    site: Option<Site>,
    object: &Expr,
    name: &str,
) -> anyhow::Result<bool> {
    if !typed_array_sites_enabled() || !annotated_typed_array(ctx, object) {
        return Ok(false);
    }
    let witnessed =
        !crate::verify_facts::enabled() || matches!(object, Expr::LocalGet(_) | Expr::This);
    // `%TypedArray%.prototype` has no `concat`: the Array arm would run
    // `Array.prototype.concat` on a typed array, where the real `Get` finds
    // nothing and the call throws.
    let proven = witnessed && name != "concat" && typed_array_receiver_proven(ctx, site, name);
    if std::env::var("PERRY_METHOD_SITE_DIAG").as_deref() == Ok("1") {
        eprintln!(
            "[perry method_site] fn={} kind=typed_array key={name} verdict={}",
            ctx.func.name,
            if proven { "proven" } else { "ordinary-call" }
        );
    }
    if proven {
        witness_typed_array(ctx, object, name)?;
    }
    Ok(!proven)
}

/// A site that keeps its typed-array lowering by the facts: the consumption
/// record, and in check mode the test that the receiver value is a typed
/// array ("method site").
fn witness_typed_array(ctx: &mut FnCtx<'_>, object: &Expr, name: &str) -> anyhow::Result<()> {
    use crate::types::{DOUBLE, I32, I64};
    crate::facts_consumed::used();
    crate::verify_facts::covered(
        crate::verify_facts::Fact::MethodSite,
        || serde_json::json!({ "key": name, "receiver": "typed array" }),
    );
    if !crate::verify_facts::enabled() {
        return Ok(());
    }
    let recv = crate::expr::lower_expr(ctx, object)?;
    let answer = ctx
        .block()
        .call(DOUBLE, "js_util_types_is_typed_array", &[(DOUBLE, &recv)]);
    let bits = ctx.block().bitcast_double_to_i64(&answer);
    let holds = ctx.block().icmp_eq(I64, &bits, crate::nanbox::TAG_TRUE_I64);
    let miss = ctx.new_block("methodsite.verify_miss");
    let next = ctx.new_block("methodsite.admitted");
    let (miss_l, next_l) = (ctx.block_label(miss), ctx.block_label(next));
    crate::verify_facts::tick(ctx);
    ctx.block().cond_br(&holds, &next_l, &miss_l);
    ctx.current_block = miss;
    let what = format!("the receiver of .{name}() holds only typed arrays");
    let site =
        crate::verify_facts::site_arg(ctx, &what, crate::verify_facts::Fact::MethodSite, &[]);
    let _ = crate::verify_facts::check(
        ctx,
        I32,
        "perry_wpfacts_check_miss",
        &[(DOUBLE, &recv)],
        &site,
    );
    ctx.block().unreachable();
    ctx.current_block = next;
    Ok(())
}

/// `PERRY_FACTS_ARRAY_METHOD_SITES=0` (`off`, `false`) keeps the run-time
/// guard of every by-name Array method site. Read per call; a compile-cache
/// input.
pub(crate) fn array_sites_enabled() -> bool {
    !matches!(
        std::env::var("PERRY_FACTS_ARRAY_METHOD_SITES").as_deref(),
        Ok("0") | Ok("off") | Ok("false")
    )
}

fn diag_on() -> bool {
    std::env::var("PERRY_METHOD_SITE_DIAG").as_deref() == Ok("1")
}

/// Do the facts prove that the by-name Array method site `site` named
/// `name` runs the realm's `Array.prototype` built-in on an Array, whatever
/// value reaches it? No site, no facts or no record: no (the guarded tier).
pub(crate) fn array_member_proven(ctx: &FnCtx<'_>, site: Option<Site>, name: &str) -> bool {
    let proven = array_sites_enabled()
        && !ctx.wpfacts_off
        && site.is_some_and(|site| {
            ctx.wpfacts
                .method_sites
                .get(&(site, name.to_string()))
                .is_some_and(|flags| flags & SITE_ARRAY_MEMBER != 0)
        });
    census(ctx, site, name, if proven { "proven" } else { "guarded" });
    proven
}

/// One `[perry method_site]` line per by-name Array method site
/// (`PERRY_METHOD_SITE_DIAG=1`): its key and what the lowering does.
pub(crate) fn census(ctx: &FnCtx<'_>, site: Option<Site>, name: &str, verdict: &str) {
    if diag_on() {
        let site = match site {
            Some(Site::Callee(o)) => format!("callee:{o}"),
            Some(Site::Recv(o)) => format!("recv:{o}"),
            Some(Site::Local(id)) => format!("local:{id}"),
            None => "none".to_string(),
        };
        eprintln!(
            "[perry method_site] fn={} kind=array key={name} site={site} verdict={verdict}",
            ctx.func.name
        );
    }
}

/// The verdict of an inline-tier site (`Expr::ArrayPush`, `Expr::ArrayPop`),
/// for the census only: the tier's admission tests are its own layout
/// assumptions (a named layout holds element descriptors and a non-writable
/// `length`; the admission byte an index on the default chain) and stay.
pub(crate) fn census_inline_tier(ctx: &FnCtx<'_>, site: Option<Site>, name: &str) {
    if diag_on() {
        let proven = !ctx.wpfacts_off
            && site.is_some_and(|site| {
                ctx.wpfacts
                    .method_sites
                    .get(&(site, name.to_string()))
                    .is_some_and(|flags| flags & SITE_ARRAY_MEMBER != 0)
            });
        census(
            ctx,
            site,
            name,
            if proven {
                "inline-tier:proven"
            } else {
                "inline-tier:unproven"
            },
        );
    }
}

/// `PERRY_ANNOTATION_RECEIVER_TESTS=0` (`off`, `false`) restores the inline
/// tiers that read an annotated receiver's header with no test that the
/// receiver is a heap pointer at all (the UNSAFE lowering of main before this
/// switch: `function f(a: number[]) { return a.push(7); } f(null as any)` read
/// the header below address 2 and died with SIGSEGV). Read per call; a
/// compile-cache input. It exists to measure the test's cost on one build.
pub(crate) fn annotation_receiver_tests_enabled() -> bool {
    !matches!(
        std::env::var("PERRY_ANNOTATION_RECEIVER_TESTS").as_deref(),
        Ok("0") | Ok("off") | Ok("false")
    )
}

/// Must an inline Array tier (`Expr::ArrayPush`, ...) whose receiver is the
/// local `array_id` test that the receiver is a heap pointer before it reads
/// a header behind it? The tier was chosen from the receiver's ANNOTATION,
/// which proves nothing about the value: `null`, a Number or a String may
/// stand behind `number[]`. No test where the whole-program verdict of the
/// site proves every receiver an Array (`SITE_ARRAY_MEMBER`: an Array is a
/// heap pointer), or where the function's own birth analysis did (an owned
/// local, `collectors::loop_pushed_arrays`: born from an Array literal here
/// and never rebound), or with the switch off. The test is the one band
/// compare (`HEAP_POINTER_BAND_*`); its miss is the ordinary `Get` + `Call`.
///
/// No check-mode witness: the facts drop a RUN-TIME test whose absence the
/// header read itself would expose, and the header admission that follows
/// still tests the kind.
pub(crate) fn inline_tier_needs_pointer_test(
    ctx: &FnCtx<'_>,
    array_id: u32,
    owned: bool,
    name: &str,
) -> bool {
    if !annotation_receiver_tests_enabled() || owned {
        return false;
    }
    let proven = !ctx.wpfacts_off
        && ctx
            .wpfacts
            .method_sites
            .get(&(Site::Local(array_id), name.to_string()))
            .is_some_and(|flags| flags & SITE_ARRAY_MEMBER != 0);
    if proven {
        crate::facts_consumed::used();
    }
    if diag_on() {
        eprintln!(
            "[perry inline_receiver] fn={} key={name} local={array_id} test={}",
            ctx.func.name,
            if proven { "dropped:proven" } else { "band" }
        );
    }
    !proven
}

/// A site that drops its run-time guard by the facts: the consumption
/// record, and in check mode the dropped test on the receiver value `recv`
/// (an Array that resolves `name` to the built-in), "method site".
pub(crate) fn witness_array_member(ctx: &mut FnCtx<'_>, recv: &str, name: &str) {
    use crate::types::{DOUBLE, I32};
    crate::facts_consumed::used();
    crate::verify_facts::covered(
        crate::verify_facts::Fact::MethodSite,
        || serde_json::json!({ "key": name, "receiver": "array member" }),
    );
    if !crate::verify_facts::enabled() {
        return;
    }
    let idx = ctx.strings.intern(name);
    let key_handle = format!("@{}", ctx.strings.entry(idx).handle_global);
    let blk = ctx.block();
    let key = blk.load(DOUBLE, &key_handle);
    let member = blk.call(
        I32,
        "js_array_member_witness",
        &[(DOUBLE, recv), (DOUBLE, &key)],
    );
    let holds = blk.icmp_ne(I32, &member, "0");
    let miss = ctx.new_block("methodsite.verify_miss");
    let next = ctx.new_block("methodsite.admitted");
    let (miss_l, next_l) = (ctx.block_label(miss), ctx.block_label(next));
    crate::verify_facts::tick(ctx);
    ctx.block().cond_br(&holds, &next_l, &miss_l);
    ctx.current_block = miss;
    let what = format!("the receiver of .{name}() is an Array whose {name} is the built-in");
    let site =
        crate::verify_facts::site_arg(ctx, &what, crate::verify_facts::Fact::MethodSite, &[]);
    let _ = crate::verify_facts::check(
        ctx,
        I32,
        "perry_wpfacts_check_miss",
        &[(DOUBLE, recv)],
        &site,
    );
    ctx.block().unreachable();
    ctx.current_block = next;
}
