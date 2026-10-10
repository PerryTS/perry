//! Phase diagnostics share the existing GC trace switch and instrument build.
pub(super) struct CloneTimer {
    phase: &'static str,
    started: Option<std::time::Instant>,
}
impl CloneTimer {
    pub(super) fn new(phase: &'static str) -> Self {
        Self {
            phase,
            started: crate::gc::env_flag_enabled("PERRY_GC_TRACE").then(std::time::Instant::now),
        }
    }
}
impl Drop for CloneTimer {
    fn drop(&mut self) {
        if let Some(started) = self.started {
            eprintln!(
                "{{\"event\":\"clone_phase\",\"phase\":\"{}\",\"agent\":{},\"elapsed_us\":{}}}",
                self.phase,
                crate::agent::current_agent(),
                started.elapsed().as_micros()
            );
        }
    }
}
