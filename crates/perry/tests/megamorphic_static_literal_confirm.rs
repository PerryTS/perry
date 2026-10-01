//! A megamorphic read of seeded literals is answered by the slot-guess
//! confirm, not by the by-name walk.
//!
//! One read site (`rd`) meets sixteen literal shapes `{a, kN}`, so it latches
//! megamorphic; every read then asks the receiver's own shape to confirm the
//! site's slot guess by ONE pointer compare of the listed key against the
//! site's key atom. Every one of those literals has a static ShapeId, so its
//! record and keys list are made by the seed unit, which runs before any
//! module's string pool. When the seeded list held its own strings instead of
//! the pool's atoms, no confirm ever matched and each read fell to
//! `ic_slow_body` + `inline_slot_of_key` (225.8 -> 435.8 instructions/read).
//!
//! The bound is on user-space instructions per read, measured as the
//! difference between a 5,000,000- and a 500,000-read run (fixed costs
//! cancel). It needs the hardware instruction counter: on a host without one
//! it fails, except under CI (`CI` set), where it says so and returns. The
//! program links the runtime as built (`PERRY_NO_AUTO_OPTIMIZE=1`), so the
//! count is the runtime under test (`PERRY_RUNTIME_DIR`, a release build).
#![cfg(target_os = "linux")]

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

const LEAD_MEGA1: &str = r#"
const N = Number(process.argv[2]);
const SINK: any[] = [];
function mk(i: number): any {
    if (i === 0) return { a: 1, k0: 0 };
    if (i === 1) return { a: 1, k1: 1 };
    if (i === 2) return { a: 1, k2: 2 };
    if (i === 3) return { a: 1, k3: 3 };
    if (i === 4) return { a: 1, k4: 4 };
    if (i === 5) return { a: 1, k5: 5 };
    if (i === 6) return { a: 1, k6: 6 };
    if (i === 7) return { a: 1, k7: 7 };
    if (i === 8) return { a: 1, k8: 8 };
    if (i === 9) return { a: 1, k9: 9 };
    if (i === 10) return { a: 1, k10: 10 };
    if (i === 11) return { a: 1, k11: 11 };
    if (i === 12) return { a: 1, k12: 12 };
    if (i === 13) return { a: 1, k13: 13 };
    if (i === 14) return { a: 1, k14: 14 };
    if (i === 15) return { a: 1, k15: 15 };
    return {};
}
function rd(o: any): number { return o.a; }
function run(n: number, xs: any[]): number {
    let h = 0.0;
    for (let k = 0; k < n; k++) { h += rd(xs[k & 15]); }
    return h;
}
const xs: any[] = [];
for (let i = 0; i < 16; i++) xs.push(mk(i));
SINK.push(xs);
console.log(run(N, xs), SINK.length);
"#;

/// Instructions per read above which the reads are not being confirmed:
/// confirmed reads measure 182.9 on x86_64, the by-name walk 392.9.
const MAX_INSTRUCTIONS_PER_READ: f64 = 260.0;

/// `perf_event_attr` up to `config1` (PERF_ATTR_SIZE_VER0, 64 bytes).
#[repr(C)]
#[derive(Default)]
struct PerfEventAttr {
    type_: u32,
    size: u32,
    config: u64,
    sample_period: u64,
    sample_type: u64,
    read_format: u64,
    flags: u64,
    wakeup_events: u32,
    bp_type: u32,
    config1: u64,
}

const PERF_TYPE_HARDWARE: u32 = 0;
const PERF_COUNT_HW_INSTRUCTIONS: u64 = 1;
const FLAG_DISABLED: u64 = 1 << 0;
const FLAG_INHERIT: u64 = 1 << 1;
const FLAG_EXCLUDE_KERNEL: u64 = 1 << 5;
const FLAG_EXCLUDE_HV: u64 = 1 << 6;
const PERF_EVENT_IOC_ENABLE: libc::c_ulong = 0x2400;
const PERF_EVENT_IOC_DISABLE: libc::c_ulong = 0x2401;

/// User-space instructions retired by `bin <n>` and everything it spawns: a
/// counter on this thread that its children inherit, enabled only around the
/// spawn and wait. `None` when the host has no usable counter.
fn instructions_of_run(bin: &std::path::Path, n: u64) -> Option<u64> {
    let attr = PerfEventAttr {
        type_: PERF_TYPE_HARDWARE,
        size: std::mem::size_of::<PerfEventAttr>() as u32,
        config: PERF_COUNT_HW_INSTRUCTIONS,
        flags: FLAG_DISABLED | FLAG_INHERIT | FLAG_EXCLUDE_KERNEL | FLAG_EXCLUDE_HV,
        ..Default::default()
    };
    // SAFETY: a valid attr of the size it declares; pid 0 = this thread.
    let fd = unsafe {
        libc::syscall(
            libc::SYS_perf_event_open,
            &attr as *const PerfEventAttr,
            0 as libc::pid_t,
            -1 as libc::c_int,
            -1 as libc::c_int,
            0 as libc::c_ulong,
        )
    } as libc::c_int;
    if fd < 0 {
        return None;
    }
    // SAFETY: `fd` is the counter opened above.
    unsafe { libc::ioctl(fd, PERF_EVENT_IOC_ENABLE, 0) };
    let out = Command::new(bin).arg(n.to_string()).output();
    // SAFETY: as above; the count of a reaped child is folded into `fd`.
    let mut count = 0u64;
    let read = unsafe {
        libc::ioctl(fd, PERF_EVENT_IOC_DISABLE, 0);
        let r = libc::read(fd, &mut count as *mut u64 as *mut libc::c_void, 8);
        libc::close(fd);
        r
    };
    let out = out.expect("run the fixture");
    assert!(
        out.status.success(),
        "fixture failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&out.stdout).trim(),
        format!("{n} 1"),
        "the fixture must print what node prints"
    );
    (read == 8 && count > 0).then_some(count)
}

#[test]
fn megamorphic_reads_of_seeded_literals_are_confirmed_by_the_slot_guess() {
    let dir = tempfile::tempdir().expect("tempdir");
    let entry = dir.path().join("main.ts");
    let bin = dir.path().join("lead_mega1");
    std::fs::write(&entry, LEAD_MEGA1).expect("write fixture");
    let compile = Command::new(perry_bin())
        .current_dir(dir.path())
        .arg("compile")
        .arg(&entry)
        .arg("-o")
        .arg(&bin)
        .env("PERRY_NO_CACHE", "1")
        .env("PERRY_NO_AUTO_OPTIMIZE", "1")
        .output()
        .expect("run perry compile");
    assert!(
        compile.status.success(),
        "perry compile failed\n{}",
        String::from_utf8_lossy(&compile.stderr)
    );
    let (small, big) = (500_000u64, 5_000_000u64);
    let (Some(a), Some(b)) = (
        instructions_of_run(&bin, small),
        instructions_of_run(&bin, big),
    ) else {
        if std::env::var_os("CI").is_some() {
            eprintln!("SKIPPED: no hardware instruction counter on this CI host");
            return;
        }
        panic!("no hardware instruction counter (perf_event_open): the bound was not measured");
    };
    let per_read = (b as f64 - a as f64) / (big - small) as f64;
    eprintln!("lead_mega1: {per_read:.1} instructions/read");
    assert!(
        per_read <= MAX_INSTRUCTIONS_PER_READ,
        "{per_read:.1} instructions per megamorphic read of a seeded literal (bound \
         {MAX_INSTRUCTIONS_PER_READ}): the slot-guess confirm is missing"
    );
}
