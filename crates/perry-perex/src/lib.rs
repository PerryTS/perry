//! Native program ownership for compiler and extension callers of Perex.
//!
//! This adapter uses the same compiler, immutable program format and matcher
//! as Perry's GC adapter. Subjects are borrowed original UTF-8/WTF-8 bytes;
//! results are UTF-16 spans. No subject copy, encoding buffer, cache, fallback
//! engine or durable interior pointer is created. Do not pass movable GC
//! payloads here: Perry's runtime has its separate traced storage adapter.
//!
//! Compile scratch, final program storage and per-search scratch have explicit
//! limits. Rebuffering counts both live owners. Scratch is released on every
//! completion/error. Work is shared with the caller, including initial byte
//! validation and every retry; initial validation/compilation is synchronous.

use perex::binding::{
    BoundProgram, BoundProgramError, BoundResources, BoundSubject, ImmutableProgram, PairError,
    SubjectError,
};
use perex::compiler::{self, CompileError, Node, Range};
use perex::executor::{
    ExecError, Frame, Progress, Scratch, ScratchOwner, ScratchRequirements, Search, SearchError,
    Undo,
};
use perex::input::{EncodingError, Input};
use perex::program::ProgramError;
pub use perex::{span::Span, Budget};
use std::convert::Infallible;
use std::fmt;

mod memory;
use memory::{Buffer, Memory};

/// Allocating convenience API for build tooling. Not used by the runtime.
pub mod tooling;

#[derive(Clone, Copy, Debug)]
pub struct Limits {
    pub program_bytes: usize,
    pub scratch_bytes: usize,
    pub input_bytes: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            program_bytes: 32 * 1024 * 1024,
            scratch_bytes: 64 * 1024 * 1024,
            input_bytes: 64 * 1024 * 1024,
        }
    }
}

#[derive(Debug)]
pub enum Error {
    Compile(CompileError),
    Program(ProgramError),
    Encoding(EncodingError),
    Execution(ExecError),
    Binding(PairError<Infallible, Infallible>),
    MemoryLimit,
    InputLimit,
    /// Pattern text in the `regex` crate's dialect that [`tooling`] cannot
    /// translate into ECMAScript with its meaning intact.
    Dialect(tooling::DialectError),
    WorkLimit,
    Allocation,
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Perex: {self:?}")
    }
}
impl std::error::Error for Error {}

fn charge(budget: &mut Budget, work: usize) -> Result<(), Error> {
    let remaining = budget.remaining().checked_sub(work);
    *budget = Budget::new(remaining.unwrap_or(0));
    remaining.map(|_| ()).ok_or(Error::WorkLimit)
}
fn input_budget(input: &[u8], limits: Limits, budget: &mut Budget) -> Result<(), Error> {
    if input.len() > limits.input_bytes {
        return Err(Error::InputLimit);
    }
    charge(budget, input.len())
}
fn search_error(error: SearchError<PairError<Infallible, Infallible>>) -> Error {
    match error {
        SearchError::Execution(e) => Error::Execution(e),
        SearchError::Resource(e) => Error::Binding(e),
    }
}
struct Words(Vec<u32>);
impl ImmutableProgram for Words {
    type Error = Infallible;
    fn with_words<T>(&self, f: impl FnOnce(&[u32]) -> T) -> Result<T, Infallible> {
        Ok(f(&self.0))
    }
}

pub struct Regex {
    program: BoundProgram<Words>,
    registers: usize,
    captures: usize,
    program_bytes: usize,
    compile_scratch_peak: usize,
}

#[derive(Default, Debug)]
pub struct SearchStats {
    pub scratch_peak_bytes: usize,
    pub scratch_final_bytes: usize,
    pub growths: usize,
    pub work_used: usize,
}

impl Regex {
    pub fn compile(
        pattern: &[u8],
        flags: &str,
        limits: Limits,
        budget: &mut Budget,
    ) -> Result<Self, Error> {
        input_budget(pattern, limits, budget)?;
        charge(budget, flags.len())?;
        let pattern = Input::wtf8(pattern).map_err(Error::Encoding)?;
        let memory = Memory::new(limits.scratch_bytes);
        let (mut node_count, mut range_count) = (64usize, 64usize);
        loop {
            let mut nodes = Buffer::<Node>::new(&memory, node_count)?;
            let mut ranges = Buffer::<Range>::new(&memory, range_count)?;
            match compiler::prepare(pattern, flags, nodes.as_mut(), ranges.as_mut(), budget) {
                Ok(plan) => {
                    let mut words = memory::words(plan.required_words(), limits.program_bytes)?;
                    let (registers, captures) = {
                        let p = plan.emit(&mut words).map_err(Error::Compile)?;
                        (p.register_count(), p.capture_count())
                    };
                    let program_bytes = words.capacity() * std::mem::size_of::<u32>();
                    // Bind validation to this immutable owner once, not on each search.
                    let program =
                        BoundProgram::new(Words(words), budget).map_err(|e| match e.error {
                            BoundProgramError::Validation(e) => Error::Program(e),
                            BoundProgramError::ChangedLayout => {
                                Error::Execution(ExecError::ChangedResources)
                            }
                            BoundProgramError::Resource(e) => match e {},
                        })?;
                    return Ok(Self {
                        program,
                        registers,
                        captures,
                        program_bytes,
                        compile_scratch_peak: memory.peak(),
                    });
                }
                Err(CompileError::Nodes) => {
                    node_count = node_count.checked_mul(2).ok_or(Error::MemoryLimit)?
                }
                Err(CompileError::Ranges) => {
                    range_count = range_count.checked_mul(2).ok_or(Error::MemoryLimit)?
                }
                Err(e) => return Err(Error::Compile(e)),
            }
        }
    }
    pub fn program_bytes(&self) -> usize {
        self.program_bytes
    }
    pub fn compile_scratch_peak_bytes(&self) -> usize {
        self.compile_scratch_peak
    }
    pub fn capture_count(&self) -> usize {
        self.captures
    }
    /// Each named group's name and the capture indices declared under it, in
    /// first-declaration order. A name declared in disjoint alternatives lists
    /// every group that carries it.
    pub fn named_groups(&self) -> Vec<(String, Vec<usize>)> {
        self.program
            .with_view(|program| {
                program
                    .named_groups()
                    .map(|group| {
                        let units: Vec<u16> = group.name_units().collect();
                        let indices = group
                            .capture_indices()
                            .iter()
                            .map(|&i| i as usize)
                            .collect();
                        (String::from_utf16_lossy(&units), indices)
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn is_match(
        &self,
        subject: &[u8],
        limits: Limits,
        budget: &mut Budget,
    ) -> Result<bool, Error> {
        self.find(
            subject,
            0,
            None,
            limits,
            budget,
            &mut SearchStats::default(),
        )
    }

    pub fn find(
        &self,
        subject: &[u8],
        start: usize,
        captures: Option<&mut [Option<Span>]>,
        limits: Limits,
        budget: &mut Budget,
        stats: &mut SearchStats,
    ) -> Result<bool, Error> {
        *stats = SearchStats::default();
        let before = budget.remaining();
        let memory = Memory::new(limits.scratch_bytes);
        let result = self.find_inner(
            subject,
            start,
            captures,
            limits,
            budget,
            &memory,
            &mut stats.growths,
        );
        stats.scratch_peak_bytes = memory.peak();
        stats.scratch_final_bytes = memory.live();
        stats.work_used = before - budget.remaining();
        debug_assert_eq!(stats.scratch_final_bytes, 0);
        result
    }

    #[allow(clippy::too_many_arguments)]
    fn find_inner(
        &self,
        subject: &[u8],
        start: usize,
        captures: Option<&mut [Option<Span>]>,
        limits: Limits,
        budget: &mut Budget,
        memory: &Memory,
        growths: &mut usize,
    ) -> Result<bool, Error> {
        input_budget(subject, limits, budget)?;
        let subject = BoundSubject::new(subject).map_err(|e| match e.error {
            SubjectError::Encoding(e) => Error::Encoding(e),
            SubjectError::ChangedLayout => Error::Execution(ExecError::ChangedResources),
            SubjectError::Resource(e) => match e {},
        })?;
        let resources = BoundResources {
            program: &self.program,
            subject: &subject,
        };
        let mut sizes = ScratchRequirements {
            registers: self.registers,
            frames: 0,
            undo: 0,
        };
        let buffers = MatchBuffers::new(memory, sizes)?;
        let mut search = Search::new(&resources, start, buffers, *budget).map_err(search_error)?;
        loop {
            let outcome = search.advance(4096);
            *budget = Budget::new(search.remaining_work());
            match outcome {
                Ok(Progress::Pending) => {}
                Ok(Progress::NoMatch) => return Ok(false),
                Ok(Progress::Matched) => {
                    if let Some(captures) = captures {
                        charge(budget, self.captures)?;
                        search.copy_captures(captures).map_err(Error::Execution)?;
                    }
                    return Ok(true);
                }
                Err(SearchError::Execution(ExecError::Frames | ExecError::Undo)) => {
                    let need = search.required_scratch();
                    sizes.frames = grow(sizes.frames, need.frames)?;
                    sizes.undo = grow(sizes.undo, need.undo)?;
                    let replacement = MatchBuffers::new(memory, sizes)?;
                    search = search
                        .rebuffer(replacement)
                        .map_err(|e| Error::Execution(e.error))?;
                    *growths += 1;
                }
                Err(e) => return Err(search_error(e)),
            }
        }
    }
}
fn grow(current: usize, required: usize) -> Result<usize, Error> {
    if required <= current {
        return Ok(current);
    }
    required
        .max(8)
        .checked_next_power_of_two()
        .ok_or(Error::MemoryLimit)
}
struct MatchBuffers<'a> {
    registers: Buffer<'a, usize>,
    frames: Buffer<'a, Frame>,
    undo: Buffer<'a, Undo>,
}
impl<'a> MatchBuffers<'a> {
    fn new(memory: &'a Memory, size: ScratchRequirements) -> Result<Self, Error> {
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
            registers: self.registers.as_mut(),
            frames: self.frames.as_mut(),
            undo: self.undo.as_mut(),
        }
    }
}

#[cfg(test)]
mod tests;
