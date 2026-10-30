//! The x86-64 frame walk against the system unwinder, over frames whose
//! layout we chose.
//!
//! Two JS-shaped probe frames (`push rbp; mov rbp, rsp; sub rsp, 32`, the
//! frame-record prologue every generated function has) each write a sentinel
//! into the word an SP-based record names and another into the word an
//! FP-based record names, then call back into Rust. The outer probe's Rust
//! callback calls the inner probe, so a Rust frame sits between the two JS
//! frames: the walk must step it by its unwind row to reach the outer roots.
//! The walking callback runs both walkers while every probe frame is live and
//! keeps only what they read.
//!
//! What is asserted is the CONTENTS of the resolved words, not just that the
//! walkers agree: two walkers that agree on wrong words would pass a set
//! comparison. And each discriminating check has a sabotage arm that makes it
//! fail: a walk that skips one Rust frame step, and records whose base
//! register the walk cannot reconstruct.

use std::cell::{Cell, RefCell};

use super::{frame_walk, unwind, ResolvedRoot, StackMapIndex};

const SENTINEL_SP: u64 = 0xCAFE_F00D;
const SENTINEL_FP: u64 = 0xCAFE_FEED;
const DWARF_RBP: u16 = 6;
const DWARF_RSP: u16 = 7;
/// `push rbp` plus `sub rsp, 32`: the map's frame size for both probes.
const PROBE_STACK_SIZE: u32 = 40;

// The callback gets (body_sp, return_address, rbp) of the probe frame: its
// stack pointer at the call, the exact pc a record matches on, and its frame
// pointer. `rdi` carries the callback in, `rsi` an argument passed through.
core::arch::global_asm!(
    ".p2align 4",
    ".globl perry_frame_walk_probe_outer",
    "perry_frame_walk_probe_outer:",
    ".cfi_startproc",
    "push rbp",
    ".cfi_def_cfa_offset 16",
    ".cfi_offset rbp, -16",
    "mov rbp, rsp",
    ".cfi_def_cfa_register rbp",
    "sub rsp, 32",
    "mov rax, 0xCAFEF00D",
    "mov qword ptr [rsp + 8], rax",
    "mov rax, 0xCAFEFEED",
    "mov qword ptr [rbp - 8], rax",
    "mov rax, rdi",
    "mov rcx, rsi",
    "mov rdi, rsp",
    "lea rsi, [rip + 2f]",
    "mov rdx, rbp",
    "call rax",
    "2:",
    "mov rsp, rbp",
    "pop rbp",
    ".cfi_def_cfa rsp, 8",
    "ret",
    ".cfi_endproc",
    ".p2align 4",
    ".globl perry_frame_walk_probe_inner",
    "perry_frame_walk_probe_inner:",
    ".cfi_startproc",
    "push rbp",
    ".cfi_def_cfa_offset 16",
    ".cfi_offset rbp, -16",
    "mov rbp, rsp",
    ".cfi_def_cfa_register rbp",
    "sub rsp, 32",
    "mov rax, 0xCAFEF00D",
    "mov qword ptr [rsp + 8], rax",
    "mov rax, 0xCAFEFEED",
    "mov qword ptr [rbp - 8], rax",
    "mov rax, rdi",
    "mov rcx, rsi",
    "mov rdi, rsp",
    "lea rsi, [rip + 3f]",
    "mov rdx, rbp",
    "call rax",
    "3:",
    "mov rsp, rbp",
    "pop rbp",
    ".cfi_def_cfa rsp, 8",
    "ret",
    ".cfi_endproc",
    // A frame whose CFA is based on rbx: legal CFI the system unwinder
    // follows and the direct decoder declines, so it stands for every row the
    // fast walk cannot read.
    ".p2align 4",
    ".globl perry_frame_walk_probe_rbx_cfa",
    "perry_frame_walk_probe_rbx_cfa:",
    ".cfi_startproc",
    "push rbx",
    ".cfi_def_cfa_offset 16",
    ".cfi_offset rbx, -16",
    "mov rbx, rsp",
    ".cfi_def_cfa_register rbx",
    "sub rsp, 16",
    "mov rax, rdi",
    "mov rcx, rsi",
    "mov rdi, rsp",
    "lea rsi, [rip + 4f]",
    "mov rdx, rbp",
    "call rax",
    "4:",
    "mov rsp, rbx",
    "pop rbx",
    ".cfi_def_cfa rsp, 8",
    "ret",
    ".cfi_endproc",
);

type Callback = extern "C" fn(body_sp: usize, return_address: usize, rbp: usize);

unsafe extern "C" {
    fn perry_frame_walk_probe_outer(callback: Callback);
    fn perry_frame_walk_probe_inner(callback: Callback);
    fn perry_frame_walk_probe_rbx_cfa(callback: Callback);
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Probe {
    body_sp: usize,
    return_address: usize,
    rbp: usize,
}

impl Probe {
    fn roots(self) -> [usize; 2] {
        [self.body_sp + 8, self.rbp - 8]
    }
}

#[derive(Clone, Copy, Debug)]
struct Sample {
    address: usize,
    word: u64,
}

fn sample(address: usize) -> Sample {
    Sample {
        address,
        word: unsafe { std::ptr::read(address as *const u64) },
    }
}

#[derive(Clone, Copy)]
enum Scenario {
    /// outer probe -> Rust -> inner probe -> walk.
    TwoJsFrames,
    /// outer probe -> Rust -> rbx-CFA frame -> walk.
    UnreadableRow,
}

#[derive(Default)]
struct State {
    outer: Probe,
    inner: Probe,
    /// Base register every record names (sabotage: one the walk cannot use).
    foreign_base: bool,
    /// Start the fast walk at the inner probe, as a poll entry would.
    from_origin: bool,
    ran: bool,
    fast: Option<(Vec<Sample>, super::NativeStackWalkStats)>,
    slow: Vec<Sample>,
}

crate::perry_thread_local! {
    static STATE: RefCell<State> = RefCell::new(State::default());
    static SCENARIO: Cell<Scenario> = const { Cell::new(Scenario::TwoJsFrames) };
}

fn record_offset(probe: Probe, function: usize) -> u32 {
    u32::try_from(probe.return_address - function).expect("return address inside its probe")
}

/// The index both walks get: a real v8 blob naming the probe frames' roots.
fn index_for(state: &State, with_inner: bool) -> StackMapIndex {
    let outer_fn = perry_frame_walk_probe_outer as *const () as usize;
    let inner_fn = perry_frame_walk_probe_inner as *const () as usize;
    let (fp_reg, sp_reg) = if state.foreign_base {
        (3u16, 3u16) // rbx
    } else {
        (DWARF_RBP, DWARF_RSP)
    };
    let roots = vec![(sp_reg, 8), (fp_reg, -8)];
    let mut functions = vec![(
        outer_fn as u64,
        PROBE_STACK_SIZE,
        vec![(record_offset(state.outer, outer_fn), roots.clone())],
    )];
    if with_inner {
        functions.push((
            inner_fn as u64,
            PROBE_STACK_SIZE,
            vec![(record_offset(state.inner, inner_fn), roots)],
        ));
    }
    functions.sort_by_key(|function| function.0);
    let origin = functions[0].0 as usize;
    let blob = super::lazy::test_blob_multi_at(origin as u64, &functions);
    let mut index = super::build_index_from_sections(
        vec![Box::leak(blob.into_boxed_slice())],
        &[origin],
        super::IndexMode::Lazy,
    );
    // The outer probe stands in for `main`: the walk ends after it, instead of
    // stepping the test harness's thread start.
    let (start, end) = crate::eh_walker::cfi_function_extent(outer_fn as u64)
        .expect("the outer probe has unwind tables");
    index.root_frames = frame_walk::RootFrames::of([(start as usize, end as usize), (0, 0)]);
    index
}

/// The walking callback: both walkers, while every probe frame is live.
/// Nothing here may panic (an `extern "C"` frame aborts on unwind).
extern "C" fn walk_here(body_sp: usize, return_address: usize, rbp: usize) {
    let scenario = SCENARIO.with(Cell::get);
    let (index, origin, foreign_base) = STATE.with(|cell| {
        let mut state = cell.borrow_mut();
        let with_inner = matches!(scenario, Scenario::TwoJsFrames);
        if with_inner {
            state.inner = Probe {
                body_sp,
                return_address,
                rbp,
            };
        }
        let origin = (with_inner && state.from_origin).then_some(super::super::WalkOrigin {
            pc: return_address,
            sp: body_sp,
            fp: rbp,
        });
        (index_for(&state, with_inner), origin, state.foreign_base)
    });
    let mut fast = Vec::new();
    let fast_stats = {
        let _origin = super::super::ActiveWalkOrigin::enter(origin);
        frame_walk::visit(&index, &mut |root: ResolvedRoot| {
            fast.push(sample(root.address))
        })
    };
    let mut slow = Vec::new();
    // An rbx-based record names words relative to whatever rbx holds here,
    // which the unwinder would resolve and the sample would read: only the
    // fast walk's refusal is under test in that case.
    if !foreign_base {
        unwind::visit(&index, &mut |root: ResolvedRoot| {
            slow.push(sample(root.address))
        });
    }
    STATE.with(|cell| {
        let mut state = cell.borrow_mut();
        state.ran = true;
        state.fast = fast_stats.map(|stats| (fast, stats));
        state.slow = slow;
    });
}

extern "C" fn outer_body(body_sp: usize, return_address: usize, rbp: usize) {
    STATE.with(|cell| {
        cell.borrow_mut().outer = Probe {
            body_sp,
            return_address,
            rbp,
        }
    });
    // A Rust frame between the two probe frames: this one.
    match SCENARIO.with(Cell::get) {
        Scenario::TwoJsFrames => unsafe { perry_frame_walk_probe_inner(walk_here) },
        Scenario::UnreadableRow => unsafe { perry_frame_walk_probe_rbx_cfa(walk_here) },
    }
    std::hint::black_box(());
}

fn drive(scenario: Scenario, foreign_base: bool, from_origin: bool) -> State {
    SCENARIO.with(|cell| cell.set(scenario));
    STATE.with(|cell| {
        *cell.borrow_mut() = State {
            foreign_base,
            from_origin,
            ..State::default()
        }
    });
    unsafe { perry_frame_walk_probe_outer(outer_body) };
    let state = STATE.with(|cell| std::mem::take(&mut *cell.borrow_mut()));
    assert!(state.ran, "the walking callback never ran");
    state
}

/// The agreement the walks must reach: both land on exactly the probes'
/// root words, each holding its sentinel. An `Err` names what went wrong.
fn agreement(state: &State, expected: &[usize]) -> Result<(), String> {
    let mut expected: Vec<usize> = expected.to_vec();
    expected.sort_unstable();
    let Some((fast, _)) = state.fast.as_ref() else {
        return Err("the fast walk declined".into());
    };
    for (walker, samples) in [("fast", fast), ("unwinder", &state.slow)] {
        let mut addresses: Vec<usize> = samples.iter().map(|s| s.address).collect();
        addresses.sort_unstable();
        addresses.dedup();
        if addresses != expected {
            return Err(format!(
                "{walker} walk resolved {addresses:#x?}, expected {expected:#x?}"
            ));
        }
        for Sample { address, word } in samples {
            if *word != SENTINEL_SP && *word != SENTINEL_FP {
                return Err(format!(
                    "{walker} walk resolved {address:#x}, holding {word:#x}, not a sentinel"
                ));
            }
        }
    }
    Ok(())
}

#[test]
fn the_fast_walk_and_the_unwinder_resolve_both_frames_across_a_rust_frame() {
    let state = drive(Scenario::TwoJsFrames, false, false);
    let expected = [state.inner.roots(), state.outer.roots()].concat();
    agreement(&state, &expected).unwrap();
    let (_, stats) = state.fast.as_ref().unwrap();
    // The inner probe steps by its frame record; the Rust frames (the walk's
    // own, the callback, `outer_body`) by their rows.
    assert!(stats.record_steps >= 1, "{stats:?}");
    assert!(stats.cfi_steps >= 3, "{stats:?}");
}

#[test]
fn a_walk_from_the_entry_origin_skips_the_collector_frames() {
    let state = drive(Scenario::TwoJsFrames, false, true);
    let expected = [state.inner.roots(), state.outer.roots()].concat();
    agreement(&state, &expected).unwrap();
    let (_, stats) = state.fast.as_ref().unwrap();
    // inner probe (record step) -> outer_body (one row) -> outer probe (end).
    assert_eq!(
        (stats.frames_visited, stats.record_steps, stats.cfi_steps),
        (3, 1, 1),
        "{stats:?}"
    );
}

/// Sabotage: a walk that skips one Rust frame step. The agreement check must
/// fail, or it proves nothing.
#[test]
fn skipping_one_rust_frame_step_is_caught() {
    frame_walk::SKIP_ONE_CFI_STEP.with(|skip| skip.set(true));
    let state = drive(Scenario::TwoJsFrames, false, true);
    frame_walk::SKIP_ONE_CFI_STEP.with(|skip| skip.set(false));
    let expected = [state.inner.roots(), state.outer.roots()].concat();
    let verdict = agreement(&state, &expected);
    assert!(
        verdict.is_err(),
        "a walk that skipped the Rust frame between two JS frames passed the agreement check"
    );
}

/// A row the direct decoder cannot follow fails the fast walk closed, and the
/// unwinder still finds the outer frame's roots.
#[test]
fn an_unreadable_row_fails_closed_and_the_unwinder_answers() {
    let state = drive(Scenario::UnreadableRow, false, false);
    assert!(
        state.fast.is_none(),
        "the fast walk stepped an rbx-based CFA"
    );
    let mut slow: Vec<usize> = state.slow.iter().map(|s| s.address).collect();
    slow.sort_unstable();
    let mut expected = state.outer.roots().to_vec();
    expected.sort_unstable();
    assert_eq!(slow, expected);
    assert!(state
        .slow
        .iter()
        .all(|s| s.word == SENTINEL_SP || s.word == SENTINEL_FP));
}

/// A record based on a register other than rbp/rsp fails the walk closed
/// rather than resolving (or dropping) that root.
#[test]
fn a_foreign_base_register_fails_closed() {
    let state = drive(Scenario::TwoJsFrames, true, false);
    assert!(
        state.fast.is_none(),
        "the fast walk resolved an rbx-based root"
    );
}

/// The extent reject in `match_records` returns what the full scan returns.
#[test]
fn a_pc_past_every_record_matches_nothing_in_one_search() {
    let state = drive(Scenario::TwoJsFrames, false, false);
    let index = index_for(&state, true);
    let inner_fn = perry_frame_walk_probe_inner as *const () as usize;
    let at = state.inner.return_address;
    assert!(index.match_records(at).is_some());
    assert!(
        index.match_records(at + 16).is_some(),
        "the window still matches"
    );
    assert!(index.match_records(at + 17).is_none(), "past the window");
    assert!(index.match_records(inner_fn + 200).is_none());
}
