//! Typed-entry contracts for read sites (decision 129, `Obj(S, memo)`).
//!
//! A read site's ShapeId compare is the proof its typed body runs on: once
//! the receiver carries the entry's token, the body trusts the memo words the
//! site already holds (slot, spill bit, holder, hop pairs) and checks only
//! the one shared proof epoch for the hops (`PERRY_PROTO_VALIDITY`). Two
//! build-time switches, both read by `perry-runtime/build.rs` and keyed into
//! the auto-optimized runtime's cache:
//!
//! * `PERRY_CHECK_CONTRACTS=1` (`cfg(perry_check_contracts)`, the one
//!   contract mechanism, `entry_contracts`) — every typed body first re-runs
//!   the full shape proof the publication established (each hop's and the
//!   holder's ShapeId) and aborts through `entry_contracts::contract_violated`
//!   when the epoch admitted a memo that proof rejects. Test builds always
//!   check, and panic instead, so a sabotage test can observe it.
//! * `PERRY_FORCE_GENERIC=1` (`cfg(perry_force_generic)`) — every typed body
//!   declines, so each read takes the one generic entry: the differential
//!   gate compares a program's output both ways.
//!
//! A build without either compiles both to `false`: no test on any typed body.

/// Contract mode: does every typed body assert its precondition?
#[inline(always)]
pub(crate) const fn check_contracts() -> bool {
    cfg!(any(test, perry_check_contracts))
}

#[cfg(test)]
thread_local! {
    static FORCE_GENERIC_FOR_TEST: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Differential mode: do typed bodies decline, so the generic entry answers?
#[inline(always)]
pub(crate) fn force_generic() -> bool {
    #[cfg(test)]
    {
        FORCE_GENERIC_FOR_TEST.with(|f| f.get())
    }
    #[cfg(not(test))]
    {
        cfg!(perry_force_generic)
    }
}

/// Test hook: decline every typed body on this thread while `on`.
#[cfg(test)]
pub(crate) fn set_force_generic_for_test(on: bool) {
    FORCE_GENERIC_FOR_TEST.with(|f| f.set(on));
}

/// A typed body ran with its precondition false: name the entry, the
/// precondition and the site, and abort (panic in a test build, so a
/// sabotage test can observe it).
#[cold]
#[inline(never)]
pub(crate) fn violated(op: &str, precondition: &str, site: usize, bits: u64) -> ! {
    #[cfg(test)]
    {
        panic!(
            "PERRY_CHECK_CONTRACTS: {op} ran with {precondition} false (site {site:#x}, value {bits:#018x})"
        );
    }
    #[cfg(not(test))]
    {
        crate::entry_contracts::contract_violated(
            op,
            precondition,
            "site-shape-guard+proof-epoch",
            bits,
            &format!(" site=0x{site:x}"),
        )
    }
}

/// `contract_assert`: in contract mode, `holds` must be true.
#[inline(always)]
pub(crate) fn assert_holds(
    holds: impl FnOnce() -> bool,
    op: &str,
    precondition: &str,
    site: usize,
    bits: u64,
) {
    if check_contracts() && !holds() {
        violated(op, precondition, site, bits);
    }
}
