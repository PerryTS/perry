//! The receiver brand test of a folded `Map` / `Set` method node.
//!
//! HIR folds `m.get(k)`, `m.set(k, v)`, `m.has(k)`, `m.delete(k)`,
//! `s.add(v)`, `s.has(v)`, `s.delete(v)` from the receiver's DECLARED type,
//! and the node's arm runs the collection built-in on the receiver's
//! payload. A declaration is no runtime fact: an Array, a Date, a plain
//! object, a typed array or the other collection behind `Map<K, V>` reached
//! the built-in, whose entry (`map::clean_map_ptr`) refused the cell and
//! answered `undefined` / `false` where node performs the real `Get` +
//! `Call` (a TypeError `m.get is not a function`, or the receiver's own
//! member: a Map behind `Set<T>` answers `s.has(v)` with `Map.prototype.has`).
//!
//! The rule of the inline tiers (#14575, #14577): a fast tier that assumes
//! the receiver's kind runs only where the facts prove it or behind the
//! tier's own band test; a miss is the ordinary `Get` + `Call`. A site whose
//! receiver the facts prove holds only realm-born Maps (resp. Sets)
//! (wpfacts `method_sites`, `SITE_MAP` / `SITE_SET`), or a receiver the
//! compiler constructed itself (`collection_receiver::constructed_kind`),
//! keeps its exact code. Every other site first tests the receiver: a heap
//! pointer whose GC type byte is the collection's goes on to the
//! own-override guard as before; a heap pointer of another kind, a runtime-id
//! object (a Proxy) and every primitive take the generic arm. An untagged raw
//! word and a JS handle keep today's path (the runtime entries refuse them
//! themselves). `PERRY_COLLECTION_RECEIVER_TESTS=0` keeps today's lowering.
//!
//! The same test guards the Map / Set calls the HIR leaves unfolded (a
//! receiver that is no plain local: `this.k.get(x)`, `h.m.forEach(f)`; the
//! own-override guard of `lower_call/property_get`) and the `size` read
//! (`collection_receiver::lower_collection_size`), whose miss is the ordinary
//! `Get` of `size`. `PERRY_COLLECTION_UNFOLDED_TESTS=0` keeps those as they
//! were.

use crate::expr::FnCtx;
use crate::types::{I1, I64, I8};
use perry_hir::folded_call::{Site, SITE_MAP, SITE_SET};
use perry_hir::Expr;

/// `PERRY_COLLECTION_RECEIVER_TESTS=0` (`off`, `false`) keeps every folded
/// Map / Set node's arm without a receiver test. Read per call; a
/// compile-cache input.
pub(crate) fn enabled() -> bool {
    !matches!(
        std::env::var("PERRY_COLLECTION_RECEIVER_TESTS").as_deref(),
        Ok("0") | Ok("off") | Ok("false")
    )
}

/// `PERRY_COLLECTION_UNFOLDED_TESTS=0` (`off`, `false`) keeps the Map / Set
/// method calls the HIR did not fold (`this.k.get(x)`, `h.m.forEach(f)`: the
/// own-override guard of `lower_call/property_get`) and the `size` read of a
/// Map / Set receiver without a receiver test. Read per call; a compile-cache
/// input.
pub(crate) fn unfolded_enabled() -> bool {
    !matches!(
        std::env::var("PERRY_COLLECTION_UNFOLDED_TESTS").as_deref(),
        Ok("0") | Ok("off") | Ok("false")
    )
}

/// [`needed`] for an unfolded method call (`site` is its callee's site) or a
/// `size` read (`site` `None`: no fact covers a property read).
pub(crate) fn needed_unfolded(
    ctx: &FnCtx<'_>,
    family: crate::builtin_proto::BuiltinProto,
    site: Option<Site>,
    receiver: &Expr,
    method: &str,
) -> Option<(Brand, bool)> {
    if !unfolded_enabled() {
        return None;
    }
    needed(ctx, family, site, receiver, method)
}

/// A folded collection node's receiver kind: its GC type byte, its facts
/// bit, its `constructed_kind`.
#[derive(Clone, Copy)]
pub(crate) struct Brand {
    gc_type: &'static str,
    site_bit: u8,
    kind: super::collection_receiver::ConstructedKind,
    what: &'static str,
}

const MAP: Brand = Brand {
    gc_type: "8",
    site_bit: SITE_MAP,
    kind: super::collection_receiver::ConstructedKind::Map,
    what: "Map",
};
const SET: Brand = Brand {
    gc_type: "12",
    site_bit: SITE_SET,
    kind: super::collection_receiver::ConstructedKind::Set,
    what: "Set",
};

/// What a folded node of `family` needs before its own-override guard:
/// `None` (no test: not a collection node, the switch is off, or the
/// receiver is proven), or the brand to test. A site the facts prove is
/// recorded as consumed; in check mode it is tested too, a miss reporting
/// the false fact ([`emit_brand_branch`] with `check`).
pub(crate) fn needed(
    ctx: &FnCtx<'_>,
    family: crate::builtin_proto::BuiltinProto,
    site: Option<Site>,
    receiver: &Expr,
    method: &str,
) -> Option<(Brand, bool)> {
    let brand = match family {
        crate::builtin_proto::BuiltinProto::Map => MAP,
        crate::builtin_proto::BuiltinProto::Set => SET,
        _ => return None,
    };
    if !enabled() || super::collection_receiver::constructed_kind(ctx, receiver) == Some(brand.kind)
    {
        return None;
    }
    let proven = !ctx.wpfacts_off
        && site.is_some_and(|site| {
            ctx.wpfacts
                .method_sites
                .get(&(site, method.to_string()))
                .is_some_and(|flags| flags & brand.site_bit != 0)
        });
    if std::env::var("PERRY_METHOD_SITE_DIAG").as_deref() == Ok("1") {
        eprintln!(
            "[perry method_site] fn={} kind={} key={method} verdict={}",
            ctx.func.name,
            brand.what,
            if proven { "proven" } else { "brand-test" }
        );
    }
    if proven {
        crate::facts_consumed::used();
        crate::verify_facts::covered(
            crate::verify_facts::Fact::MethodSite,
            || serde_json::json!({ "key": method, "receiver": brand.what }),
        );
        if !crate::verify_facts::enabled() {
            return None;
        }
    }
    Some((brand, proven))
}

/// Branch on the receiver `recv` (boxed): to `ok_label` for a heap pointer
/// whose GC type byte is `brand`'s, an untagged raw word or a JS handle; to
/// `miss_label` otherwise. With `check` (a proven site in check mode) a miss
/// reports the false fact instead and `miss_label` is not used.
pub(crate) fn emit_brand_branch(
    ctx: &mut FnCtx<'_>,
    brand: Brand,
    check: bool,
    method: &str,
    recv: &str,
    ok_label: &str,
    miss_label: &str,
) {
    let heap_idx = ctx.new_block("collbrand.heap");
    let other_idx = ctx.new_block("collbrand.other");
    let heap_label = ctx.block_label(heap_idx);
    let other_label = ctx.block_label(other_idx);
    if check {
        crate::verify_facts::tick(ctx);
    }
    let report = check.then(|| ctx.new_block("collbrand.verify_miss"));
    let miss_label = match report {
        Some(idx) => ctx.block_label(idx),
        None => miss_label.to_string(),
    };
    super::array_push_own::emit_heap_pointer_branch(ctx, recv, &heap_label, &other_label);

    ctx.current_block = heap_idx;
    {
        let blk = ctx.block();
        let bits = blk.bitcast_double_to_i64(recv);
        let handle = blk.and(I64, &bits, crate::nanbox::POINTER_MASK_I64);
        let type_addr = blk.sub(I64, &handle, "8");
        let type_ptr = blk.inttoptr(I64, &type_addr);
        let obj_type = blk.load(I8, &type_ptr);
        let is_kind = blk.icmp_eq(I8, &obj_type, brand.gc_type);
        blk.cond_br(&is_kind, ok_label, &miss_label);
    }

    ctx.current_block = other_idx;
    {
        let blk = ctx.block();
        let bits = blk.bitcast_double_to_i64(recv);
        let top16 = blk.lshr(I64, &bits, "48");
        let raw = blk.icmp_eq(I64, &top16, "0");
        let js_handle = blk.icmp_eq(I64, &top16, "32763");
        let keep = blk.or(I1, &raw, &js_handle);
        blk.cond_br(&keep, ok_label, &miss_label);
    }

    if let Some(idx) = report {
        ctx.current_block = idx;
        let what = format!("the receiver of .{method}() holds only {}s", brand.what);
        let site =
            crate::verify_facts::site_arg(ctx, &what, crate::verify_facts::Fact::MethodSite, &[]);
        let _ = crate::verify_facts::check(
            ctx,
            crate::types::I32,
            "perry_wpfacts_check_miss",
            &[(crate::types::DOUBLE, recv)],
            &site,
        );
        ctx.block().unreachable();
    }
}
