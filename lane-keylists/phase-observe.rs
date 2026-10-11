// Private attribution only; not compiled into the shipping runtime.
use std::cell::Cell;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

#[no_mangle]
pub static PERRY_LANE_PHASE_INS: [AtomicU64; 10] = [const { AtomicU64::new(0) }; 10];
#[no_mangle]
pub static PERRY_LANE_PHASE_US: [AtomicU64; 10] = [const { AtomicU64::new(0) }; 10];
#[no_mangle]
pub static PERRY_LANE_KEY_BIRTHS: AtomicU64 = AtomicU64::new(0);
#[no_mangle]
pub static PERRY_LANE_KEY_BYTES: AtomicU64 = AtomicU64::new(0);
thread_local! { static FD: Cell<i32> = const { Cell::new(-1) }; }

fn instructions() -> u64 {
    FD.with(|slot| unsafe {
        let mut fd = slot.get();
        if fd == -1 {
            // perf_event_attr: hardware instructions, user mode only,
            // counting this thread. Independent counters for worker collectors.
            let mut attr = [0u64; 16];
            attr[0] = 128u64 << 32;
            attr[1] = 1; // PERF_COUNT_HW_INSTRUCTIONS
            attr[4] = 3; // total time enabled + running: reject multiplexing
            attr[5] = (1 << 5) | (1 << 6); // exclude_kernel, exclude_hv
            fd = libc::syscall(libc::SYS_perf_event_open, attr.as_ptr(), 0, -1, -1, 0) as i32;
            assert!(fd >= 0, "private instruction counter could not open");
            slot.set(fd);
        }
        let mut counts = [0u64; 3];
        assert_eq!(libc::read(fd, counts.as_mut_ptr().cast(), 24), 24);
        assert_eq!(counts[1], counts[2], "private instruction counter multiplexed");
        counts[0]
    })
}

pub(super) struct Phase {
    index: usize,
    instructions: u64,
    start: Instant,
}
impl Phase {
    pub(super) fn enter(index: usize) -> Self {
        Self { index, instructions: instructions(), start: Instant::now() }
    }
}
impl Drop for Phase {
    fn drop(&mut self) {
        let us = self.start.elapsed().as_micros() as u64;
        let ins = instructions().saturating_sub(self.instructions);
        PERRY_LANE_PHASE_INS[self.index].fetch_add(ins, Ordering::Relaxed);
        PERRY_LANE_PHASE_US[self.index].fetch_add(us, Ordering::Relaxed);
    }
}

pub(super) fn birth(bytes: usize) {
    PERRY_LANE_KEY_BIRTHS.fetch_add(1, Ordering::Relaxed);
    PERRY_LANE_KEY_BYTES.fetch_add(bytes as u64, Ordering::Relaxed);
}
