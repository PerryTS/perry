//! How many threads the codegen-unit phases use: LLVM unit workers and the
//! unit layout's text workers share one budget.

/// How many codegen units compile at once: `PERRY_CODEGEN_UNIT_JOBS` when set
/// to a positive count, else [`default_unit_workers`]. The one knob both unit
/// backends (in-process native construction and the textual clang path) read.
pub fn unit_workers() -> usize {
    std::env::var("PERRY_CODEGEN_UNIT_JOBS")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .filter(|&n| n > 0)
        .unwrap_or_else(default_unit_workers)
}

/// Default number of concurrent LLVM unit workers when `PERRY_CODEGEN_UNIT_JOBS`
/// is unset: every core the process may run on, bounded by memory.
///
/// Each worker holds one whole translation unit through RS4GC, optimization
/// and emission. With the giant entry function's roots spilled (#8583) no
/// unit carries an unbounded fan-out, so a unit's peak is a bounded ~1-2 GiB,
/// and the memory bound allows one worker per [`UNIT_WORKER_MEMORY`] of the
/// memory the host reports available when the LLVM phase starts. Units are
/// fixed by the partition before any worker runs, and each unit's object
/// depends only on that unit, so the worker count changes wall time and peak
/// memory, never the output.
///
/// The previous default, half the logical CPUs clamped to `[2, 8]`, left a
/// 64-core host compiling the Claude Code bundle's 128 units eight at a time.
/// Windows keeps the conservative `2` until its pagefile behavior under higher
/// fan-out is measured — the platform the original cap was chosen for (#8017).
pub fn default_unit_workers() -> usize {
    if cfg!(target_os = "windows") {
        return 2;
    }
    let cpus = std::thread::available_parallelism()
        .map(|p| p.get())
        .unwrap_or(1);
    let memory_bound = available_memory_bytes()
        .map(|bytes| (bytes / UNIT_WORKER_MEMORY).max(1) as usize)
        .unwrap_or(cpus);
    cpus.min(memory_bound).max(1)
}

/// The memory one LLVM unit worker is budgeted (see [`default_unit_workers`]).
const UNIT_WORKER_MEMORY: u64 = 2 << 30;

/// Memory the host reports available to new allocations, when it says.
fn available_memory_bytes() -> Option<u64> {
    if !cfg!(target_os = "linux") {
        return None;
    }
    let meminfo = std::fs::read_to_string("/proc/meminfo").ok()?;
    meminfo.lines().find_map(|line| {
        let kib = line
            .strip_prefix("MemAvailable:")?
            .trim()
            .strip_suffix("kB")?;
        kib.trim().parse::<u64>().ok().map(|kib| kib * 1024)
    })
}
