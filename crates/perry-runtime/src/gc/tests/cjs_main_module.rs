//! #10735 witness: `scan_cjs_main_module_root_mut`'s rewrite counter is a
//! diagnostic nothing else reads, which is exactly the kind of thing that
//! rots silently -- a future change that stops rewriting the cache would
//! report `total_rewrites=0` forever while looking perfectly healthy. This
//! test forces a REAL evacuating collection (not a passive scan that never
//! sees a move) and asserts both that the placeholder's address actually
//! changed and that the counter tracked it, per CLAUDE.md's "a gate must
//! assert its subject was live" rule.
//!
//! Lives here rather than in `module_require.rs` because a real evacuating
//! minor needs `CopyingNurseryTestGuard`'s preflight setup (generated write
//! barriers reporting "active", the conservative-full-scan test default
//! turned off, a clean shadow stack / remembered set) -- machinery private
//! to this `gc::tests` tree. That guard also clears the thread's mutable
//! scanner registry so unrelated GC tests see only the roots they install,
//! which would remove the very scanner under test, so this file
//! re-registers it explicitly after constructing the guard.

use super::super::*;
use super::support::CopyingNurseryTestGuard;

#[test]
fn placeholder_move_under_forced_evacuation_increments_the_rewrite_counter() {
    let _nursery = CopyingNurseryTestGuard::new(0);
    let _evac = knob_overrides::ForcedEvacuationTestGuard::on();
    let _diag = GcDiagTestGuard::force_on();
    // `CopyingNurseryTestGuard::new` clears the thread's scanner registry so
    // this collection sees exactly the roots the test installs -- put the
    // one under test back.
    gc_register_mutable_root_scanner(crate::module_require::scan_cjs_main_module_root_mut);

    crate::module_require::js_bootstrap_cjs_main_module_placeholder();
    let before_bits =
        crate::module_require::test_cjs_main_module_bits().expect("placeholder must be published");
    let rewrites_before = crate::module_require::test_cjs_main_module_rewrite_count();

    js_gc_collect();

    let after_bits = crate::module_require::test_cjs_main_module_bits()
        .expect("placeholder must survive the collection");
    let rewrites_after = crate::module_require::test_cjs_main_module_rewrite_count();

    assert_ne!(
        before_bits, after_bits,
        "forced evacuation must have moved the placeholder -- if the \
         address is unchanged, this test's premise (a real move happened) \
         is false, and the counter assertion below would be checking \
         nothing (before={before_bits:#x} after={after_bits:#x})"
    );
    assert!(
        rewrites_after > rewrites_before,
        "scan_cjs_main_module_root_mut must have counted the rewrite \
         (before={rewrites_before} after={rewrites_after}); a zero delta \
         here means the cache stopped following the object it caches -- \
         the exact regression #10735's identity guarantee depends on never \
         happening"
    );
}
