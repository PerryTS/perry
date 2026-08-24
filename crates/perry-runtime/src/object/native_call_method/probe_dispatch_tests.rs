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

/// The `obj_type` `match` at the tail of `gc_pointer_and_type_from_value`,
/// mirrored with the byte it switches on supplied by the CALLER instead of read
/// from `addr - GC_HEADER_SIZE`.
///
/// #8728: reading that byte for a `Box`-leaked symbol reads memory this test
/// does not own — such a symbol has no `GcHeader`, so those bytes are allocator
/// metadata or a neighbour's tail. It also made the mirror non-deterministic,
/// because the `GC_TYPE_REGEXP` arm answers `true` *without looking at the
/// address at all*: whenever the stray byte happened to equal `GC_TYPE_REGEXP`
/// (20) the mirror reported "excluded" and the assertion below fired. In
/// isolation the byte is stable — 1000/1000 runs of this test alone passed — so
/// it only showed up once the rest of the suite had churned the allocator,
/// which is exactly the shape that reads as a flake.
///
/// Taking `obj_type` as a parameter lets the caller quantify over the WHOLE
/// domain of that byte rather than sample the one value that happens to be
/// there. That is sound, deterministic, and a strictly stronger statement than
/// the single-sample version it replaces.
///
/// Kept FAITHFUL on purpose: the `GC_TYPE_REGEXP` arm is a bare `true`, not
/// `regex::is_registered_regex(addr)`, because a bare `true` is what production
/// does — "RegExp has the dedicated `GC_TYPE_REGEXP` kind", so the header is
/// treated as authoritative and no registry is consulted. A predicate does
/// exist (`regex::is_registered_regex`), but the mirror must not call it: it
/// would prove something about a function this dispatch path never invokes,
/// and it reaches `try_read_gc_header`, i.e. the very `addr - 8` read this fix
/// removes.
fn excluded_by_the_header_arms(addr: usize, obj_type: u8) -> bool {
    match obj_type {
        crate::gc::GC_TYPE_SET => crate::set::is_registered_set(addr),
        crate::gc::GC_TYPE_MAP => crate::map::is_registered_map(addr),
        crate::gc::GC_TYPE_REGEXP => true,
        _ => false,
    }
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

    // Soundness, expressed as data rather than a switch: without the screen a
    // leaked symbol is classified by whatever `obj_type` the arms are handed,
    // and `ptr - 8` for such a symbol is allocator bytes that can read as ANY
    // value. So don't sample that byte (#8728 — that is what made this
    // assertion fail intermittently, and it read memory the test does not own).
    // Ask the mirror for the ENTIRE domain of the byte instead, and pin the
    // answer exactly.
    //
    // Exactly one value may exclude these symbols: `GC_TYPE_REGEXP`, whose arm
    // is a bare `true` and never consults the address — so its exclusion is an
    // accident of the production `match`, not evidence that anything recognises
    // a symbol. The other 255 fall through to `_ => false`, i.e. production
    // WOULD hand a leaked symbol back as an ordinary object. `may_be_symbol_header`
    // is the only thing standing between them and that, which is what makes the
    // `classify(sym).is_none()` assertions above non-vacuous.
    //
    // This fails in BOTH directions, and both are the right way round: an added
    // arm that covers leaked symbols grows the set, and giving the RegExp arm a
    // real predicate shrinks it. Either way the mirror must be re-derived from
    // `gc_pointer_and_type_from_value` rather than left to drift.
    for i in 0..8 {
        let sym = leaked_symbol(&format!("perry-7850-magic-{i}"));
        let excluding: Vec<u8> = (0..=u8::MAX)
            .filter(|&obj_type| excluded_by_the_header_arms(sym, obj_type))
            .collect();
        assert_eq!(
            excluding,
            vec![crate::gc::GC_TYPE_REGEXP],
            "leaked symbol {sym:#x}: the production `obj_type` match must exclude it for \
             EXACTLY the one byte value whose arm excludes unconditionally \
             (GC_TYPE_REGEXP), and for no other. Got {excluding:?}. More values ⇒ some \
             arm now recognises leaked symbols, so the magic screen is no longer the \
             only thing keeping them out and every `classify(sym).is_none()` above is \
             vacuous. Fewer ⇒ the production match changed and this mirror is stale."
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
