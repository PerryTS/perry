//! Which machine-code tier each function of a compile landed in.
//!
//! A function over the optimized machine-pipeline budget
//! (`inprocess::DEFAULT_FAST_EMIT_MAX_INSTRS_X86_64`) no longer silently
//! costs its whole codegen unit. It takes, in order:
//!
//! 1. **re-lowered** — a statepoint function is re-lowered with its GC roots
//!    in a shadow frame and compiled again, still through the optimized
//!    machine pipeline;
//! 2. **contained** — a function still over the budget is emitted alone
//!    through a bounded machine (`inprocess::MachineTier`: the optimized
//!    pipeline with FastISel, or O0 past four times the budget), and its
//!    unit's other functions keep the optimized one;
//! 3. **whole unit** — where the unit cannot be split (COFF, or a host that
//!    cannot partially link the target's objects), the whole unit takes the
//!    bounded machine.
//!
//! These counters are process-wide and only ever grow. The compile driver
//! prints [`summary`] once codegen is done, so a build in which anything
//! left the optimized tier says so in one line instead of burying it in
//! per-unit logs.

use std::sync::atomic::{AtomicUsize, Ordering};

static RELOWERED: AtomicUsize = AtomicUsize::new(0);
static CONTAINED_FAST_ISEL: AtomicUsize = AtomicUsize::new(0);
static CONTAINED_O0: AtomicUsize = AtomicUsize::new(0);
static WHOLE_UNITS: AtomicUsize = AtomicUsize::new(0);
static WHOLE_UNIT_FUNCTIONS: AtomicUsize = AtomicUsize::new(0);
static WHOLE_UNITS_O0: AtomicUsize = AtomicUsize::new(0);

/// A snapshot of the counters.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MachineTierCounts {
    /// Statepoint functions re-lowered onto a shadow frame because they were
    /// over the machine budget after IR optimization.
    pub relowered: usize,
    /// Functions emitted alone through the optimized machine pipeline with
    /// FastISel instruction selection.
    pub contained_fast_isel: usize,
    /// Functions emitted alone through LLVM's O0 machine pipeline (more than
    /// four times over the budget).
    pub contained_o0: usize,
    /// Units emitted whole through a bounded machine because they could not
    /// be split.
    pub whole_units: usize,
    /// Of those, the units that took the O0 machine pipeline.
    pub whole_units_o0: usize,
    /// Defined functions in those units.
    pub whole_unit_functions: usize,
}

pub(crate) fn note_relowered(n: usize) {
    RELOWERED.fetch_add(n, Ordering::Relaxed);
}

#[cfg(feature = "llvm-inprocess")]
pub(crate) fn note_contained(tier: crate::inprocess::MachineTier, n: usize) {
    match tier {
        crate::inprocess::MachineTier::FastIsel => &CONTAINED_FAST_ISEL,
        crate::inprocess::MachineTier::O0 => &CONTAINED_O0,
    }
    .fetch_add(n, Ordering::Relaxed);
}

#[cfg(feature = "llvm-inprocess")]
pub(crate) fn note_whole_unit(tier: crate::inprocess::MachineTier, functions: usize) {
    WHOLE_UNITS.fetch_add(1, Ordering::Relaxed);
    WHOLE_UNIT_FUNCTIONS.fetch_add(functions, Ordering::Relaxed);
    if tier == crate::inprocess::MachineTier::O0 {
        WHOLE_UNITS_O0.fetch_add(1, Ordering::Relaxed);
    }
}

pub fn counts() -> MachineTierCounts {
    MachineTierCounts {
        relowered: RELOWERED.load(Ordering::Relaxed),
        contained_fast_isel: CONTAINED_FAST_ISEL.load(Ordering::Relaxed),
        contained_o0: CONTAINED_O0.load(Ordering::Relaxed),
        whole_units: WHOLE_UNITS.load(Ordering::Relaxed),
        whole_units_o0: WHOLE_UNITS_O0.load(Ordering::Relaxed),
        whole_unit_functions: WHOLE_UNIT_FUNCTIONS.load(Ordering::Relaxed),
    }
}

/// One line for the compile summary, or `None` when every function this
/// process emitted kept the optimized machine pipeline.
pub fn summary() -> Option<String> {
    format_summary(counts())
}

fn format_summary(c: MachineTierCounts) -> Option<String> {
    if c == MachineTierCounts::default() {
        return None;
    }
    Some(format!(
        "machine code: {} function(s) over the optimized-pipeline budget re-lowered onto a \
         shadow frame (optimized pipeline kept); {} emitted alone with FastISel, {} alone \
         with O0; {} unit(s) / {} function(s) emitted whole by a bounded machine ({} of those \
         units with O0) because the unit could not be split",
        c.relowered,
        c.contained_fast_isel,
        c.contained_o0,
        c.whole_units,
        c.whole_unit_functions,
        c.whole_units_o0
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summary_is_silent_until_a_function_leaves_the_optimized_tier() {
        assert_eq!(format_summary(MachineTierCounts::default()), None);
        let line = format_summary(MachineTierCounts {
            relowered: 1,
            contained_fast_isel: 2,
            contained_o0: 4,
            whole_units: 3,
            whole_unit_functions: 950,
            whole_units_o0: 1,
        })
        .expect("a non-empty census prints");
        for needle in [
            "1 function(s)",
            "2 emitted alone with FastISel",
            "4 alone with O0",
            "3 unit(s) / 950 function(s)",
            "1 of those units with O0",
        ] {
            assert!(line.contains(needle), "{needle:?} missing from {line}");
        }
    }
}
