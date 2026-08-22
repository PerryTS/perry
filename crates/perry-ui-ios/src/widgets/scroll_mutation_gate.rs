use std::collections::HashSet;

pub(crate) const POST_SCROLL_QUIET_PUMPS: u8 = 2;

#[derive(Default)]
pub(crate) struct ScrollMutationGate {
    active_scrolls: HashSet<i64>,
    quiet_pumps_remaining: u8,
}

impl ScrollMutationGate {
    pub(crate) fn begin(&mut self, handle: i64) {
        self.active_scrolls.insert(handle);
        self.quiet_pumps_remaining = 0;
    }

    pub(crate) fn end(&mut self, handle: i64) {
        if self.active_scrolls.remove(&handle) && self.active_scrolls.is_empty() {
            self.quiet_pumps_remaining = POST_SCROLL_QUIET_PUMPS;
        }
    }

    pub(crate) fn should_defer_pump(&mut self) -> bool {
        if !self.active_scrolls.is_empty() {
            return true;
        }
        if self.quiet_pumps_remaining > 0 {
            self.quiet_pumps_remaining -= 1;
            return true;
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::{ScrollMutationGate, POST_SCROLL_QUIET_PUMPS};

    #[test]
    fn touch_scroll_defers_until_after_quiet_pumps() {
        let mut gate = ScrollMutationGate::default();
        assert!(!gate.should_defer_pump());

        gate.begin(7);
        assert!(gate.should_defer_pump());
        assert!(gate.should_defer_pump());

        gate.end(7);
        for _ in 0..POST_SCROLL_QUIET_PUMPS {
            assert!(gate.should_defer_pump());
        }
        assert!(!gate.should_defer_pump());
    }

    #[test]
    fn overlapping_scrolls_keep_the_gate_closed() {
        let mut gate = ScrollMutationGate::default();
        gate.begin(1);
        gate.begin(2);
        gate.end(1);
        assert!(gate.should_defer_pump());

        gate.end(2);
        for _ in 0..POST_SCROLL_QUIET_PUMPS {
            assert!(gate.should_defer_pump());
        }
        assert!(!gate.should_defer_pump());
    }
}
