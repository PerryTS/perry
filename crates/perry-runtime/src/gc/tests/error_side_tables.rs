//! Error side-table GC integration (2026-07-02 audit, GC deep set).
//!
//! Node diagnostics live in one record keyed by the movable ErrorHeader and
//! follow its `ErrorSideTables` move/finalize hook. User-assigned properties
//! live on `ObjectMeta.expando` and follow ordinary heap tracing.

use super::super::*;
use super::support::*;

fn error_bits(err: usize) -> u64 {
    ptr_bits(err)
}

/// A registered Node diagnostic belongs to the Error, not to the address of
/// its movable message string. Force both cells through copied-minor so this
/// cannot pass when the collector happens to leave the message in place.
#[test]
fn test_error_code_survives_message_and_error_move() {
    let _guard = CopyingNurseryTestGuard::new(1);

    let message = crate::string::js_string_from_bytes(b"boom".as_ptr(), 4);
    crate::node_submodules::register_error_code_pub(message, "ERR_TEST_9530");
    crate::node_submodules::register_error_syscall(message, "open");
    crate::node_submodules::register_error_errno(message, -2);
    crate::node_submodules::register_error_path(message, "/missing/source".to_string());
    crate::node_submodules::register_error_dest(message, "/missing/dest".to_string());
    crate::node_submodules::register_error_hostname(message, "missing.test".to_string());
    let error = crate::error::js_error_new_with_message(message);
    let before_error = error as usize;
    let before_message = unsafe { (*error).message as usize };
    js_shadow_slot_set(0, error_bits(before_error));

    let _ = gc_collect_minor();

    let after_error = (js_shadow_slot_get(0) & POINTER_MASK) as usize;
    let after_message =
        unsafe { (*(after_error as *const crate::error::ErrorHeader)).message as usize };
    assert_ne!(
        after_error, before_error,
        "test premise: the error must move"
    );
    assert_ne!(
        after_message, before_message,
        "test premise: the message must move"
    );
    assert_eq!(
        crate::node_submodules::error_code_for_error(
            after_error as *const crate::error::ErrorHeader,
        ),
        Some("ERR_TEST_9530"),
        "the moved error must retain its registered Node code"
    );
    let moved_error = after_error as *const crate::error::ErrorHeader;
    assert_eq!(
        crate::node_submodules::error_syscall_for_error(moved_error),
        Some("open")
    );
    assert_eq!(
        crate::node_submodules::error_errno_for_error(moved_error),
        Some(-2)
    );
    assert_eq!(
        crate::node_submodules::error_path_for_error(moved_error).as_deref(),
        Some("/missing/source")
    );
    assert_eq!(
        crate::node_submodules::error_dest_for_error(moved_error).as_deref(),
        Some("/missing/dest")
    );
    assert_eq!(
        crate::node_submodules::error_hostname_for_error(moved_error).as_deref(),
        Some("missing.test")
    );
    assert!(
        crate::node_submodules::error_code_for_error(
            before_error as *const crate::error::ErrorHeader,
        )
        .is_none(),
        "the stale pre-move owner key must be gone"
    );

    // Exercise the actual `err.code` property path, not only the table API.
    // Suppress new automatic collections after the explicit positive-control
    // move so the raw test pointer remains current while the getter allocates
    // its result string and the report materialises `.stack`.
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let code_key = crate::string::js_string_from_bytes(b"code".as_ptr(), 4);
    let code = crate::object::js_object_get_field_by_name(
        moved_error.cast::<crate::object::ObjectHeader>(),
        code_key,
    );
    unsafe { assert_string_bytes(code.as_string_ptr(), b"ERR_TEST_9530") };

    let report = unsafe { crate::exception::uncaught_native_error_report(moved_error.cast_mut()) };
    let (head, frames) = report
        .split_once('\n')
        .unwrap_or_else(|| panic!("the report must carry frames; got {report:?}"));
    assert_eq!(head, "Error [ERR_TEST_9530]: boom");
    assert!(
        frames.contains("    at "),
        "report lost its frames: {report:?}"
    );

    crate::node_submodules::diagnostics_gc::error_side_tables_clear_dead(after_error);
}

/// A real ENOENT Error keeps its enumerable Node fields through relocation and
/// `util.inspect`, including the path that applications commonly log.
#[test]
fn test_moved_fs_enoent_round_trips_through_util_inspect() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let path = "/perry-9530-definitely-missing";
    let io_error = std::io::Error::from_raw_os_error(libc::ENOENT);
    let error = unsafe { crate::fs::build_fs_error_value(&io_error, "open", path) };
    let before_error = (error.to_bits() & POINTER_MASK) as usize;
    let before_message =
        unsafe { (*(before_error as *const crate::error::ErrorHeader)).message as usize };
    js_shadow_slot_set(0, error.to_bits());

    let _ = gc_collect_minor();

    let after_bits = js_shadow_slot_get(0);
    let after_error = (after_bits & POINTER_MASK) as usize;
    let after_message =
        unsafe { (*(after_error as *const crate::error::ErrorHeader)).message as usize };
    assert_ne!(
        after_error, before_error,
        "test premise: the fs Error must move"
    );
    assert_ne!(
        after_message, before_message,
        "test premise: the fs Error message must move"
    );

    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let inspected = crate::builtins::js_util_inspect(
        f64::from_bits(after_bits),
        f64::from_bits(crate::value::TAG_UNDEFINED),
    );
    let inspected = crate::value::JSValue::from_bits(inspected.to_bits());
    let text = unsafe {
        let ptr = inspected.as_string_ptr();
        let bytes = std::slice::from_raw_parts(
            (ptr as *const u8).add(std::mem::size_of::<crate::StringHeader>()),
            (*ptr).byte_len as usize,
        );
        String::from_utf8_lossy(bytes).into_owned()
    };
    assert!(text.contains("code: 'ENOENT'"), "missing ENOENT: {text}");
    assert!(
        text.contains("path: '/perry-9530-definitely-missing'"),
        "missing path: {text}"
    );
}

/// A rooted error that MOVES in a copied-minor must keep its user props —
/// the side-table entry is rekeyed to the new address by the move hook.
#[test]
fn test_error_user_props_survive_copied_minor_move() {
    let _guard = CopyingNurseryTestGuard::new(1);

    let err = crate::error::js_error_new() as usize;
    assert!(crate::arena::pointer_in_nursery(err));
    crate::node_submodules::diagnostics::set_error_user_prop(
        err,
        "code",
        f64::from_bits(crate::value::TAG_TRUE),
    );
    js_shadow_slot_set(0, error_bits(err));

    let _ = gc_collect_minor();

    let moved = (js_shadow_slot_get(0) & POINTER_MASK) as usize;
    assert_ne!(moved, err, "test premise: the error must actually move");
    let prop = crate::node_submodules::diagnostics::error_user_prop(moved, "code");
    assert_eq!(
        prop.map(f64::to_bits),
        Some(crate::value::TAG_TRUE),
        "user prop must be readable at the error's post-move address \
         (the side tables were keyed by the stale pre-move address)"
    );
    assert!(
        crate::node_submodules::diagnostics::error_user_prop(err, "code").is_none(),
        "the stale pre-move key must be gone (a recycled address would \
         inherit it otherwise)"
    );
}

/// A DEAD error's side-table entries must be dropped by the copied-minor
/// from-space finalize so a fresh error at the recycled address doesn't
/// inherit them.
#[test]
fn test_dead_error_side_table_entries_cleared() {
    let _guard = CopyingNurseryTestGuard::new(1);

    let message = crate::string::js_string_from_bytes(b"dead".as_ptr(), 4);
    crate::node_submodules::register_error_code_pub(message, "ERR_DEAD_9530");
    let err = crate::error::js_error_new_with_message(message) as usize;
    crate::node_submodules::diagnostics::set_error_user_prop(
        err,
        "inherited",
        f64::from_bits(crate::value::TAG_TRUE),
    );
    // Not rooted: dead at the first minor.
    js_shadow_slot_set(0, 0);

    let _ = gc_collect_minor();

    assert!(
        crate::node_submodules::error_code_for_error(err as *const crate::error::ErrorHeader,)
            .is_none(),
        "dead error's diagnostic record must be cleared, not left for a \
         fresh error at the recycled address to inherit"
    );
    assert!(
        crate::node_submodules::diagnostics::error_user_prop(err, "inherited").is_none(),
        "dead error's user-prop entry must be cleared, not left for a \
         fresh error at the recycled address to inherit"
    );
}

/// An OBJECT-valued user prop must keep its referent alive across a
/// copied-minor and must read back at the referent's moved address.
#[test]
fn test_object_valued_user_prop_is_a_gc_root_and_rewrites() {
    let _guard = CopyingNurseryTestGuard::new(1);
    // #6759 phase 1: no scanner to re-register any more. The prop's referent
    // now hangs off the error's `ObjectMeta.expando` bag, so it is kept alive
    // and rewritten by ORDINARY object tracing rather than by a bespoke
    // mutable-root scanner over an address-keyed table. The guarantee this
    // test asserts is unchanged; the mechanism providing it is simpler.

    let err = crate::error::js_error_new() as usize;
    js_shadow_slot_set(0, error_bits(err));

    // The prop's object is reachable ONLY through the error's metadata bag.
    let cause = crate::object::js_object_alloc(0, 0);
    crate::node_submodules::diagnostics::set_error_user_prop(
        err,
        "cause",
        f64::from_bits(ptr_bits(cause as usize)),
    );

    let _ = gc_collect_minor();

    let moved_err = (js_shadow_slot_get(0) & POINTER_MASK) as usize;
    let prop = crate::node_submodules::diagnostics::error_user_prop(moved_err, "cause")
        .expect("prop must survive");
    let prop_addr = (prop.to_bits() & POINTER_MASK) as usize;
    assert_ne!(
        prop_addr, cause as usize,
        "the object referent must have been evacuated (and the stored \
         bits rewritten) — an identical address means the expando bag's \
         edge was not traced"
    );
    unsafe {
        let header = (prop_addr - crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
        assert_eq!(
            (*header).obj_type,
            crate::gc::GC_TYPE_OBJECT,
            "rewritten prop bits must point at the live moved object"
        );
    }
}
