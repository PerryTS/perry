//! P0 is the blocking native-host adapter. Preserve the existing web/WASI host
//! path; P3/P4 must wire host scheduling and the backend's own clock per agent.
use std::time::Instant;

pub fn install_turnloop_driver() {}
pub fn register_native_wait_bridge(_: fn() -> bool, _: fn(Instant), _: fn()) {}
pub extern "C" fn wake() {}
pub(super) fn next_deadline() -> Option<Instant> {
    None
}

pub fn loop_statistics() -> Option<[u64; 4]> {
    None
}
