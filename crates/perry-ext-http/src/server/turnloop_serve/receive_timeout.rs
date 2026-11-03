//! HTTP time belongs to the Socket's parser: receive a request, answer it,
//! then wait for another request. Body activity never restarts receipt time.
use std::time::{Duration, Instant};

#[derive(Clone, Copy)]
pub(super) enum Clock {
    Receiving(Instant),
    KeepAlive(Instant),
    Off,
}
impl Clock {
    pub(super) fn receiving(&mut self, now: Instant) {
        if !matches!(self, Self::Receiving(_)) {
            *self = Self::Receiving(now);
        }
    }

    pub(super) fn remaining(
        self,
        headers_received: bool,
        request_ms: f64,
        headers_ms: f64,
        idle_ms: u64,
        now: Instant,
    ) -> Option<Duration> {
        let positive =
            |ms: f64| (ms > 0.0 && ms.is_finite()).then(|| Duration::from_millis(ms as u64));
        let (since, limit) = match self {
            Self::Receiving(since) => {
                let request = positive(request_ms);
                let headers = (!headers_received).then(|| positive(headers_ms)).flatten();
                let limit = match (request, headers) {
                    (Some(a), Some(b)) => Some(a.min(b)),
                    (a, b) => a.or(b),
                }?;
                (since, limit)
            }
            Self::KeepAlive(since) if idle_ms > 0 => (since, Duration::from_millis(idle_ms)),
            Self::KeepAlive(_) | Self::Off => return None,
        };
        Some(limit.saturating_sub(now.saturating_duration_since(since)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn receipt_uses_its_start_clock_and_does_not_restart_on_activity() {
        let start = Instant::now();
        let mut clock = Clock::Receiving(start);
        clock.receiving(start + Duration::from_millis(30));
        assert_eq!(
            clock.remaining(false, 80.0, 40.0, 5, start + Duration::from_millis(30)),
            Some(Duration::from_millis(10))
        );
        assert_eq!(
            clock.remaining(true, 80.0, 40.0, 5, start + Duration::from_millis(60)),
            Some(Duration::from_millis(20))
        );
        assert_eq!(
            clock.remaining(true, 80.0, 40.0, 5, start + Duration::from_millis(81)),
            Some(Duration::ZERO)
        );
    }
    #[test]
    fn disabled_request_timeout_keeps_only_the_incomplete_header_limit() {
        let start = Instant::now();
        let clock = Clock::Receiving(start);
        assert_eq!(
            clock.remaining(false, 0.0, 40.0, 5, start),
            Some(Duration::from_millis(40))
        );
        assert_eq!(
            clock.remaining(true, 0.0, 40.0, 5, start + Duration::from_secs(900)),
            None
        );
        assert_eq!(clock.remaining(false, 0.0, 0.0, 5, start), None);
    }
    #[test]
    fn a_response_has_no_receipt_deadline_and_keepalive_begins_after_it() {
        let start = Instant::now();
        assert_eq!(
            Clock::Off.remaining(false, 1.0, 1.0, 5, start + Duration::from_secs(900)),
            None
        );
        assert_eq!(
            Clock::KeepAlive(start).remaining(false, 1.0, 1.0, 5, start + Duration::from_millis(3)),
            Some(Duration::from_millis(2))
        );
        assert_eq!(
            Clock::KeepAlive(start).remaining(false, 1.0, 1.0, 0, start + Duration::from_secs(900)),
            None
        );
    }
}
