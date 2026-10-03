//! `"a" in o` on a megamorphic receiver set reads the receiver's shape record
//! by id without resolving the runtime state.
//!
//! One `in` site meets sixteen literal shapes `{a, kN}`. Each test asks the
//! shape record of the receiver's ShapeId, which the lookup finds through the
//! agent's thread-local shape directory: one load of the band's directory, a
//! band select and two dependent loads, with no `state()` fetch and no
//! presence branch (an absent id reads the shared empty record). Resolving
//! the runtime state and walking the slab per lookup cost 973.1
//! instructions/op on x86_64; the directory read measures 872.7.
//!
//! The bound is on user-space instructions per op, measured as the
//! difference between a 5,000,000- and a 500,000-op run (fixed costs
//! cancel). It needs the hardware instruction counter: on a host without one
//! it fails, except under CI (`CI` set), where it says so and returns. The
//! program links the runtime as built (`PERRY_NO_AUTO_OPTIMIZE=1`), so the
//! count is the runtime under test (`PERRY_RUNTIME_DIR`, a release build).
// The instruction bound above was calibrated on x86_64.
#![cfg(all(target_os = "linux", target_arch = "x86_64"))]

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

const IN_OP: &str = r#"
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
    return { a: 1, k15: 15 };
}
const N = Number(process.argv[2]);
const xs: any[] = [];
for (let i = 0; i < 16; i++) xs.push(mk(i));
function has(o: any): boolean { return "a" in o; }
let h = 0;
for (let k = 0; k < N; k++) { if (has(xs[k & 15])) h++; }
console.log(h);
"#;

/// Instructions per op above which the by-id record lookup has gone back to
/// resolving the runtime state: the directory read measures 872.7 on x86_64,
/// the state walk 973.1.
const MAX_INSTRUCTIONS_PER_OP: f64 = 920.0;

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
        format!("{n}"),
        "the fixture must print what node prints"
    );
    (read == 8 && count > 0).then_some(count)
}

#[test]
fn in_on_megamorphic_literals_reads_the_shape_record_from_the_agent_directory() {
    let dir = tempfile::tempdir().expect("tempdir");
    let entry = dir.path().join("main.ts");
    let bin = dir.path().join("in_op");
    std::fs::write(&entry, IN_OP).expect("write fixture");
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
    let per_op = (b as f64 - a as f64) / (big - small) as f64;
    eprintln!("in_op: {per_op:.1} instructions/op");
    assert!(
        per_op <= MAX_INSTRUCTIONS_PER_OP,
        "{per_op:.1} instructions per megamorphic `in` (bound {MAX_INSTRUCTIONS_PER_OP}): \
         the by-id shape record lookup is resolving the runtime state again"
    );
}
