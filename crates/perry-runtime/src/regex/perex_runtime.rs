//! One host compiler/search path. All outcomes are explicit and every result
//! is a UTF-16 span. Collection and cancellation occur outside resource views.

use super::flags::CanonicalFlags;
use super::perex_memory::{Buffer, Charge, MemoryBudget, StorageError};
use super::perex_owner::{BuildError, GcProgram, HostResourceError, InPlace, OwnerError};
use crate::gc::RuntimeHandleScope;
use perex::binding::{
    BoundProgram, BoundProgramError, BoundResources, BoundSubject, ImmutableSubject, PairError,
    SubjectError,
};
use perex::compiler::{self, CompileError, Node, Range};
use perex::executor::{
    ExecError, Frame, Progress, Resources, Run, Scratch, ScratchOwner, ScratchRequirements, Search,
    SearchError, Undo,
};
use perex::input::Position;
use perex::span::Span;
use perex::Budget;

#[derive(Debug)]
pub(crate) enum EngineError {
    Compile(CompileError),
    Build(BuildError),
    Storage(StorageError),
    Subject(SubjectError<OwnerError>),
    Program(BoundProgramError<OwnerError>),
    Execution(ExecError),
    /// Returned by a host callback that stops an operation early. The runtime
    /// reports it, but only the paused-execution witnesses construct it today.
    #[cfg_attr(not(test), allow(dead_code))]
    Cancelled,
    InvalidQuantum,
    InvalidSpan,
    InvalidFlags,
    Type(&'static str),
    /// Returned by a local JS trap. Propagate directly to the ABI boundary;
    /// do not allocate or poll while these unrooted exception bits are held.
    Abrupt(f64),
}

pub(crate) fn charge(budget: &mut Budget, work: usize) -> Result<(), EngineError> {
    let left = budget.remaining().checked_sub(work);
    *budget = Budget::new(left.unwrap_or(0));
    left.map(|_| ())
        .ok_or(EngineError::Execution(ExecError::WorkLimit))
}

impl From<StorageError> for EngineError {
    fn from(error: StorageError) -> Self {
        Self::Storage(error)
    }
}

/// The safepoint poll for a loop that emits many small pieces: once per
/// `POLL_UNITS` units of output, counting each piece as one more unit so a
/// run of empty or one-unit pieces still reaches it. A poll per piece runs the
/// budgeted trigger ladder for a handful of units each, which on a short
/// `split` was a third of the call; `perex_replace_storage::POLL_UNITS`
/// records why 512 keeps the collector's openings without a peak-RSS cost.
pub(crate) struct PieceStride {
    unpolled: usize,
}

impl PieceStride {
    pub(crate) const fn new() -> Self {
        Self { unpolled: 0 }
    }

    /// Whether the piece of `units` just emitted brings the poll due, which it
    /// then resets. The caller runs the poll.
    pub(crate) fn due(&mut self, units: usize) -> bool {
        self.unpolled = self.unpolled.saturating_add(units).saturating_add(1);
        if self.unpolled >= super::perex_replace_storage::POLL_UNITS {
            self.unpolled = 0;
            true
        } else {
            false
        }
    }

    /// Account for a piece and poll when that brings the poll due.
    pub(crate) fn tick(&mut self, units: usize) -> Result<(), EngineError> {
        if self.due(units) {
            poll()
        } else {
            Ok(())
        }
    }
}

/// Normal runtime poll. Test/embedding callers may supply an alternative poll
/// that requests cancellation or forces actual collection. No input/program
/// view or scratch slice is live when any poll is invoked.
pub(crate) fn poll() -> Result<(), EngineError> {
    crate::gc::gc_runtime_safepoint_poll();
    Ok(())
}

/// Parse original rooted storage and emit directly into its final GC owner.
/// Flags are an owned eight-byte value, so no flag-string borrow spans emission.
/// Parser retries retain the same work allowance; growth does not restart it.
/// Initial validation/parsing/emission are synchronous core operations; their
/// finer-grained suspension remains a separate core integration requirement.
pub(crate) fn compile<'scope, S: ImmutableSubject<Error = OwnerError>>(
    scope: &'scope RuntimeHandleScope,
    pattern: &BoundSubject<S>,
    flags: CanonicalFlags,
    budget: &mut Budget,
    memory: &MemoryBudget,
    max_program_bytes: usize,
    poll: &mut impl FnMut() -> Result<(), EngineError>,
) -> Result<GcProgram<'scope>, EngineError> {
    poll()?;
    let mut node_count = 64usize;
    let mut range_count = 64usize;
    let mut nodes = Buffer::<Node>::new(memory, node_count)?;
    let mut ranges = Buffer::<Range>::new(memory, range_count)?;
    loop {
        let prepared = pattern
            .with_view(|input| {
                compiler::prepare(input, flags.as_str(), &mut nodes, &mut ranges, budget)
            })
            .map_err(EngineError::Subject)?;
        match prepared {
            Ok(plan) => {
                // Prepared retains scratch and budget, but no pattern view.
                poll()?;
                return GcProgram::emit(scope, plan, max_program_bytes).map_err(EngineError::Build);
            }
            Err(CompileError::Nodes) => {
                poll()?;
                node_count = node_count.checked_mul(2).ok_or(StorageError::Limit)?;
                nodes = Buffer::new(memory, node_count)?;
            }
            Err(CompileError::Ranges) => {
                poll()?;
                range_count = range_count.checked_mul(2).ok_or(StorageError::Limit)?;
                ranges = Buffer::new(memory, range_count)?;
            }
            Err(error) => return Err(EngineError::Compile(error)),
        }
    }
}

/// Match slots a search per call needs, held inline when they fit: such a call
/// allocates nothing and notes no external bytes. A heap buffer was about a
/// tenth of every short `test` (#10166). Inline slots are still charged to the
/// operation's limit, exactly as a buffer of the same count is. Past `N`
/// slots, and for any growth, they are a heap buffer as before.
pub(crate) enum Slots<'a, T: Copy + Default, const N: usize> {
    /// The slots, how many are in use, and their charge to the limit, which
    /// is released when they are dropped.
    Inline {
        slots: [T; N],
        count: usize,
        _charge: Charge<'a>,
    },
    Heap(Buffer<'a, T>),
}

impl<'a, T: Copy + Default, const N: usize> Slots<'a, T, N> {
    fn new(memory: &'a MemoryBudget, count: usize) -> Result<Self, StorageError> {
        if count <= N {
            let bytes = count
                .checked_mul(std::mem::size_of::<T>())
                .ok_or(StorageError::Limit)?;
            let charge = Charge::new(memory, bytes)?;
            Ok(Self::Inline {
                slots: [T::default(); N],
                count,
                _charge: charge,
            })
        } else {
            Buffer::new(memory, count).map(Self::Heap)
        }
    }
}

impl<T: Copy + Default, const N: usize> std::ops::Deref for Slots<'_, T, N> {
    type Target = [T];
    fn deref(&self) -> &[T] {
        match self {
            Self::Inline { slots, count, .. } => &slots[..*count],
            Self::Heap(buffer) => buffer,
        }
    }
}

impl<T: Copy + Default, const N: usize> std::ops::DerefMut for Slots<'_, T, N> {
    fn deref_mut(&mut self) -> &mut [T] {
        match self {
            Self::Inline { slots, count, .. } => &mut slots[..*count],
            Self::Heap(buffer) => buffer,
        }
    }
}

/// Registers a program can have and still search without allocating on the
/// owned path, which builds its buffers per call. Most programs need far
/// fewer: `/^[a-z]+_[0-9]+$/` needs 2. Frames and undo entries start empty and
/// only grow through `rebuffer`, so they are never inline.
const INLINE_REGISTERS: usize = 8;
/// The most registers the lent cell will grow to hold (#11549). The cell's
/// registers are allocated once per thread and kept, so a program up to this
/// size searches without building per-call buffers after its first call.
///
/// This is the bound on what the cell retains for registers, and the reason
/// it needs no collector accounting: at most `LENT_REGISTERS * 8` bytes
/// (8 KiB) per thread, whatever programs run. dotenv's `LINE` needs 42 (the
/// old fixed 32 sent every one of its searches down the owned path); a
/// program past the bound (hundreds of capture groups) still takes the owned
/// path, whose buffers the operation's `MemoryBudget` bounds and the
/// operation frees.
///
/// A register count is a property of the program, so the choice between the
/// two paths is made before any work, never by a failed attempt.
const LENT_REGISTERS: usize = 1024;
/// Capture spans an `exec` result can have and still be read without
/// allocating.
const INLINE_CAPTURES: usize = 16;

struct MatchBuffers<'a> {
    registers: Slots<'a, usize, INLINE_REGISTERS>,
    frames: Slots<'a, Frame, 0>,
    undo: Slots<'a, Undo, 0>,
}

impl<'a> MatchBuffers<'a> {
    fn new(memory: &'a MemoryBudget, size: ScratchRequirements) -> Result<Self, StorageError> {
        if crate::hot_diag::regex_on() {
            crate::hot_diag::regex_with(|d| d.perex_scratch_allocs += 1);
        }
        Ok(Self {
            registers: Slots::new(memory, size.registers)?,
            frames: Slots::new(memory, size.frames)?,
            undo: Slots::new(memory, size.undo)?,
        })
    }
}

impl ScratchOwner for MatchBuffers<'_> {
    fn scratch(&mut self) -> Scratch<'_> {
        Scratch {
            registers: &mut self.registers,
            frames: &mut self.frames,
            undo: &mut self.undo,
        }
    }
}

/// Scratch a thread lends to one search at a time, instead of building an
/// owner per call (#10166).
///
/// `find_near` built a `MatchBuffers` for every call: a 32-register inline
/// array zeroed and then moved by value into `Search`, which disassembled to a
/// 336-byte `memcpy` at every call and measured as a third of a short
/// `.test()`. Nothing in that scratch depends on the subject, and a search
/// initializes its own live state, so one cell serves every call on the
/// thread. The arrays keep whatever size an earlier call needed, so a loop
/// reaches a steady state that grows nothing and constructs nothing.
///
/// No GC pointer is ever stored here: registers are subject offsets, and
/// frames and undo entries are the engine's own opaque scratch, exactly as in
/// the owned buffers this replaces (see this module's header).
struct ScratchCell {
    /// Grown on demand to the largest register count a search on this thread
    /// has needed, never past `LENT_REGISTERS`.
    registers: Vec<usize>,
    frames: Vec<Frame>,
    undo: Vec<Undo>,
}

crate::perry_thread_local! {
    /// Borrowed for one search. A nested regex — a replacer callback that runs
    /// its own match, or a poll that re-enters — finds the cell borrowed and
    /// takes the owned path, so two searches never share slots. This is the
    /// runtime half of the guarantee perex's `ScratchOwner for &mut O` makes at
    /// compile time within a single frame.
    ///
    /// `perry_thread_local!` rather than the raw macro (#7469): every search on
    /// this thread reads it, so the address belongs in the hot cache instead of
    /// costing a `_tlv_get_addr` call — the opposite of what this change is for.
    static LENT_SCRATCH: std::cell::RefCell<ScratchCell> = const {
        std::cell::RefCell::new(ScratchCell {
            registers: Vec::new(),
            frames: Vec::new(),
            undo: Vec::new(),
        })
    };
}

/// One call in `PRE_SEARCH_POLL_STRIDE` runs the pre-search safepoint poll.
///
/// The poll costs 502 instructions of the 4,792 a hoisted `.test()` call takes,
/// and measurement says it buys very little (#10166). It cannot cancel: nothing
/// in production constructs `EngineError::Cancelled`, and `host::poll` returns
/// `Ok(())` unconditionally. It performs no cycle stepping in practice either —
/// with it removed entirely, `cycle_starts`, `completions` and `steps` were
/// identical across 48,000,000 allocation-free calls interleaved with churn.
///
/// What it does retain is the one thing a witness could not rule out: the
/// option of servicing a due collection from a loop that allocates nothing,
/// which is how a non-allocating mutator participates in an incremental cycle.
/// That is why it is strided rather than removed. A stride of 64 keeps a
/// participation point every 64 searches while recovering most of the cost.
pub(crate) const PRE_SEARCH_POLL_STRIDE: usize = 64;

crate::perry_thread_local! {
    /// Counts searches for the stride above. A tick count, never an address.
    static PRE_SEARCH_POLL_TICK: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
crate::perry_thread_local! {
    /// Test-only: how many pre-search polls actually ran, so a test can assert
    /// the stride took the poll path rather than infer it from a timing.
    pub(crate) static PRE_SEARCH_POLLS_RUN: std::cell::Cell<usize> =
        const { std::cell::Cell::new(0) };
}

#[cfg(test)]
crate::perry_thread_local! {
    /// Test-only: how many searches took the owned path (built per-call
    /// `MatchBuffers`), so a test can assert which path a program took.
    pub(crate) static OWNED_SEARCHES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// Run the pre-search poll on one call in `PRE_SEARCH_POLL_STRIDE`.
#[inline]
fn poll_on_stride(poll: &mut impl FnMut() -> Result<(), EngineError>) -> Result<(), EngineError> {
    let due = PRE_SEARCH_POLL_TICK.with(|tick| {
        let next = tick.get().wrapping_add(1);
        tick.set(next);
        next % PRE_SEARCH_POLL_STRIDE == 0
    });
    if !due {
        return Ok(());
    }
    #[cfg(test)]
    PRE_SEARCH_POLLS_RUN.with(|n| n.set(n.get() + 1));
    poll()
}

/// What a lent attempt produced: an answer, or a reason to run the owned path.
/// Captures, when asked for, went to the caller's slot; nothing large is
/// carried here, so a decided search moves two words, not its capture slots.
enum Lent {
    Done(Option<Span>, Position),
    /// The cell was already borrowed, or the search asked for more frames or
    /// undo entries than it holds. The cell has been grown to the requested
    /// size, so the next call starts big enough; this call runs the owned path
    /// from its entry budget, exactly as it would have without lending.
    Fallback,
}

#[derive(Clone, Copy)]
pub(crate) enum CaptureMode {
    /// Test/search need only the full match, without allocating output slots.
    Full,
    All,
}

/// Capture slots of one match: None under Full. All retains unset groups and
/// includes group zero.
pub(crate) type Captures<'a> = Slots<'a, Option<Span>, INLINE_CAPTURES>;

pub(crate) struct Match<'a> {
    pub(crate) full: Span,
    /// None under Full. All retains unset groups and includes group zero.
    pub(crate) captures: Option<Captures<'a>>,
}

fn search_error<P: HostResourceError, S: HostResourceError>(
    error: SearchError<PairError<P, S>>,
) -> EngineError {
    match error {
        SearchError::Execution(error) => EngineError::Execution(error),
        SearchError::Resource(PairError::Program(error)) => {
            EngineError::Program(program_error(error))
        }
        SearchError::Resource(PairError::Subject(error)) => {
            EngineError::Subject(subject_error(error))
        }
    }
}

/// A program binding's error in the host's terms.
pub(crate) fn program_error<E: HostResourceError>(
    error: BoundProgramError<E>,
) -> BoundProgramError<OwnerError> {
    match error {
        BoundProgramError::Resource(e) => BoundProgramError::Resource(e.into_owner()),
        BoundProgramError::Validation(e) => BoundProgramError::Validation(e),
        BoundProgramError::ChangedLayout => BoundProgramError::ChangedLayout,
    }
}

/// A subject binding's error in the host's terms.
pub(crate) fn subject_error<E: HostResourceError>(
    error: SubjectError<E>,
) -> SubjectError<OwnerError> {
    match error {
        SubjectError::Resource(e) => SubjectError::Resource(e.into_owner()),
        SubjectError::Encoding(e) => SubjectError::Encoding(e),
        SubjectError::ChangedLayout => SubjectError::ChangedLayout,
    }
}

/// What a search's polls do to the resources it reads.
///
/// A poll may collect, and a collection may move the program cell and the
/// subject string. Owner-based bindings (`GcProgram`, `HeapSubject`) re-read
/// their base from a registered root on every view and need nothing here
/// (`()`). Resources read in place (`perex_owner::InPlace`) start unrooted:
/// `before_poll` gives them roots, and every view after it reads its base
/// through them. Every poll a search runs goes through [`poll_with`], so none
/// runs before the hook.
pub(crate) trait PollRoots {
    fn before_poll(&self) {}
    fn after_poll(&self) {}
}

impl PollRoots for () {}

/// The one way a search polls: after the resources' [`PollRoots`] hook.
#[inline]
fn poll_with<H: PollRoots>(
    hooks: &H,
    poll: &mut impl FnMut() -> Result<(), EngineError>,
) -> Result<(), EngineError> {
    hooks.before_poll();
    let polled = poll();
    hooks.after_poll();
    polled
}

/// Copy a decided match's captures into the caller's slot under `All`.
fn take_captures<'mem>(
    mode: CaptureMode,
    count: usize,
    memory: &'mem MemoryBudget,
    captures: &mut Option<Captures<'mem>>,
    copy: impl FnOnce(&mut [Option<Span>]) -> Result<(), ExecError>,
) -> Result<(), EngineError> {
    if let CaptureMode::All = mode {
        let output = captures.insert(Slots::new(memory, count)?);
        copy(output).map_err(EngineError::Execution)?;
    }
    Ok(())
}

/// Find from an absolute UTF-16 position in the complete original string.
/// The caller supplies the same budget across repeated global searches.
pub(crate) fn find<'mem, S: ImmutableSubject<Error = OwnerError>>(
    program: &BoundProgram<GcProgram<'_>>,
    subject: &BoundSubject<S>,
    start: usize,
    mode: CaptureMode,
    budget: &mut Budget,
    memory: &'mem MemoryBudget,
    quantum: usize,
    poll: &mut impl FnMut() -> Result<(), EngineError>,
) -> Result<Option<Match<'mem>>, EngineError> {
    find_near(
        program, subject, start, None, mode, budget, memory, quantum, poll,
    )
    .map(|(found, _)| found)
}

/// `find`, seeking to `start` from `near` when that is closer than either end
/// of the subject, and returning where the search stood: the match's end, or
/// the start of its last attempt (#10164). On non-ASCII storage a search from
/// an end costs up to half the subject, so a loop of them is quadratic.
///
/// `near` must come from a search or reader over this same binding. Another
/// string with an identical layout cannot be detected and would give wrong
/// answers, so callers keep a position only as long as the binding it came from.
pub(crate) fn find_near<'mem, S: ImmutableSubject<Error = OwnerError>>(
    program: &BoundProgram<GcProgram<'_>>,
    subject: &BoundSubject<S>,
    start: usize,
    near: Option<Position>,
    mode: CaptureMode,
    budget: &mut Budget,
    memory: &'mem MemoryBudget,
    quantum: usize,
    poll: &mut impl FnMut() -> Result<(), EngineError>,
) -> Result<(Option<Match<'mem>>, Position), EngineError> {
    let mut captures = None;
    let (full, position) = find_near_into(
        program,
        subject,
        start,
        near,
        mode,
        budget,
        memory,
        quantum,
        &mut captures,
        poll,
    )?;
    Ok((full.map(|full| Match { full, captures }), position))
}

/// [`find_near`] answering the full match and position, with the captures (under
/// `All`, on a match) written to the caller's slot instead of moved out.
#[allow(clippy::too_many_arguments)]
pub(crate) fn find_near_into<'mem, S: ImmutableSubject<Error = OwnerError>>(
    program: &BoundProgram<GcProgram<'_>>,
    subject: &BoundSubject<S>,
    start: usize,
    near: Option<Position>,
    mode: CaptureMode,
    budget: &mut Budget,
    memory: &'mem MemoryBudget,
    quantum: usize,
    captures: &mut Option<Captures<'mem>>,
    poll: &mut impl FnMut() -> Result<(), EngineError>,
) -> Result<(Option<Span>, Position), EngineError> {
    begin(quantum, poll)?;
    let registers = program
        .with_view(|program| program.register_count())
        .map_err(EngineError::Program)?;
    search(
        &BoundResources { program, subject },
        &(),
        registers,
        start,
        near,
        mode,
        budget,
        memory,
        quantum,
        captures,
        poll,
    )
}

/// One builtin search over `program`, the program cell of the RegExp at `re`,
/// and `input`'s own bytes (S6), all at their current addresses: the bound
/// program is the cell, whose witness makes binding it a header compare, and the subject is the string's storage, bound in constant
/// work once it carries `STRING_FLAG_WTF8_VALIDATED`. Nothing is rooted,
/// copied or marked unless the search polls; see `perex_owner::InPlace` for
/// why every poll is safe. `hint` asks for the cross-call position hint of a
/// non-ASCII subject (#10164), which only g/y searches can use. Returns the
/// RegExp's address after the search, which moved only if the search polled.
#[allow(clippy::too_many_arguments)]
#[inline]
pub(crate) fn find_in_place<'mem>(
    re: *const super::RegExpHeader,
    program: *const u8,
    input: *const crate::string::StringHeader,
    start: usize,
    hint: bool,
    mode: CaptureMode,
    budget: &mut Budget,
    memory: &'mem MemoryBudget,
    quantum: usize,
    captures: &mut Option<Captures<'mem>>,
    poll: &mut impl FnMut() -> Result<(), EngineError>,
) -> Result<(Option<Span>, Position, *mut super::RegExpHeader), EngineError> {
    // The pre-search poll runs before either base is read.
    begin(quantum, poll)?;
    // No collecting action from here on except the search's own polls, each
    // of which roots the in-place resources first.
    let place = unsafe { InPlace::new(re, program, input) }
        .map_err(|e| EngineError::Subject(SubjectError::Resource(e)))?;
    let program =
        super::perex_api::bind_witnessed(place.words(), place.witness(), budget, |witness| {
            place.record_witness(witness)
        })?;
    // Binding the subject neither allocates nor collects, so the header
    // `with_string_mut` passes stays current through it, and marking it
    // validated is a flag store into that same header.
    let (subject, identity) = place.with_string_mut(|s| unsafe {
        let identity = if hint {
            super::perex_position_hint::identity_of_header(s)
        } else {
            None
        };
        let subject = super::perex_api::bind_counted(
            place.bytes(),
            (*s).utf16_len as usize,
            (*s).flags & crate::string::STRING_FLAG_WTF8_VALIDATED != 0,
            || (*s).flags |= crate::string::STRING_FLAG_WTF8_VALIDATED,
        )?;
        Ok::<_, EngineError>((subject, identity))
    })?;
    let near = identity.and_then(super::perex_position_hint::lookup);
    let registers = match place.registers() {
        Some(registers) => registers,
        None => {
            let registers = program
                .with_view(|program| program.register_count())
                .map_err(|e| EngineError::Program(program_error(e)))?;
            place.record_registers(registers);
            registers
        }
    };
    let (full, position) = search(
        &BoundResources {
            program: &program,
            subject: &subject,
        },
        &place,
        registers,
        start,
        near,
        mode,
        budget,
        memory,
        quantum,
        captures,
        poll,
    )?;
    if identity.is_some() {
        // Re-read after the search: a collection during it may have moved the
        // string, and the identity must be the one the next call will see.
        if let Some(identity) =
            place.with_string_mut(|s| unsafe { super::perex_position_hint::identity_of_header(s) })
        {
            super::perex_position_hint::record(identity, position);
        }
    }
    Ok((full, position, place.receiver()))
}

/// The per-search preamble: count it, check the quantum, and run the strided
/// pre-search poll while no search state or view exists yet.
#[inline(always)]
fn begin(
    quantum: usize,
    poll: &mut impl FnMut() -> Result<(), EngineError>,
) -> Result<(), EngineError> {
    if crate::hot_diag::regex_on() {
        crate::hot_diag::regex_with(|d| d.perex_searches += 1);
    }
    if quantum == 0 {
        return Err(EngineError::InvalidQuantum);
    }
    poll_on_stride(poll)
}

/// One search over lent scratch. Returns `Fallback` without an answer when the
/// scratch cannot serve this search; the caller then runs the owned path.
///
/// The search decided within its first quantum, which is nearly every
/// per-call search, is the straight line here. A search that pauses continues
/// in [`resume_lent`] and one the lent cell cannot serve in [`search_owned`],
/// both outlined, so the decided search carries neither their state nor
/// their frame.
#[allow(clippy::too_many_arguments)]
#[inline]
fn find_near_lent<'mem, R, RP, RS, H>(
    resources: &R,
    hooks: &H,
    registers: usize,
    start: usize,
    near: Option<Position>,
    mode: CaptureMode,
    budget: &mut Budget,
    memory: &'mem MemoryBudget,
    quantum: usize,
    captures: &mut Option<Captures<'mem>>,
    poll: &mut impl FnMut() -> Result<(), EngineError>,
) -> Result<Lent, EngineError>
where
    R: Resources<Error = PairError<RP, RS>>,
    RP: HostResourceError,
    RS: HostResourceError,
    H: PollRoots,
{
    LENT_SCRATCH.with(|cell| {
        let Ok(mut cell) = cell.try_borrow_mut() else {
            return Ok(Lent::Fallback);
        };
        let cell = &mut *cell;
        if cell.registers.len() < registers && !grow_lent_registers(cell, registers) {
            return Ok(Lent::Fallback);
        }
        // Charged exactly like the owner it replaces: the operation's limit
        // sees the slots a search may use, whether or not they were allocated
        // for it. The thread keeps the memory; the operation only borrows it.
        let bytes = registers
            .checked_mul(std::mem::size_of::<usize>())
            .and_then(|n| {
                n.checked_add(
                    cell.frames
                        .len()
                        .checked_mul(std::mem::size_of::<Frame>())?,
                )
            })
            .and_then(|n| n.checked_add(cell.undo.len().checked_mul(std::mem::size_of::<Undo>())?))
            .ok_or(StorageError::Limit)?;
        let _charge = Charge::new(memory, bytes)?;
        let scratch = Scratch {
            registers: &mut cell.registers[..registers],
            frames: &mut cell.frames[..],
            undo: &mut cell.undo[..],
        };
        // Both views are acquired once for the quantum that decides nearly
        // every per-call search; a `Search` is built and moved only if this
        // one pauses or asks for more scratch. The run is matched where it is
        // returned, so only the arm taken moves out of it.
        // A failed run reports the work it left (perex 0.1.10), so the budget
        // records what the failed call spent, as a failed search did.
        let required = match Search::run(resources, start, near, scratch, *budget, quantum) {
            Err(failed) => {
                *budget = Budget::new(failed.remaining_work);
                return Err(search_error(failed.error));
            }
            Ok(Run::Finished(mut finished)) => {
                *budget = Budget::new(finished.remaining_work());
                let position = finished.position();
                if !finished.matched() {
                    return Ok(Lent::Done(None, position));
                }
                let full = finished
                    .capture(0)
                    .map_err(EngineError::Execution)?
                    .ok_or(EngineError::Execution(ExecError::InvalidProgram))?;
                if let CaptureMode::All = mode {
                    poll_with(hooks, poll)?;
                }
                let count = finished.capture_count();
                take_captures(mode, count, memory, captures, |output| {
                    finished.copy_captures(output)
                })?;
                return Ok(Lent::Done(Some(full), position));
            }
            Ok(Run::Paused(search)) => {
                match resume_lent(search, hooks, mode, budget, memory, quantum, captures, poll)? {
                    Resumed::Done(found, position) => return Ok(Lent::Done(found, position)),
                    Resumed::Grow(required) => required,
                }
            }
        };
        // Grow the cell for the next call and let this one run the owned
        // path, which charges the whole search once from the caller's entry
        // budget. Rebuffering in place would need a second borrow of the cell
        // the paused search held.
        grow_lent_frames(cell, required);
        Ok(Lent::Fallback)
    })
}

/// What a paused lent search came to: an answer, or the scratch it asked for.
enum Resumed {
    Done(Option<Span>, Position),
    Grow(ScratchRequirements),
}

/// The rest of a lent search that did not decide within its first quantum:
/// poll between quanta, or report the frames or undo entries it needs.
#[cold]
#[inline(never)]
#[allow(clippy::too_many_arguments)]
fn resume_lent<'mem, R, RP, RS, H>(
    mut search: Search<'_, R, Scratch<'_>>,
    hooks: &H,
    mode: CaptureMode,
    budget: &mut Budget,
    memory: &'mem MemoryBudget,
    quantum: usize,
    captures: &mut Option<Captures<'mem>>,
    poll: &mut impl FnMut() -> Result<(), EngineError>,
) -> Result<Resumed, EngineError>
where
    R: Resources<Error = PairError<RP, RS>>,
    RP: HostResourceError,
    RS: HostResourceError,
    H: PollRoots,
{
    loop {
        let result = search.advance(quantum);
        *budget = Budget::new(search.remaining_work());
        match result {
            Ok(Progress::NoMatch) => return Ok(Resumed::Done(None, search.position())),
            Ok(Progress::Matched) => {
                let full = search
                    .capture(0)
                    .map_err(EngineError::Execution)?
                    .ok_or(EngineError::Execution(ExecError::InvalidProgram))?;
                if let CaptureMode::All = mode {
                    poll_with(hooks, poll)?;
                }
                let count = search.capture_count();
                take_captures(mode, count, memory, captures, |output| {
                    search.copy_captures(output)
                })?;
                return Ok(Resumed::Done(Some(full), search.position()));
            }
            Ok(Progress::Pending) => poll_with(hooks, poll)?,
            Err(SearchError::Execution(ExecError::Frames | ExecError::Undo)) => {
                return Ok(Resumed::Grow(search.required_scratch()));
            }
            Err(error) => return Err(search_error(error)),
        }
    }
}

/// Grow the lent registers to `registers`, once per thread per new
/// high-water mark; `search` has already checked `registers <= LENT_REGISTERS`.
/// A search initializes the registers it reads, so the fill value is never
/// observed. `false` when the memory is not available.
#[cold]
#[inline(never)]
fn grow_lent_registers(cell: &mut ScratchCell, registers: usize) -> bool {
    if cell
        .registers
        .try_reserve_exact(registers - cell.registers.len())
        .is_err()
    {
        return false;
    }
    cell.registers.resize(registers, 0);
    true
}

/// Grow the lent frames and undo entries to what a search asked for, so the
/// next search of the same program fits.
#[cold]
#[inline(never)]
fn grow_lent_frames(cell: &mut ScratchCell, required: ScratchRequirements) {
    if crate::hot_diag::regex_on() {
        crate::hot_diag::regex_with(|d| d.perex_scratch_grows += 1);
    }
    let frames = required
        .frames
        .max(cell.frames.len().saturating_mul(2))
        .max(8);
    let undo = required.undo.max(cell.undo.len().saturating_mul(2)).max(16);
    if frames > cell.frames.len() {
        cell.frames.resize(frames, Frame::default());
    }
    if undo > cell.undo.len() {
        cell.undo.resize(undo, Undo::default());
    }
}

/// The search every entry above runs once its resources are bound: lent
/// scratch first, owned buffers when the lent cell cannot serve it. Every poll
/// it runs goes through `hooks` ([`PollRoots`]).
#[allow(clippy::too_many_arguments)]
fn search<'mem, R, RP, RS, H>(
    resources: &R,
    hooks: &H,
    registers: usize,
    start: usize,
    near: Option<Position>,
    mode: CaptureMode,
    budget: &mut Budget,
    memory: &'mem MemoryBudget,
    quantum: usize,
    captures: &mut Option<Captures<'mem>>,
    poll: &mut impl FnMut() -> Result<(), EngineError>,
) -> Result<(Option<Span>, Position), EngineError>
where
    R: Resources<Error = PairError<RP, RS>>,
    RP: HostResourceError,
    RS: HostResourceError,
    H: PollRoots,
{
    // Lend the thread's scratch first: a search that fits it constructs and
    // moves nothing (#10166). Anything the cell cannot serve falls through to
    // the owned buffers with the budget it entered on.
    if registers <= LENT_REGISTERS {
        let entry = *budget;
        match find_near_lent(
            resources, hooks, registers, start, near, mode, budget, memory, quantum, captures, poll,
        )? {
            Lent::Done(found, position) => return Ok((found, position)),
            Lent::Fallback => *budget = entry,
        }
    }
    search_owned(
        resources, hooks, registers, start, near, mode, budget, memory, quantum, captures, poll,
    )
}

/// [`search`] over buffers this search owns.
#[cold]
#[inline(never)]
#[allow(clippy::too_many_arguments)]
fn search_owned<'mem, R, RP, RS, H>(
    resources: &R,
    hooks: &H,
    registers: usize,
    start: usize,
    near: Option<Position>,
    mode: CaptureMode,
    budget: &mut Budget,
    memory: &'mem MemoryBudget,
    quantum: usize,
    captures: &mut Option<Captures<'mem>>,
    poll: &mut impl FnMut() -> Result<(), EngineError>,
) -> Result<(Option<Span>, Position), EngineError>
where
    R: Resources<Error = PairError<RP, RS>>,
    RP: HostResourceError,
    RS: HostResourceError,
    H: PollRoots,
{
    let mut size = ScratchRequirements {
        registers,
        frames: 0,
        undo: 0,
    };
    #[cfg(test)]
    OWNED_SEARCHES.with(|n| n.set(n.get() + 1));
    poll_with(hooks, poll)?;
    let buffers = MatchBuffers::new(memory, size)?;
    // A failed run reports the work it left (perex 0.1.10), so the budget
    // records what the failed call spent, as a failed search did.
    let mut search = match Search::run(resources, start, near, buffers, *budget, quantum) {
        Err(failed) => {
            *budget = Budget::new(failed.remaining_work);
            return Err(search_error(failed.error));
        }
        Ok(Run::Finished(mut finished)) => {
            *budget = Budget::new(finished.remaining_work());
            let position = finished.position();
            if !finished.matched() {
                return Ok((None, position));
            }
            let full = finished
                .capture(0)
                .map_err(EngineError::Execution)?
                .ok_or(EngineError::Execution(ExecError::InvalidProgram))?;
            if let CaptureMode::All = mode {
                poll_with(hooks, poll)?;
            }
            let count = finished.capture_count();
            take_captures(mode, count, memory, captures, |output| {
                finished.copy_captures(output)
            })?;
            return Ok((Some(full), position));
        }
        Ok(Run::Paused(search)) => search,
    };
    loop {
        let result = search.advance(quantum);
        // Preserve consumed work even when the following poll cancels/throws,
        // allocation fails, or a scratch replacement cannot fit the cap.
        *budget = Budget::new(search.remaining_work());
        match result {
            Ok(Progress::NoMatch) => return Ok((None, search.position())),
            Ok(Progress::Matched) => {
                let full = search
                    .capture(0)
                    .map_err(EngineError::Execution)?
                    .ok_or(EngineError::Execution(ExecError::InvalidProgram))?;
                if let CaptureMode::All = mode {
                    poll_with(hooks, poll)?;
                }
                let count = search.capture_count();
                take_captures(mode, count, memory, captures, |output| {
                    search.copy_captures(output)
                })?;
                return Ok((Some(full), search.position()));
            }
            Ok(Progress::Pending) => poll_with(hooks, poll)?,
            Err(SearchError::Execution(ExecError::Frames | ExecError::Undo)) => {
                if crate::hot_diag::regex_on() {
                    crate::hot_diag::regex_with(|d| d.perex_scratch_grows += 1);
                }
                let required = search.required_scratch();
                if required.frames > size.frames {
                    size.frames = required
                        .frames
                        .max(size.frames.checked_mul(2).ok_or(StorageError::Limit)?)
                        .max(8);
                }
                if required.undo > size.undo {
                    size.undo = required
                        .undo
                        .max(size.undo.checked_mul(2).ok_or(StorageError::Limit)?)
                        .max(16);
                }
                poll_with(hooks, poll)?;
                let replacement = MatchBuffers::new(memory, size)?;
                search = search
                    .rebuffer(replacement)
                    .map_err(|failure| EngineError::Execution(failure.error))?;
            }
            Err(error) => return Err(search_error(error)),
        }
    }
}

/// AdvanceStringIndex after an empty match. UTF-16 mode keeps the second half
/// of an astral character observable; Unicode mode consumes a complete pair.
#[cfg(test)]
pub(crate) fn advance_empty(
    subject: &BoundSubject<super::perex_owner::HeapSubject<'_>>,
    index: usize,
    unicode: bool,
) -> Result<usize, EngineError> {
    if !unicode {
        return index
            .checked_add(1)
            .ok_or(EngineError::Storage(StorageError::Limit));
    }
    subject
        .with_view(|input| {
            if index >= input.len_utf16() {
                return index.checked_add(1);
            }
            let mut cursor = input.cursor_at(index)?;
            cursor.next_point()?;
            Some(cursor.position())
        })
        .map_err(EngineError::Subject)?
        .ok_or(EngineError::Storage(StorageError::Limit))
}
