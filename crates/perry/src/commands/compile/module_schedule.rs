//! Module codegen scheduling.
//!
//! Every module job is charged its estimated peak memory
//! ([`perry_codegen::workers::module_memory_estimate`]) against the compile's
//! [`CodegenBudget`], and jobs start largest first: the modules whose lowering
//! and LLVM units take longest begin while the many small modules fill the
//! remaining CPU slots around them, instead of running alone at the end. A
//! module whose estimate exceeds the whole budget still runs, alone.
//!
//! Scheduling decides only when a module compiles. Each job's result lands in
//! its module's slot, and the caller reads them in module order.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use perry_codegen::workers::CodegenBudget;

/// Module indices in the order their jobs start: largest estimate first,
/// module order among equals.
pub(super) fn start_order(estimates: &[u64]) -> Vec<usize> {
    let mut order: Vec<usize> = (0..estimates.len()).collect();
    order.sort_by(|&a, &b| estimates[b].cmp(&estimates[a]).then(a.cmp(&b)));
    order
}

/// Run `job(i)` for every module on `threads` workers of `pool`, starting
/// modules in `order`, each holding a grant of `estimates[i]` from `budget`
/// while it runs. Returns the results indexed by module.
pub(super) fn run_module_jobs<T: Send>(
    pool: &rayon::ThreadPool,
    threads: usize,
    budget: &CodegenBudget,
    order: &[usize],
    estimates: &[u64],
    job: impl Fn(usize) -> T + Sync,
) -> Vec<T> {
    let slots: Vec<Mutex<Option<T>>> = (0..estimates.len()).map(|_| Mutex::new(None)).collect();
    let next = AtomicUsize::new(0);
    pool.scope(|scope| {
        for _ in 0..threads.clamp(1, order.len().max(1)) {
            scope.spawn(|_| loop {
                let Some(&module) = order.get(next.fetch_add(1, Ordering::Relaxed)) else {
                    break;
                };
                let _grant = budget.acquire(estimates[module]);
                let result = job(module);
                *slots[module].lock().unwrap_or_else(|e| e.into_inner()) = Some(result);
            });
        }
    });
    slots
        .into_iter()
        .map(|slot| {
            slot.into_inner()
                .unwrap_or_else(|e| e.into_inner())
                .expect("every module job ran")
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicU64;
    use std::time::Duration;

    fn pool(threads: usize) -> rayon::ThreadPool {
        rayon::ThreadPoolBuilder::new()
            .num_threads(threads)
            .build()
            .unwrap()
    }

    #[test]
    fn largest_modules_start_first_and_ties_keep_module_order() {
        assert_eq!(start_order(&[5, 9, 1, 9, 5]), vec![1, 3, 0, 4, 2]);
        assert_eq!(start_order(&[]), Vec::<usize>::new());
    }

    #[test]
    fn results_come_back_in_module_order_whatever_the_schedule() {
        let estimates: Vec<u64> = (0..64).map(|i| (i * 7919) % 101).collect();
        let order = start_order(&estimates);
        let budget = CodegenBudget::new(8, 1_000);
        let results = run_module_jobs(&pool(8), 8, &budget, &order, &estimates, |i| {
            std::thread::sleep(Duration::from_micros(((i * 31) % 7) as u64 * 100));
            i * 10
        });
        assert_eq!(results, (0..64).map(|i| i * 10).collect::<Vec<_>>());
    }

    #[test]
    fn concurrent_jobs_stay_inside_the_memory_budget() {
        let estimates: Vec<u64> = (0..40).map(|i| 50 + (i * 13) % 300).collect();
        let order = start_order(&estimates);
        let budget = CodegenBudget::new(16, 600);
        let in_flight = AtomicU64::new(0);
        let peak = AtomicU64::new(0);
        let running = AtomicUsize::new(0);
        let peak_running = AtomicUsize::new(0);
        run_module_jobs(&pool(16), 16, &budget, &order, &estimates, |i| {
            let now = in_flight.fetch_add(estimates[i], Ordering::SeqCst) + estimates[i];
            peak.fetch_max(now, Ordering::SeqCst);
            let jobs = running.fetch_add(1, Ordering::SeqCst) + 1;
            peak_running.fetch_max(jobs, Ordering::SeqCst);
            std::thread::sleep(Duration::from_millis(3));
            running.fetch_sub(1, Ordering::SeqCst);
            in_flight.fetch_sub(estimates[i], Ordering::SeqCst);
        });
        assert!(peak.load(Ordering::SeqCst) <= 600, "memory ceiling held");
        assert!(
            peak_running.load(Ordering::SeqCst) > 1,
            "small jobs ran concurrently"
        );
    }

    #[test]
    fn an_oversized_module_runs_alone_and_the_rest_still_finish() {
        // Module 0 alone exceeds the budget: nothing runs beside it.
        let estimates = vec![5_000u64, 10, 10, 10, 10];
        let order = start_order(&estimates);
        let budget = CodegenBudget::new(4, 100);
        let running = AtomicUsize::new(0);
        let overlap_with_big = AtomicUsize::new(0);
        let results = run_module_jobs(&pool(4), 4, &budget, &order, &estimates, |i| {
            let others = running.fetch_add(1, Ordering::SeqCst);
            if i == 0 && others != 0 {
                overlap_with_big.fetch_add(1, Ordering::SeqCst);
            }
            std::thread::sleep(Duration::from_millis(if i == 0 { 30 } else { 5 }));
            running.fetch_sub(1, Ordering::SeqCst);
            i
        });
        assert_eq!(results, vec![0, 1, 2, 3, 4]);
        assert_eq!(overlap_with_big.load(Ordering::SeqCst), 0);
    }
}
