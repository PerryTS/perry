//! How many threads the codegen phases use, and how much memory they may
//! hold at once.
//!
//! A compile's codegen runs two nested kinds of work: module jobs (lower one
//! module's HIR to LLVM IR, then compile it) and, inside a large module, LLVM
//! unit workers (one translation unit each). Both draw from one
//! [`CodegenBudget`]: every running module job and every unit worker beyond a
//! module's first holds one CPU slot plus its estimated peak memory. The
//! driver sizes the budget from the CPUs this process may run on and the
//! memory the host (or its cgroup) has available, and hands it to each module
//! job with [`with_budget`]; the unit layer reads it with [`current_budget`].
//!
//! The budget changes only when work runs, never what it produces: units are
//! fixed by the partition before any worker runs, each unit's object depends
//! only on that unit, and module objects are collected in module order.

use std::cell::RefCell;
use std::path::Path;
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

/// How many codegen units compile at once: `PERRY_CODEGEN_UNIT_JOBS` when set
/// to a positive count, else the CPU slots of the budget the calling module
/// job runs under, else [`default_unit_workers`]. The one knob both unit
/// backends (in-process native construction and the textual clang path) read.
pub fn unit_workers() -> usize {
    std::env::var("PERRY_CODEGEN_UNIT_JOBS")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .filter(|&n| n > 0)
        .or_else(|| current_budget().map(|budget| budget.cpus()))
        .unwrap_or_else(default_unit_workers)
}

/// Default number of concurrent LLVM unit workers when `PERRY_CODEGEN_UNIT_JOBS`
/// is unset and no budget is installed: every core the process may run on,
/// bounded by memory.
///
/// Each worker holds one whole translation unit through RS4GC, optimization
/// and emission. With the giant entry function's roots spilled (#8583) no
/// unit carries an unbounded fan-out, so a unit's peak is a bounded ~1-2 GiB,
/// and the memory bound allows one worker per [`UNIT_WORKER_MEMORY`] of the
/// memory the host reports available when the LLVM phase starts.
///
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

/// Headroom under this process's cgroup v2 memory limit (`memory.max` minus
/// `memory.current`), when the process runs under a finite one.
fn cgroup_memory_headroom_bytes() -> Option<u64> {
    if !cfg!(target_os = "linux") {
        return None;
    }
    let cgroup = std::fs::read_to_string("/proc/self/cgroup").ok()?;
    let relative = cgroup
        .lines()
        .find_map(|line| line.strip_prefix("0::"))?
        .trim();
    let dir = std::path::Path::new("/sys/fs/cgroup").join(relative.trim_start_matches('/'));
    let read = |name: &str| {
        std::fs::read_to_string(dir.join(name))
            .ok()
            .and_then(|text| text.trim().parse::<u64>().ok())
    };
    let limit = read("memory.max")?;
    let current = read("memory.current").unwrap_or(0);
    Some(limit.saturating_sub(current))
}

/// Peak memory a module job is charged before its IR exists: the lowering
/// graph plus one LLVM unit in flight. Calibrated against per-module peaks
/// measured one job at a time (`PERRY_CODEGEN_MEMORY_TRACE=1` with
/// `PERRY_MODULE_JOBS=1 PERRY_CODEGEN_UNIT_JOBS=1`) over the 353 modules of
/// tsc, Zod, Fastify and the Next.js standalone server: every module's
/// measured peak is at or below its estimate (closest: ajv's
/// `compile/codegen/index.js`, 326 MiB measured for 390 estimated; the
/// 9 MB TypeScript bundle measured 3.4 GiB for 6.0 GiB estimated). Most
/// small modules peak far below the base term; a few with wide import
/// surfaces reach 150-330 MiB, which is what the base covers.
pub fn module_memory_estimate(source_bytes: u64, callables: usize) -> u64 {
    MODULE_BASE_MEMORY
        + source_bytes.saturating_mul(MODULE_MEMORY_PER_SOURCE_BYTE)
        + (callables as u64).saturating_mul(MODULE_MEMORY_PER_CALLABLE)
}

const MODULE_BASE_MEMORY: u64 = 320 << 20;
const MODULE_MEMORY_PER_SOURCE_BYTE: u64 = 500;
const MODULE_MEMORY_PER_CALLABLE: u64 = 512 << 10;

/// Peak memory one extra LLVM unit worker is charged for a unit of
/// `estimated_ir_bytes` (the partition's own estimate). Calibrated like
/// [`module_memory_estimate`] over 115 units: the largest measured peak was
/// 24 bytes per estimated IR byte (a 19.4 MiB unit peaking at 465 MiB).
pub fn unit_memory_estimate(estimated_ir_bytes: u64) -> u64 {
    UNIT_BASE_MEMORY + estimated_ir_bytes.saturating_mul(UNIT_MEMORY_PER_IR_BYTE)
}

const UNIT_BASE_MEMORY: u64 = 64 << 20;
const UNIT_MEMORY_PER_IR_BYTE: u64 = 32;

/// The share of the measured headroom a compile's codegen may plan to fill.
/// The rest absorbs estimate error and whatever else the host is doing.
const BUDGET_SHARE_NUM: u64 = 3;
const BUDGET_SHARE_DEN: u64 = 4;

/// Memory budget when the host does not report its available memory: about
/// what two module jobs with two unit workers each held before the budget.
const FALLBACK_MEMORY_BUDGET: u64 = 8 << 30;

/// The CPUs this process may run on, from `Cpus_allowed_list` in
/// `/proc/self/status` (the affinity mask). `None` where that is not reported.
fn allowed_cpus() -> Option<Vec<usize>> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    let list = status
        .lines()
        .find_map(|line| line.strip_prefix("Cpus_allowed_list:"))?;
    parse_cpu_list(list)
}

/// Parses a kernel CPU list such as `0-23,32-55` or `3`.
fn parse_cpu_list(list: &str) -> Option<Vec<usize>> {
    let mut cpus = Vec::new();
    for part in list.trim().split(',').filter(|p| !p.trim().is_empty()) {
        let (lo, hi) = match part.split_once('-') {
            Some((lo, hi)) => (
                lo.trim().parse::<usize>().ok()?,
                hi.trim().parse::<usize>().ok()?,
            ),
            None => {
                let n = part.trim().parse::<usize>().ok()?;
                (n, n)
            }
        };
        if hi < lo || hi - lo > 1 << 16 {
            return None;
        }
        cpus.extend(lo..=hi);
    }
    if cpus.is_empty() {
        None
    } else {
        Some(cpus)
    }
}

/// Number of distinct physical cores (unique `(physical_package_id, core_id)`
/// pairs under `<sysfs>/cpuN/topology`) among `cpus`. `None` when any of
/// those CPUs has no readable topology, as on containers and non-Linux hosts,
/// so the caller falls back to counting hardware threads.
fn physical_core_count(cpus: &[usize], sysfs: &Path) -> Option<usize> {
    let mut cores = std::collections::HashSet::new();
    for cpu in cpus {
        let topology = sysfs.join(format!("cpu{cpu}")).join("topology");
        let read = |name: &str| -> Option<i64> {
            std::fs::read_to_string(topology.join(name))
                .ok()?
                .trim()
                .parse()
                .ok()
        };
        cores.insert((read("physical_package_id")?, read("core_id")?));
    }
    if cores.is_empty() {
        None
    } else {
        Some(cores.len())
    }
}

/// Where a [`CodegenBudget`]'s numbers came from, for the compile log.
#[derive(Debug, Clone, Default)]
pub struct BudgetProbe {
    /// CPU slots the budget got.
    pub cpus: usize,
    /// `available_parallelism`: the CPUs (hardware threads) this process may
    /// run on, after the affinity mask and the cgroup CPU quota.
    pub threads: usize,
    /// Physical cores among those CPUs, where the host reports a topology.
    pub physical_cores: Option<usize>,
    /// `PERRY_CODEGEN_CPU_SLOTS`, when set to a positive number.
    pub cpu_slots_override: Option<usize>,
    pub mem_available: Option<u64>,
    pub cgroup_headroom: Option<u64>,
    pub memory_override: Option<u64>,
}

/// CPU slots and memory shared by every module job and unit worker of one
/// compile. A grant holds one CPU slot and its charged memory until dropped.
///
/// Admission: a grant needs a free CPU slot and its memory must fit in what
/// is free — except when nothing else holds a grant, so an estimate larger
/// than the whole budget still runs, alone. Memory in flight therefore never
/// exceeds the budget unless a single job's estimate does.
///
/// Priority: a unit worker of a module that is already running
/// ([`Self::acquire_while`]) goes before a module job that has not started
/// ([`Self::acquire`]). A started module is on some path to the end of the
/// compile, and its remaining units are the only work that shortens it; a
/// new small module can always run later.
#[derive(Debug)]
pub struct CodegenBudget {
    cpus: usize,
    memory: u64,
    state: Mutex<BudgetState>,
    freed: Condvar,
}

#[derive(Debug)]
struct BudgetState {
    cpus_free: usize,
    memory_free: u64,
    grants: usize,
    /// Unit workers waiting for a grant; module jobs wait while any do.
    priority_waiting: usize,
}

/// One CPU slot and the memory charged for it; returned on drop.
#[derive(Debug)]
pub struct BudgetGrant<'a> {
    budget: &'a CodegenBudget,
    memory: u64,
}

impl CodegenBudget {
    pub fn new(cpus: usize, memory: u64) -> Self {
        let cpus = cpus.max(1);
        Self {
            cpus,
            memory,
            state: Mutex::new(BudgetState {
                cpus_free: cpus,
                memory_free: memory,
                grants: 0,
                priority_waiting: 0,
            }),
            freed: Condvar::new(),
        }
    }

    /// The budget for this machine: one CPU slot per physical core among the
    /// CPUs this process may run on (affinity mask and cgroup quota, as
    /// `available_parallelism` reports them; hardware threads where the
    /// topology is unreadable; `PERRY_CODEGEN_CPU_SLOTS` replaces the figure)
    /// and [`BUDGET_SHARE_NUM`]/[`BUDGET_SHARE_DEN`] of the smaller of
    /// `MemAvailable` and the cgroup headroom. `PERRY_CODEGEN_MEMORY_MB`
    /// replaces the memory figure outright.
    pub fn for_this_machine() -> (Self, BudgetProbe) {
        let threads = std::thread::available_parallelism()
            .map(|p| p.get())
            .unwrap_or(1);
        let physical_cores = allowed_cpus()
            .and_then(|cpus| physical_core_count(&cpus, Path::new("/sys/devices/system/cpu")));
        let cpu_slots_override = std::env::var("PERRY_CODEGEN_CPU_SLOTS")
            .ok()
            .and_then(|v| v.trim().parse::<usize>().ok())
            .filter(|&n| n > 0);
        let probe = BudgetProbe {
            cpus: cpu_slots_override
                .unwrap_or_else(|| physical_cores.map_or(threads, |cores| cores.min(threads))),
            threads,
            physical_cores,
            cpu_slots_override,
            mem_available: available_memory_bytes(),
            cgroup_headroom: cgroup_memory_headroom_bytes(),
            memory_override: std::env::var("PERRY_CODEGEN_MEMORY_MB")
                .ok()
                .and_then(|v| v.trim().parse::<u64>().ok())
                .filter(|&mb| mb > 0)
                .map(|mb| mb << 20),
        };
        let memory = probe.memory_override.unwrap_or_else(|| {
            match (probe.mem_available, probe.cgroup_headroom) {
                (None, None) => FALLBACK_MEMORY_BUDGET,
                (available, headroom) => {
                    available
                        .unwrap_or(u64::MAX)
                        .min(headroom.unwrap_or(u64::MAX))
                        / BUDGET_SHARE_DEN
                        * BUDGET_SHARE_NUM
                }
            }
        });
        (Self::new(probe.cpus, memory), probe)
    }

    pub fn cpus(&self) -> usize {
        self.cpus
    }

    pub fn memory(&self) -> u64 {
        self.memory
    }

    /// How many jobs of `smallest` estimated memory can hold grants at once:
    /// the CPU slots, or fewer when memory admits fewer. A thread beyond this
    /// could never start a job, and its allocator arena would still hold
    /// memory.
    pub fn admissible_jobs(&self, smallest: u64) -> usize {
        let by_memory = (self.memory / smallest.max(1)).max(1);
        self.cpus
            .min(usize::try_from(by_memory).unwrap_or(usize::MAX))
    }

    fn try_take(&self, state: &mut BudgetState, memory: u64) -> Option<BudgetGrant<'_>> {
        if state.cpus_free == 0 {
            return None;
        }
        let charged = if state.grants == 0 {
            memory.min(state.memory_free)
        } else if memory <= state.memory_free {
            memory
        } else {
            return None;
        };
        state.cpus_free -= 1;
        state.memory_free -= charged;
        state.grants += 1;
        Some(BudgetGrant {
            budget: self,
            memory: charged,
        })
    }

    /// A module job: wait for one CPU slot and `memory`, after every waiting
    /// unit worker.
    pub fn acquire(&self, memory: u64) -> BudgetGrant<'_> {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        loop {
            if state.priority_waiting == 0 {
                if let Some(grant) = self.try_take(&mut state, memory) {
                    return grant;
                }
            }
            state = self
                .freed
                .wait_timeout(state, Duration::from_millis(20))
                .unwrap_or_else(|e| e.into_inner())
                .0;
        }
    }

    /// A unit worker of a running module: wait, ahead of module jobs, for
    /// one CPU slot and `memory`, and give up (returning `None`) once
    /// `keep_waiting` turns false — when the module has no unclaimed unit
    /// left. `keep_waiting` is re-checked at least every few milliseconds, so
    /// it need not signal this budget.
    pub fn acquire_while(
        &self,
        memory: u64,
        keep_waiting: impl Fn() -> bool,
    ) -> Option<BudgetGrant<'_>> {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        state.priority_waiting += 1;
        let grant = loop {
            if !keep_waiting() {
                break None;
            }
            if let Some(grant) = self.try_take(&mut state, memory) {
                break Some(grant);
            }
            state = self
                .freed
                .wait_timeout(state, Duration::from_millis(20))
                .unwrap_or_else(|e| e.into_inner())
                .0;
        };
        state.priority_waiting -= 1;
        drop(state);
        if grant.is_none() {
            // Module jobs held back by this waiter may proceed now.
            self.freed.notify_all();
        }
        grant
    }

    #[cfg(test)]
    fn free(&self) -> (usize, u64, usize) {
        let state = self.state.lock().unwrap();
        (state.cpus_free, state.memory_free, state.grants)
    }
}

impl BudgetGrant<'_> {
    /// The memory this grant was charged.
    pub fn memory(&self) -> u64 {
        self.memory
    }
}

impl Drop for BudgetGrant<'_> {
    fn drop(&mut self) {
        let mut state = self.budget.state.lock().unwrap_or_else(|e| e.into_inner());
        state.cpus_free += 1;
        state.memory_free += self.memory;
        state.grants -= 1;
        drop(state);
        self.budget.freed.notify_all();
    }
}

thread_local! {
    static CURRENT_BUDGET: RefCell<Option<Arc<CodegenBudget>>> = const { RefCell::new(None) };
}

/// Run `f` with `budget` as the budget its unit workers draw from.
pub fn with_budget<R>(budget: &Arc<CodegenBudget>, f: impl FnOnce() -> R) -> R {
    struct Restore(Option<Arc<CodegenBudget>>);
    impl Drop for Restore {
        fn drop(&mut self) {
            let previous = self.0.take();
            CURRENT_BUDGET.with(|slot| *slot.borrow_mut() = previous);
        }
    }
    let _restore =
        Restore(CURRENT_BUDGET.with(|slot| slot.borrow_mut().replace(Arc::clone(budget))));
    f()
}

/// The budget the calling module job runs under, if any.
pub fn current_budget() -> Option<Arc<CodegenBudget>> {
    CURRENT_BUDGET.with(|slot| slot.borrow().clone())
}

/// `PERRY_CODEGEN_MEMORY_TRACE=1`: report each module job's (and each LLVM
/// unit's) peak resident memory. The peak is process-wide, so the numbers
/// mean "this job's peak" only when one job runs at a time
/// (`PERRY_MODULE_JOBS=1 PERRY_CODEGEN_UNIT_JOBS=1`); that is how the
/// estimate constants above are calibrated.
pub fn memory_trace_enabled() -> bool {
    std::env::var("PERRY_CODEGEN_MEMORY_TRACE").as_deref() == Ok("1")
}

/// Return freed C-heap pages to the OS, then reset the process's
/// peak-resident mark (`VmHWM`) to its current resident size (Linux
/// `clear_refs` value 5), so a traced job's peak is its own growth rather
/// than hidden by pages an earlier job freed but the allocator kept. A no-op
/// off Linux.
pub fn reset_peak_resident() {
    #[cfg(all(target_os = "linux", target_env = "gnu"))]
    {
        extern "C" {
            fn malloc_trim(pad: usize) -> std::os::raw::c_int;
        }
        // SAFETY: glibc's malloc_trim takes no pointers and only releases
        // free heap pages.
        unsafe {
            malloc_trim(0);
        }
    }
    if cfg!(target_os = "linux") {
        let _ = std::fs::write("/proc/self/clear_refs", "5");
    }
}

/// `(VmRSS, VmHWM)` of this process in bytes, where the host reports them.
pub fn resident_bytes() -> Option<(u64, u64)> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    let field = |name: &str| {
        status.lines().find_map(|line| {
            let kib = line.strip_prefix(name)?.trim().strip_suffix("kB")?;
            kib.trim().parse::<u64>().ok().map(|kib| kib * 1024)
        })
    };
    Some((field("VmRSS:")?, field("VmHWM:")?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    /// A fake `/sys/devices/system/cpu` with `(cpu, package, core)` rows.
    fn fake_sysfs(name: &str, rows: &[(usize, i64, i64)]) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("perry-sysfs-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        for (cpu, package, core) in rows {
            let topology = dir.join(format!("cpu{cpu}")).join("topology");
            std::fs::create_dir_all(&topology).unwrap();
            std::fs::write(topology.join("physical_package_id"), format!("{package}\n")).unwrap();
            std::fs::write(topology.join("core_id"), format!("{core}\n")).unwrap();
        }
        dir
    }

    #[test]
    fn cpu_lists_parse_ranges_and_singles() {
        assert_eq!(
            parse_cpu_list("0-3,8,10-11\n"),
            Some(vec![0, 1, 2, 3, 8, 10, 11])
        );
        assert_eq!(parse_cpu_list("5"), Some(vec![5]));
        assert_eq!(parse_cpu_list(" 0-1 "), Some(vec![0, 1]));
        assert_eq!(parse_cpu_list(""), None);
        assert_eq!(parse_cpu_list("3-1"), None);
        assert_eq!(parse_cpu_list("a-b"), None);
    }

    #[test]
    fn smt_siblings_share_one_physical_core() {
        // Two packages; cpu0/cpu4 and cpu1/cpu5 are siblings, as are
        // cpu2/cpu6 (package 1, core 0 — the same core_id as package 0's).
        let dir = fake_sysfs(
            "smt",
            &[
                (0, 0, 0),
                (1, 0, 1),
                (2, 1, 0),
                (3, 1, 1),
                (4, 0, 0),
                (5, 0, 1),
                (6, 1, 0),
                (7, 1, 1),
            ],
        );
        assert_eq!(
            physical_core_count(&[0, 1, 2, 3, 4, 5, 6, 7], &dir),
            Some(4)
        );
        // Only the allowed CPUs count: one thread of each core.
        assert_eq!(physical_core_count(&[0, 1, 2, 3], &dir), Some(4));
        assert_eq!(physical_core_count(&[0, 4], &dir), Some(1));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn unreadable_topology_falls_back_to_none() {
        let dir = fake_sysfs("missing", &[(0, 0, 0), (1, 0, 1)]);
        assert_eq!(physical_core_count(&[0, 1, 2], &dir), None);
        assert_eq!(physical_core_count(&[0], &dir.join("nowhere")), None);
        assert_eq!(physical_core_count(&[], &dir), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn grants_need_a_cpu_and_their_memory() {
        let budget = CodegenBudget::new(2, 100);
        let a = budget.acquire(60);
        assert_eq!(budget.free(), (1, 40, 1));
        assert!(budget.acquire_while(50, || false).is_none());
        let mut state = budget.state.lock().unwrap();
        assert!(
            budget.try_take(&mut state, 50).is_none(),
            "50 does not fit in 40"
        );
        let b = budget.try_take(&mut state, 40).expect("40 fits");
        assert!(budget.try_take(&mut state, 0).is_none(), "no CPU slot left");
        drop(state);
        drop(a);
        drop(b);
        assert_eq!(budget.free(), (2, 100, 0));
    }

    #[test]
    fn an_estimate_above_the_whole_budget_runs_alone() {
        let budget = CodegenBudget::new(4, 100);
        let big = budget.acquire(1_000);
        assert_eq!(big.memory(), 100, "charged the whole budget, not more");
        assert_eq!(budget.free(), (3, 0, 1));
        let mut state = budget.state.lock().unwrap();
        assert!(
            budget.try_take(&mut state, 1).is_none(),
            "nothing else runs beside it"
        );
        drop(state);
        drop(big);
        let small = budget.acquire(10);
        let mut state = budget.state.lock().unwrap();
        assert!(
            budget.try_take(&mut state, 1_000).is_none(),
            "an oversized job waits while others hold grants"
        );
        drop(state);
        drop(small);
    }

    #[test]
    fn memory_in_flight_never_exceeds_the_budget() {
        let budget = Arc::new(CodegenBudget::new(8, 1_000));
        let in_flight = Arc::new(AtomicUsize::new(0));
        let peak = Arc::new(AtomicUsize::new(0));
        std::thread::scope(|scope| {
            for i in 0..32u64 {
                let (budget, in_flight, peak) = (&budget, &in_flight, &peak);
                scope.spawn(move || {
                    let charge = 100 + (i * 37) % 400;
                    let grant = budget.acquire(charge);
                    let now =
                        in_flight.fetch_add(charge as usize, Ordering::SeqCst) + charge as usize;
                    peak.fetch_max(now, Ordering::SeqCst);
                    std::thread::sleep(Duration::from_millis(2));
                    in_flight.fetch_sub(charge as usize, Ordering::SeqCst);
                    drop(grant);
                });
            }
        });
        assert!(peak.load(Ordering::SeqCst) <= 1_000);
        assert_eq!(budget.free(), (8, 1_000, 0));
    }

    #[test]
    fn a_waiting_unit_worker_gives_up_when_its_work_is_gone() {
        let budget = CodegenBudget::new(1, 100);
        let job = budget.acquire(10);
        let work_left = AtomicBool::new(true);
        std::thread::scope(|scope| {
            let waiter = scope.spawn(|| {
                budget
                    .acquire_while(10, || work_left.load(Ordering::SeqCst))
                    .is_some()
            });
            std::thread::sleep(Duration::from_millis(50));
            work_left.store(false, Ordering::SeqCst);
            assert!(
                !waiter.join().unwrap(),
                "no grant once the module's units are claimed"
            );
        });
        drop(job);
    }

    #[test]
    fn a_waiting_unit_worker_goes_before_a_new_module_job() {
        let budget = CodegenBudget::new(2, 100);
        let running_module = budget.acquire(10);
        let busy = budget.acquire(10);
        let order = Mutex::new(Vec::new());
        let module_started = std::sync::atomic::AtomicBool::new(false);
        let wait_until = |done: &dyn Fn() -> bool| {
            let deadline = std::time::Instant::now() + Duration::from_secs(60);
            while !done() {
                assert!(std::time::Instant::now() < deadline, "premise never held");
                std::thread::sleep(Duration::from_millis(1));
            }
        };
        std::thread::scope(|scope| {
            scope.spawn(|| {
                module_started.store(true, std::sync::atomic::Ordering::SeqCst);
                let _job = budget.acquire(10);
                order.lock().unwrap().push("module job");
            });
            // The module job is at least running; it may be anywhere before or in
            // acquire. Every slot is held, so it cannot be granted either way.
            wait_until(&|| module_started.load(std::sync::atomic::Ordering::SeqCst));
            scope.spawn(|| {
                let _unit = budget.acquire_while(10, || true).unwrap();
                order.lock().unwrap().push("unit worker");
            });
            // Free the slot only once the unit worker is registered as waiting:
            // that registration, not elapsed time, is what the module job defers to.
            wait_until(&|| {
                budget
                    .state
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .priority_waiting
                    == 1
            });
            drop(busy);
        });
        drop(running_module);
        assert_eq!(*order.lock().unwrap(), vec!["unit worker", "module job"]);
    }

    #[test]
    fn the_installed_budget_is_scoped_to_the_job() {
        assert!(current_budget().is_none());
        let budget = Arc::new(CodegenBudget::new(3, 1));
        with_budget(&budget, || {
            assert_eq!(current_budget().map(|b| b.cpus()), Some(3));
            if std::env::var("PERRY_CODEGEN_UNIT_JOBS").is_err() {
                assert_eq!(unit_workers(), 3);
            }
        });
        assert!(current_budget().is_none());
    }

    #[test]
    fn threads_are_capped_by_what_memory_admits() {
        let budget = CodegenBudget::new(48, 1 << 30);
        assert_eq!(budget.admissible_jobs(320 << 20), 3);
        assert_eq!(budget.admissible_jobs(4 << 30), 1);
        assert_eq!(CodegenBudget::new(4, u64::MAX).admissible_jobs(1), 4);
    }

    #[test]
    fn estimates_grow_with_their_inputs() {
        assert!(module_memory_estimate(1 << 20, 1_000) > module_memory_estimate(1 << 10, 10));
        assert!(unit_memory_estimate(20 << 20) > unit_memory_estimate(1 << 20));
    }
}
