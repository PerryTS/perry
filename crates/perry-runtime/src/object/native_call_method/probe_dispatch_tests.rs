//! #7850: the header-directed probe dispatch in `gc_pointer_and_type_from_value`.
//!
//! Every dynamic method call goes through that function, and it used to run four
//! side-registry probes — `is_registered_set`, `is_registered_map`,
//! `is_regex_pointer`, `is_registered_symbol` — before reading the `GcHeader`
//! that already records the kind three of them were looking for. The symbol one
//! is the expensive one: a process-global `Mutex` plus a SipHash, entered on
//! every dispatch as soon as ANY `Symbol` exists, which one `for…of` makes true
//! (it materialises `Symbol.iterator`).
//!
//! These tests pin the two halves that a re-ordering can break:
//!
//! * **the saving is real** — a plain-object receiver must not move the symbol,
//!   map or set probe counters, *with the symbol latch armed*. A test that only
//!   checked "nothing threw" would pass with the whole optimisation deleted;
//!   this one goes red (case 4 of CLAUDE.md's "four ways a gate can be unable to
//!   fail").
//! * **the answer is unchanged** — Set, Map, RegExp, fresh `Symbol()` and
//!   `Box`-leaked (`Symbol.for` / well-known) receivers must all still be
//!   excluded, including when they are created AFTER the idle fast path has
//!   already answered for an unrelated address.
//!
//! The leaked symbols are the interesting case and the reason
//! `symbol::may_be_symbol_header` exists: they have no `GcHeader`, so `ptr - 8`
//! is foreign allocator bytes that can read as any `obj_type` at all. The screen
//! is therefore on the object's OWN first word (`SYMBOL_MAGIC`), not on the
//! header. `header_directed_dispatch_needs_the_symbol_magic_screen` sabotages it
//! and requires the classification to go WRONG, so a future edit that drops the
//! screen cannot leave these tests quietly green.
//!
//! Leaked symbols are minted through `Symbol.for` with a key unique to each
//! test rather than through the well-known cache: `WELL_KNOWN_SYMBOLS` is a
//! process-global cache while `SYMBOL_POINTERS` is `per_test_global!` (i.e. per
//! THREAD under `cargo test`), so a well-known symbol first created on another
//! test thread would come back cached and unregistered here. A unique key always
//! allocates and registers on the calling thread.

use super::*;

fn nanboxed(ptr: usize) -> f64 {
    f64::from_bits(crate::value::js_nanbox_pointer(ptr as i64).to_bits())
}

fn plain_object() -> usize {
    crate::object::js_object_alloc(0, 4) as usize
}

/// A `Box`-leaked symbol (no `GcHeader`), registered on THIS thread. Same
/// storage class as `Symbol.iterator` and the Intl fallback symbol.
fn leaked_symbol(key: &str) -> usize {
    let key_str = crate::string::js_string_from_str(key);
    let key_f64 = f64::from_bits(crate::value::js_nanbox_string(key_str as i64).to_bits());
    let addr = unsafe { crate::value::js_nanbox_get_pointer(crate::symbol::js_symbol_for(key_f64)) }
        as usize;
    assert!(addr != 0, "test premise: Symbol.for({key}) allocated");
    assert!(
        crate::symbol::is_registered_symbol(addr),
        "test premise: Symbol.for({key}) is registered on this thread"
    );
    addr
}

fn classify(addr: usize) -> Option<(*const u8, u8)> {
    unsafe { test_gc_pointer_and_type_from_value(nanboxed(addr)) }
}

/// The saving, asserted rather than assumed: with the symbol latch ARMED — the
/// state every realistic program is in — a plain-object dispatch must not enter
/// `is_registered_symbol` at all, nor the map/set registries.
///
/// Delete the `obj_type` dispatch and this goes red, because the probes run
/// again.
#[test]
fn plain_object_dispatch_probes_no_side_registry() {
    // Arm the latch the way ordinary code does.
    leaked_symbol("perry-7850-arm-the-latch");
    assert!(
        !crate::symbol::test_symbol_latch_is_idle(),
        "test premise: creating a symbol must arm SYMBOL_EVER_REGISTERED"
    );

    let obj = plain_object();
    // Warm any lazy state so the measured call below is steady-state.
    assert!(classify(obj).is_some());

    let sym_before = crate::symbol::test_symbol_registry_probe_count();
    let map_before = crate::map::test_map_registry_probe_count();
    let set_before = crate::set::test_set_registry_probe_count();

    let got = classify(obj);
    assert_eq!(
        got.map(|(_, t)| t),
        Some(crate::gc::GC_TYPE_OBJECT),
        "a plain object must classify as GC_TYPE_OBJECT"
    );

    assert_eq!(
        crate::symbol::test_symbol_registry_probe_count(),
        sym_before,
        "a plain-object dispatch must not take the process-global symbol \
         registry mutex — that was 6.5% of `pipeline` (#7850)"
    );
    assert_eq!(
        crate::map::test_map_registry_probe_count(),
        map_before,
        "GC_TYPE_OBJECT rules a Map out; the registry must not be consulted"
    );
    assert_eq!(
        crate::set::test_set_registry_probe_count(),
        set_before,
        "GC_TYPE_OBJECT rules a Set out; the registry must not be consulted"
    );
}

/// The answer, unchanged. Each of these kinds resolved to `None` before the
/// re-ordering and must still.
#[test]
fn exotic_receivers_are_still_excluded() {
    let set = crate::set::js_set_alloc(4) as usize;
    assert!(
        classify(set).is_none(),
        "a Set must not classify as an object"
    );

    let map = crate::map::js_map_alloc(4) as usize;
    assert!(
        classify(map).is_none(),
        "a Map must not classify as an object"
    );

    // Fresh `Symbol(desc)`: a `gc_malloc(_, GC_TYPE_STRING)` allocation, so the
    // header CAN speak for it — but only through the GC_TYPE_STRING arm.
    let fresh = unsafe {
        crate::value::js_nanbox_get_pointer(crate::symbol::js_symbol_new_empty()) as usize
    };
    assert!(fresh != 0, "test premise: Symbol() allocated");
    assert!(
        classify(fresh).is_none(),
        "a fresh Symbol must not classify as an object"
    );

    // A `Box`-leaked symbol created AFTER the idle fast path has already
    // answered for the unrelated addresses above (#7474 shape).
    let leaked = leaked_symbol("perry-7850-after-the-fast-path");
    assert!(
        classify(leaked).is_none(),
        "a leaked symbol created after the idle fast path must still be excluded"
    );

    // The realistic leaked-symbol path — what a `for…of` mints. It carries no
    // GcHeader, so only the magic screen can keep it out of the object arms.
    let wk = crate::symbol::well_known_symbol("iterator") as usize;
    assert!(
        unsafe { crate::symbol::may_be_symbol_header(wk as *const u8) },
        "a well-known symbol must carry SYMBOL_MAGIC in its first word; if it \
         does not, `Symbol.iterator.toString()` reads `ptr - 8` as a GcHeader"
    );
}

/// RegExp has its own GC kind, so it must be rejected without entering the
/// ordinary-object arm or consulting an `ObjectHeader` payload word.
#[cfg(feature = "regex-engine")]
#[test]
fn regexp_receiver_is_still_excluded() {
    let pattern = crate::string::js_string_from_str("a+b");
    let flags = crate::string::js_string_from_str("g");
    let re = crate::regex::js_regexp_new(pattern, flags) as usize;
    assert!(re != 0, "test premise: RegExp allocated");
    assert!(
        classify(re).is_none(),
        "a RegExp has GC_TYPE_REGEXP and must be excluded before ordinary-object \
         header reads"
    );
}

/// Sabotage. The `SYMBOL_MAGIC` screen is what makes the header-directed
/// dispatch sound; with it forced to "maybe", every dispatch pays the registry
/// again — and with it forced OFF entirely a leaked symbol's `ptr - 8` would be
/// read as if it were a real `GcHeader`. This test pins both directions:
/// screen ON ⟹ no probe for a plain object; screen defeated ⟹ the probe returns.
/// If it stops failing when sabotaged, the screen is not load-bearing and every
/// other assertion here is proving nothing.
#[test]
fn header_directed_dispatch_needs_the_symbol_magic_screen() {
    // Arm the latch, then confirm the screen is what suppresses the probe.
    leaked_symbol("perry-7850-magic-screen");
    let obj = plain_object();
    assert!(classify(obj).is_some());

    let before = crate::symbol::test_symbol_registry_probe_count();
    assert!(classify(obj).is_some());
    assert_eq!(
        crate::symbol::test_symbol_registry_probe_count(),
        before,
        "screen ON: a plain object must not reach the symbol registry"
    );

    let restore = crate::symbol::test_disable_symbol_magic_screen(true);
    let before = crate::symbol::test_symbol_registry_probe_count();
    let answer = classify(obj).map(|(_, t)| t);
    let probed = crate::symbol::test_symbol_registry_probe_count() > before;
    crate::symbol::test_disable_symbol_magic_screen(restore);

    assert!(
        probed,
        "sabotage check: with the magic screen defeated the dispatch MUST fall \
         through to `is_registered_symbol` — if it does not, the screen is not \
         what is keeping the fast path fast and this suite is vacuous"
    );
    assert_eq!(
        answer,
        Some(crate::gc::GC_TYPE_OBJECT),
        "the slow path must still give the same answer"
    );
}

/// Every `Box`-leaked symbol must carry `SYMBOL_MAGIC`, and no ordinary GC
/// object may — the first is soundness (a `false` here is a silent wrong
/// answer), the second is the performance invariant that keeps the fast path
/// firing. Both are cheap to check and both have been wrong in this family.
#[test]
fn the_magic_screen_covers_every_symbol_and_no_ordinary_object() {
    for i in 0..8 {
        let sym = leaked_symbol(&format!("perry-7850-magic-{i}"));
        assert!(
            unsafe { crate::symbol::may_be_symbol_header(sym as *const u8) },
            "leaked symbol {sym:#x} must carry SYMBOL_MAGIC"
        );
        assert!(classify(sym).is_none(), "leaked symbol {sym:#x} excluded");
    }
    let fresh = unsafe {
        crate::value::js_nanbox_get_pointer(crate::symbol::js_symbol_new_empty()) as usize
    };
    assert!(
        unsafe { crate::symbol::may_be_symbol_header(fresh as *const u8) },
        "a gc_malloc'd Symbol must carry SYMBOL_MAGIC too"
    );

    // Soundness sabotage, expressed as data rather than a switch: without the
    // screen a leaked symbol would be classified by the bytes at `ptr - 8`,
    // which belong to the allocator and not to us. This mirrors the production
    // `match` DELIBERATELY — it asserts that no other arm covers these, i.e.
    // that the screen is the only thing keeping them out. If someone adds an arm
    // that does cover them, this mirror goes stale and the assertion below
    // fails, which is the right way round.
    for i in 0..8 {
        let sym = leaked_symbol(&format!("perry-7850-magic-{i}"));
        let obj_type = unsafe {
            (*((sym as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader))
                .obj_type
        };
        let excluded_without_the_screen = match obj_type {
            crate::gc::GC_TYPE_SET => crate::set::is_registered_set(sym),
            crate::gc::GC_TYPE_MAP => crate::map::is_registered_map(sym),
            crate::gc::GC_TYPE_REGEXP => true,
            _ => false,
        };
        assert!(
            !excluded_without_the_screen,
            "leaked symbol {sym:#x} (allocator bytes read as obj_type {obj_type}) would \
             be excluded even without the magic screen — the screen is then not \
             load-bearing and this suite is vacuous"
        );
    }

    let mut covered = 0usize;
    for _ in 0..64 {
        let o = plain_object();
        if unsafe { crate::symbol::may_be_symbol_header(o as *const u8) } {
            covered += 1;
        }
    }
    assert_eq!(
        covered, 0,
        "{covered}/64 fresh GC objects read as SYMBOL_MAGIC; the #7850 fast path \
         is not firing and the symbol registry mutex is back on every dispatch"
    );
}
