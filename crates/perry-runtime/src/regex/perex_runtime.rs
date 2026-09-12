//! One host compiler/search path. All outcomes are explicit and every result
//! is a UTF-16 span. Collection and cancellation occur outside resource views.

use super::flags::CanonicalFlags;
use super::perex_memory::{Buffer, MemoryBudget, StorageError};
use super::perex_owner::{BuildError, GcProgram, HeapSubject, OwnerError};
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
    crate::gc::gc_runtime_safepoint();
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

struct MatchBuffers<'a> {
    registers: Buffer<'a, usize>,
    frames: Buffer<'a, Frame>,
    undo: Buffer<'a, Undo>,
}

impl<'a> MatchBuffers<'a> {
    fn new(memory: &'a MemoryBudget, size: ScratchRequirements) -> Result<Self, StorageError> {
        Ok(Self {
            registers: Buffer::new(memory, size.registers)?,
            frames: Buffer::new(memory, size.frames)?,
            undo: Buffer::new(memory, size.undo)?,
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

#[derive(Clone, Copy)]
pub(crate) enum CaptureMode {
    /// Test/search need only the full match, without allocating output slots.
    Full,
    All,
}

pub(crate) struct Match<'a> {
    pub(crate) full: Span,
    /// None under Full. All retains unset groups and includes group zero.
    pub(crate) captures: Option<Buffer<'a, Option<Span>>>,
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
    poll()?;
    let buffers = MatchBuffers::new(memory, size)?;
    let mut search = Search::new(&resources, start, buffers, *budget).map_err(search_error)?;
    loop {
        let result = search.advance(quantum);
        // Preserve consumed work even when the following poll cancels/throws,
        // allocation fails, or a scratch replacement cannot fit the cap.
        *budget = Budget::new(search.remaining_work());
        match result {
            Ok(Progress::NoMatch) => return Ok(None),
            Ok(Progress::Matched) => {
                let full = search
                    .capture(0)
                    .map_err(EngineError::Execution)?
                    .ok_or(EngineError::Execution(ExecError::InvalidProgram))?;
                let captures = match mode {
                    CaptureMode::Full => None,
                    CaptureMode::All => {
                        poll()?;
                        let mut output = Buffer::new(memory, search.capture_count())?;
                        search
                            .copy_captures(&mut output)
                            .map_err(EngineError::Execution)?;
                        Some(output)
                    }
                };
                return Ok(Some(Match { full, captures }));
            }
            Ok(Progress::Pending) => poll()?,
            Err(SearchError::Execution(ExecError::Frames | ExecError::Undo)) => {
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
pub(crate) fn advance_empty(
    subject: &BoundSubject<HeapSubject<'_>>,
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
