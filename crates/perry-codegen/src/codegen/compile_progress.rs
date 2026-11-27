//! Per-module codegen progress reporting and the shared heartbeat thread.

use std::cell::Cell;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Arc, Mutex, Once};
use std::time::{Duration, Instant};

pub(crate) struct CompileProgress {
    enabled: bool,
    started: Instant,
    last_checkpoint: Cell<Instant>,
    phase: Arc<AtomicU8>,
    /// This module's row in the heartbeat's in-flight list.
    ticket: Option<u64>,
    module: String,
    stage: &'static str,
}

/// Every module in a codegen stage right now, for the one heartbeat thread.
/// A stage that runs long names itself even when its module is too small for
/// detailed progress: a births pre-pass once ran for an hour with no output.
struct InFlight {
    next: u64,
    modules: Vec<(u64, String, &'static str, Instant, Arc<AtomicU8>)>,
}

static IN_FLIGHT: Mutex<InFlight> = Mutex::new(InFlight {
    next: 0,
    modules: Vec::new(),
});
static HEARTBEAT: Once = Once::new();

const HEARTBEAT_EVERY: Duration = Duration::from_secs(30);

fn start_heartbeat() {
    HEARTBEAT.call_once(|| {
        let spawned = std::thread::Builder::new()
            .name("perry-progress".into())
            .spawn(|| loop {
                std::thread::sleep(HEARTBEAT_EVERY);
                let in_flight = IN_FLIGHT.lock().unwrap_or_else(|e| e.into_inner());
                for (_, module, stage, started, phase) in &in_flight.modules {
                    let elapsed = started.elapsed();
                    if elapsed < HEARTBEAT_EVERY {
                        continue;
                    }
                    let label = match phase.load(Ordering::Relaxed) {
                        0 => "lowering HIR",
                        1 => "finalizing generated IR",
                        2 => "partitioning/freezing/LLVM codegen",
                        3 => "releasing generated IR",
                        _ => "LLVM optimization and object emission",
                    };
                    eprintln!(
                        "[perry] codegen: {module}: {stage}: {label}, elapsed {:.1} min",
                        elapsed.as_secs_f64() / 60.0
                    );
                }
            });
        // Progress is diagnostic: a compile without a heartbeat still compiles.
        drop(spawned);
    });
}

impl CompileProgress {
    pub(super) fn new(module: &str, callables: usize, births: bool) -> Self {
        let progress_mode = std::env::var("PERRY_CODEGEN_PROGRESS").unwrap_or_default();
        // Avoid three status lines for every tiny module in dependency-heavy
        // projects. Long modules get automatic reporting; `=all` is the
        // diagnostic override when per-module detail is wanted regardless.
        // The heartbeat covers every module whenever progress is on.
        let enabled = progress_mode == "all" || (progress_mode == "1" && callables >= 1_000);
        let stage = if births {
            "births pre-pass"
        } else {
            "lowering"
        };
        let started = Instant::now();
        let phase = Arc::new(AtomicU8::new(0));
        if enabled {
            eprintln!("[perry] codegen: {stage} {module} ({callables} callables)");
        }
        let ticket = matches!(progress_mode.as_str(), "1" | "all").then(|| {
            start_heartbeat();
            let mut in_flight = IN_FLIGHT.lock().unwrap_or_else(|e| e.into_inner());
            let ticket = in_flight.next;
            in_flight.next += 1;
            in_flight.modules.push((
                ticket,
                module.to_string(),
                stage,
                started,
                Arc::clone(&phase),
            ));
            ticket
        });
        Self {
            enabled,
            started,
            last_checkpoint: Cell::new(started),
            phase,
            ticket,
            module: module.to_string(),
            stage,
        }
    }

    pub(super) fn phase(&self, phase: u8, label: &str) {
        self.phase.store(phase, Ordering::Relaxed);
        if self.enabled {
            eprintln!(
                "[perry] codegen: {}: {} ({:.1}s elapsed)",
                self.module,
                label,
                self.started.elapsed().as_secs_f64()
            );
        }
    }

    /// Report a completed lowering subphase. Large generated bundles can spend
    /// minutes before LLVM sees any IR, so a coarse heartbeat is not enough to
    /// distinguish useful progress from a stuck compiler. Keeping the lap time
    /// here also makes the output directly usable as a lightweight profile.
    pub(crate) fn checkpoint(&self, label: &str) {
        let now = Instant::now();
        let previous = self.last_checkpoint.replace(now);
        if self.enabled {
            eprintln!(
                "[perry] codegen: {}: {} in {:.1}s ({:.1}s total)",
                self.module,
                label,
                now.duration_since(previous).as_secs_f64(),
                now.duration_since(self.started).as_secs_f64()
            );
        }
    }

    pub(crate) fn items(&self, label: &str, done: usize, total: usize, started: Instant) {
        if !self.enabled || total == 0 {
            return;
        }
        let elapsed = started.elapsed().as_secs_f64();
        let eta = if done == 0 {
            0.0
        } else {
            elapsed * (total.saturating_sub(done)) as f64 / done as f64
        };
        eprintln!(
            "[perry] codegen: {}: {} {}/{} ({:.0}%; {:.1}s elapsed; ETA ~{:.1}s)",
            self.module,
            label,
            done,
            total,
            done as f64 * 100.0 / total as f64,
            elapsed,
            eta
        );
    }
}

impl Drop for CompileProgress {
    fn drop(&mut self) {
        if let Some(ticket) = self.ticket {
            IN_FLIGHT
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .modules
                .retain(|(t, ..)| *t != ticket);
        }
        if self.enabled {
            eprintln!(
                "[perry] codegen: {}: {} finished in {:.1}s",
                self.module,
                self.stage,
                self.started.elapsed().as_secs_f64()
            );
        }
    }
}
