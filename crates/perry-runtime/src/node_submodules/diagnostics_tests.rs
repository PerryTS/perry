//! Unit tests for the node:diagnostics_channel submodule.
//!
//! Split out of `diagnostics.rs` to keep it under the 2,000-line file gate.

// Bring `diagnostics`'s items into scope so the nested `mod tests` blocks
// below resolve `use super::*` to them.
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

    /// Own string keys enumerate in INSERTION order, not hash or alphabetical
    /// order. This is observable through `Object.keys`, `for…in`, `{...err}`
    /// and `JSON.stringify`, so a caught fs error must serialize as node's
    /// `{"errno":…,"code":…,"syscall":…,"path":…}`.
    ///
    /// The store was a `HashMap` with an alphabetical `sort_by` bolted on for
    /// determinism, which is stable but wrong: it emitted `code` before
    /// `errno`. Reverting to any unordered container fails this test.
    #[test]
    fn user_props_enumerate_in_insertion_order() {
        let err = 0x4000_1000usize;
        for k in ["errno", "code", "syscall", "path"] {
            set_error_user_prop(err, k, 1.0);
        }
        let keys: Vec<String> = error_user_props(err).into_iter().map(|(k, _)| k).collect();
        assert_eq!(
            keys,
            vec![
                "errno".to_string(),
                "code".to_string(),
                "syscall".to_string(),
                "path".to_string()
            ],
            "fs error fields must enumerate in node's insertion order, not sorted"
        );
    }

    /// Reassigning an existing key keeps its ORIGINAL position — in node,
    /// `o.a=1; o.b=2; o.a=3` still enumerates `a,b`. An implementation that
    /// removed-then-appended would report `b,a`.
    #[test]
    fn reassignment_keeps_original_position() {
        let err = 0x4000_2000usize;
        set_error_user_prop(err, "a", 1.0);
        set_error_user_prop(err, "b", 2.0);
        set_error_user_prop(err, "a", 3.0);
        let keys: Vec<String> = error_user_props(err).into_iter().map(|(k, _)| k).collect();
        assert_eq!(keys, vec!["a".to_string(), "b".to_string()]);
        assert_eq!(
            error_user_prop(err, "a"),
            Some(3.0),
            "reassignment must still update the value"
        );
    }

    /// Removing a key must not disturb the order of the survivors.
    #[test]
    fn removal_preserves_order_of_the_rest() {
        let err = 0x4000_3000usize;
        for k in ["one", "two", "three"] {
            set_error_user_prop(err, k, 0.0);
        }
        assert!(remove_error_user_prop(err, "two"));
        let keys: Vec<String> = error_user_props(err).into_iter().map(|(k, _)| k).collect();
        assert_eq!(keys, vec!["one".to_string(), "three".to_string()]);
        assert!(
            !remove_error_user_prop(err, "two"),
            "second remove is a no-op"
        );
    }
}
