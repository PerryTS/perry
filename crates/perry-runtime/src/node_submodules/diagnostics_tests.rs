//! Tests split out of `diagnostics.rs` for the 2,000-line file gate.

#[allow(unused_imports)]
use super::*;

#[cfg(test)]
mod tests {
    use super::*;

    fn inactive_state() -> DiagChannelState {
        DiagChannelState {
            name: 0.0,
            obj: std::ptr::null_mut(),
            subscribers: Vec::new(),
            stores: Vec::new(),
        }
    }

    // #1309: crossing the soft cap evicts a batch of the oldest inactive
    // channels so the live-channel map stays bounded.
    #[test]
    fn diag_channels_capped_by_evicting_inactive() {
        DIAG_CHANNELS.with(|m| m.borrow_mut().clear());
        DIAG_CHANNEL_BY_KEY.with(|m| m.borrow_mut().clear());
        for _ in 0..DIAG_CHANNEL_SOFT_CAP + 100 {
            let id = next_diag_id();
            DIAG_CHANNELS.with(|m| {
                m.borrow_mut().insert(id, inactive_state());
            });
        }
        evict_inactive_diag_channels_if_needed();
        let len = DIAG_CHANNELS.with(|m| m.borrow().len());
        assert!(len <= DIAG_CHANNEL_SOFT_CAP, "expected <= cap, got {len}");
        assert!(
            len >= DIAG_CHANNEL_SOFT_CAP - DIAG_CHANNEL_EVICT_BATCH,
            "should evict at most one batch, got {len}"
        );
        DIAG_CHANNELS.with(|m| m.borrow_mut().clear());
    }

    // #1309: a subscribed (active) channel is never evicted, even when the
    // map is over the cap.
    #[test]
    fn active_diag_channel_survives_eviction() {
        DIAG_CHANNELS.with(|m| m.borrow_mut().clear());
        DIAG_CHANNEL_BY_KEY.with(|m| m.borrow_mut().clear());
        let active_id = next_diag_id();
        DIAG_CHANNELS.with(|m| {
            let mut s = inactive_state();
            s.subscribers.push(1.0);
            m.borrow_mut().insert(active_id, s);
        });
        for _ in 0..DIAG_CHANNEL_SOFT_CAP + 100 {
            let id = next_diag_id();
            DIAG_CHANNELS.with(|m| {
                m.borrow_mut().insert(id, inactive_state());
            });
        }
        evict_inactive_diag_channels_if_needed();
        assert!(
            DIAG_CHANNELS.with(|m| m.borrow().contains_key(&active_id)),
            "subscribed channel must not be evicted"
        );
        DIAG_CHANNELS.with(|m| m.borrow_mut().clear());
    }
}

#[cfg(test)]
mod error_prop_order_tests {
    use super::*;

    /// Allocate a REAL error. These tests used synthetic addresses
    /// (`0x4000_1000`) back when the properties lived in a side table keyed by
    /// an arbitrary `usize` — any integer was a valid key. The bag now hangs
    /// off the error's own `ObjectMeta`, so a fake address is dereferenced as a
    /// GC cell and segfaults. Storage on the object means tests need objects.
    unsafe fn fresh_error() -> usize {
        let msg = js_string_from_bytes(b"order".as_ptr(), 5);
        crate::error::js_error_new_with_message(msg) as usize
    }

    fn keys_of(err: usize) -> Vec<String> {
        error_user_props(err).into_iter().map(|(k, _)| k).collect()
    }

    /// Own string keys enumerate in INSERTION order, not hash or alphabetical
    /// order. Observable through `Object.keys`, `for…in`, `{...err}` and
    /// `JSON.stringify`, so a caught fs error must serialize as node's
    /// `{"errno":…,"code":…,"syscall":…,"path":…}`.
    #[test]
    fn user_props_enumerate_in_insertion_order() {
        let _lock = crate::gc::global_side_table_test_lock();
        unsafe {
            let err = fresh_error();
            for k in ["errno", "code", "syscall", "path"] {
                set_error_user_prop(err, k, 1.0);
            }
            assert_eq!(
                keys_of(err),
                vec![
                    "errno".to_string(),
                    "code".to_string(),
                    "syscall".to_string(),
                    "path".to_string()
                ],
                "fs error fields must enumerate in node's insertion order"
            );
        }
    }

    /// Reassigning an existing key keeps its ORIGINAL position — in node,
    /// `o.a=1; o.b=2; o.a=3` still enumerates `a,b`.
    #[test]
    fn reassignment_keeps_original_position() {
        let _lock = crate::gc::global_side_table_test_lock();
        unsafe {
            let err = fresh_error();
            set_error_user_prop(err, "a", 1.0);
            set_error_user_prop(err, "b", 2.0);
            set_error_user_prop(err, "a", 3.0);
            assert_eq!(keys_of(err), vec!["a".to_string(), "b".to_string()]);
            assert_eq!(
                error_user_prop(err, "a"),
                Some(3.0),
                "reassignment must still update the value"
            );
        }
    }

    /// Removing a key must not disturb the order of the survivors.
    #[test]
    fn removal_preserves_order_of_the_rest() {
        let _lock = crate::gc::global_side_table_test_lock();
        unsafe {
            let err = fresh_error();
            for k in ["one", "two", "three"] {
                set_error_user_prop(err, k, 0.0);
            }
            assert!(remove_error_user_prop(err, "two"));
            assert_eq!(keys_of(err), vec!["one".to_string(), "three".to_string()]);
            assert!(
                !remove_error_user_prop(err, "two"),
                "second remove is a no-op"
            );
        }
    }

    /// #6759 phase 1: two errors must not share properties, even if one is
    /// allocated at an address the other previously occupied. The old
    /// address-keyed table could not express that; storage on the object does
    /// so by construction.
    #[test]
    fn properties_belong_to_the_error_not_its_address() {
        let _lock = crate::gc::global_side_table_test_lock();
        unsafe {
            let a = fresh_error();
            let b = fresh_error();
            set_error_user_prop(a, "code", 1.0);
            assert_eq!(error_user_prop(a, "code"), Some(1.0));
            assert!(
                error_user_prop(b, "code").is_none(),
                "a distinct error must not see another's properties"
            );
        }
    }
}
