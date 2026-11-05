use super::{
    bindings::wasi::clocks::{monotonic_clock, wall_clock},
    streams::Pollable,
    State,
};
use cap_time_ext::{MonotonicClockExt, SystemClockExt};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use wasmtime::component::Resource;

fn datetime(d: Duration) -> wall_clock::Datetime {
    wall_clock::Datetime {
        seconds: d.as_secs(),
        nanoseconds: d.subsec_nanos(),
    }
}
impl wall_clock::Host for State {
    fn now(&mut self) -> wasmtime::Result<wall_clock::Datetime> {
        Ok(datetime(SystemTime::now().duration_since(UNIX_EPOCH)?))
    }
    fn resolution(&mut self) -> wasmtime::Result<wall_clock::Datetime> {
        Ok(datetime(
            cap_std::time::SystemClock::new(cap_std::ambient_authority()).resolution(),
        ))
    }
}
impl monotonic_clock::Host for State {
    fn now(&mut self) -> wasmtime::Result<u64> {
        Ok(self.initial.elapsed().as_nanos().try_into()?)
    }
    fn resolution(&mut self) -> wasmtime::Result<u64> {
        Ok(
            cap_std::time::MonotonicClock::new(cap_std::ambient_authority())
                .resolution()
                .as_nanos()
                .try_into()?,
        )
    }
    fn subscribe_instant(&mut self, when: u64) -> wasmtime::Result<Resource<Pollable>> {
        let now = monotonic_clock::Host::now(self)?;
        self.subscribe_duration(when.saturating_sub(now))
    }
    fn subscribe_duration(&mut self, when: u64) -> wasmtime::Result<Resource<Pollable>> {
        let operation = self.io.timer(when)?;
        Ok(self.table.push(Pollable::Operation(operation))?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sync_wasi::bindings::wasi::io::poll::{Host, HostPollable};
    use monotonic_clock::Host as _;
    #[test]
    fn timer_poll_waits_until_its_deadline_and_stays_ready() {
        let mut state = State::new(&crate::WasiOptions::default()).unwrap();
        let start = monotonic_clock::Host::now(&mut state).unwrap();
        let timer = state.subscribe_duration(5_000_000).unwrap();
        state.block(Resource::new_borrow(timer.rep())).unwrap();
        assert!(monotonic_clock::Host::now(&mut state).unwrap() >= start + 5_000_000);
        assert!(state.ready(Resource::new_borrow(timer.rep())).unwrap());
        assert_eq!(
            state
                .poll(vec![
                    Resource::new_borrow(timer.rep()),
                    Resource::new_borrow(timer.rep())
                ])
                .unwrap(),
            vec![0, 1]
        );
        HostPollable::drop(&mut state, timer).unwrap();
    }
    #[test]
    fn file_ready_does_not_wait_for_a_future_timer() {
        let mut state = State::new(&crate::WasiOptions::default()).unwrap();
        let timer = state.subscribe_duration(60_000_000_000).unwrap();
        let file = state.table.push(Pollable::Always).unwrap();
        assert_eq!(
            state
                .poll(vec![
                    Resource::new_borrow(timer.rep()),
                    Resource::new_borrow(file.rep())
                ])
                .unwrap(),
            vec![1]
        );
        HostPollable::drop(&mut state, timer).unwrap();
        HostPollable::drop(&mut state, file).unwrap();
    }
}
