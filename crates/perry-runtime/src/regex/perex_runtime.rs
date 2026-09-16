//! One host compiler/search path. All outcomes are explicit and every result
//! is a UTF-16 span. Collection and cancellation occur outside resource views.

use super::flags::CanonicalFlags;
use super::perex_memory::{Buffer, Charge, MemoryBudget, StorageError};
use super::perex_owner::{BuildError, GcProgram, OwnerError};
use crate::gc::RuntimeHandleScope;
use perex::binding::{
    BoundProgram, BoundProgramError, BoundResources, BoundSubject, ImmutableSubject, PairError,
    SubjectError,
};
use perex::compiler::{self, CompileError, Node, Range};
use perex::executor::{
    ExecError, Frame, Progress, Scratch, ScratchOwner, ScratchRequirements, Search, SearchError,
    Undo,
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
/// Registers the lent cell holds. This array is allocated once per thread, not
/// per call, so it is sized for the programs a search may bring rather than
/// for what is cheap to move.
const LENT_REGISTERS: usize = 32;
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
    registers: [usize; LENT_REGISTERS],
    frames: Vec<Frame>,
    undo: Vec<Undo>,
}

thread_local! {
    /// Borrowed for one search. A nested regex — a replacer callback that runs
    /// its own match, or a poll that re-enters — finds the cell borrowed and
    /// takes the owned path, so two searches never share slots. This is the
    /// runtime half of the guarantee perex's `ScratchOwner for &mut O` makes at
    /// compile time within a single frame.
    static LENT_SCRATCH: std::cell::RefCell<ScratchCell> =
        std::cell::RefCell::new(ScratchCell {
            registers: [0; LENT_REGISTERS],
            frames: Vec::new(),
            undo: Vec::new(),
        });
}

/// What a lent attempt produced: an answer, or a reason to run the owned path.
enum Lent<'mem> {
    Done(Option<Match<'mem>>, Position),
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

pub(crate) struct Match<'a> {
    pub(crate) full: Span,
    /// None under Full. All retains unset groups and includes group zero.
    pub(crate) captures: Option<Slots<'a, Option<Span>, INLINE_CAPTURES>>,
}

fn search_error(error: SearchError<PairError<OwnerError, OwnerError>>) -> EngineError {
    match error {
        SearchError::Execution(error) => EngineError::Execution(error),
        SearchError::Resource(PairError::Program(error)) => EngineError::Program(error),
        SearchError::Resource(PairError::Subject(error)) => EngineError::Subject(error),
    }
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
/// One search over lent scratch. Returns `Fallback` without an answer when the
/// scratch cannot serve this search; the caller then runs the owned path.
#[allow(clippy::too_many_arguments)]
fn find_near_lent<'mem, S: ImmutableSubject<Error = OwnerError>>(
    resources: &BoundResources<'_, GcProgram<'_>, S>,
    registers: usize,
    start: usize,
    near: Option<Position>,
    mode: CaptureMode,
    budget: &mut Budget,
    memory: &'mem MemoryBudget,
    quantum: usize,
    poll: &mut impl FnMut() -> Result<(), EngineError>,
) -> Result<Lent<'mem>, EngineError> {
    LENT_SCRATCH.with(|cell| {
        let Ok(mut cell) = cell.try_borrow_mut() else {
            return Ok(Lent::Fallback);
        };
        let cell = &mut *cell;
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
        poll()?;
        let mut search = match near {
            Some(near) => Search::new_near(resources, start, near, scratch, *budget),
            None => Search::new(resources, start, scratch, *budget),
        }
        .map_err(search_error)?;
        loop {
            let result = search.advance(quantum);
            *budget = Budget::new(search.remaining_work());
            match result {
                Ok(Progress::NoMatch) => return Ok(Lent::Done(None, search.position())),
                Ok(Progress::Matched) => {
                    let full = search
                        .capture(0)
                        .map_err(EngineError::Execution)?
                        .ok_or(EngineError::Execution(ExecError::InvalidProgram))?;
                    let captures = match mode {
                        CaptureMode::Full => None,
                        CaptureMode::All => {
                            poll()?;
                            let mut output = Slots::new(memory, search.capture_count())?;
                            search
                                .copy_captures(&mut output)
                                .map_err(EngineError::Execution)?;
                            Some(output)
                        }
                    };
                    let position = search.position();
                    return Ok(Lent::Done(Some(Match { full, captures }), position));
                }
                Ok(Progress::Pending) => poll()?,
                Err(SearchError::Execution(ExecError::Frames | ExecError::Undo)) => {
                    // Grow the cell for the next call and let this one run the
                    // owned path, which charges the whole search once from the
                    // caller's entry budget. Rebuffering in place would need a
                    // second borrow of the cell the search already holds.
                    let required = search.required_scratch();
                    drop(search);
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
                    return Ok(Lent::Fallback);
                }
                Err(error) => return Err(search_error(error)),
            }
        }
    })
}

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
    if crate::hot_diag::regex_on() {
        crate::hot_diag::regex_with(|d| d.perex_searches += 1);
    }
    if quantum == 0 {
        return Err(EngineError::InvalidQuantum);
    }
    let registers = program
        .with_view(|program| program.register_count())
        .map_err(EngineError::Program)?;
    let resources = BoundResources { program, subject };
    let mut size = ScratchRequirements {
        registers,
        frames: 0,
        undo: 0,
    };
    // Lend the thread's scratch first: a search that fits it constructs and
    // moves nothing (#10166). Anything the cell cannot serve falls through to
    // the owned buffers below with the budget it entered on.
    if registers <= LENT_REGISTERS {
        let entry = *budget;
        match find_near_lent(
            &resources, registers, start, near, mode, budget, memory, quantum, poll,
        )? {
            Lent::Done(found, position) => return Ok((found, position)),
            Lent::Fallback => *budget = entry,
        }
    }

    poll()?;
    let buffers = MatchBuffers::new(memory, size)?;
    let mut search = match near {
        Some(near) => Search::new_near(&resources, start, near, buffers, *budget),
        None => Search::new(&resources, start, buffers, *budget),
    }
    .map_err(search_error)?;
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
                let captures = match mode {
                    CaptureMode::Full => None,
                    CaptureMode::All => {
                        poll()?;
                        let mut output = Slots::new(memory, search.capture_count())?;
                        search
                            .copy_captures(&mut output)
                            .map_err(EngineError::Execution)?;
                        Some(output)
                    }
                };
                return Ok((Some(Match { full, captures }), search.position()));
            }
            Ok(Progress::Pending) => poll()?,
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
                poll()?;
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
