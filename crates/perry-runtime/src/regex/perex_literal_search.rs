//! Non-overlapping StringIndexOf positions, before replacement callbacks.
//! KMP retains only failure offsets; both strings remain in original storage.
use super::perex_match_search::subject;
use super::perex_memory::{Buffer, MemoryBudget};
use super::perex_owner::HeapSubject;
use super::perex_replace_storage::{List, Units};
use super::perex_runtime::{self as host, EngineError};
use crate::gc::RuntimeHandle;
use perex::binding::BoundSubject;
use perex::Budget;

/// A literal-search coordinate is supplied by the host string representation.
/// Both readers must use the same domain: UTF-16 units or original raw bytes.
pub(super) trait Read {
    fn at(&mut self, index: usize, budget: &mut Budget) -> Result<u16, EngineError>;
}

impl Read for Units<'_, '_> {
    fn at(&mut self, index: usize, budget: &mut Budget) -> Result<u16, EngineError> {
        Units::at(self, index, budget)
    }
}

pub(super) fn positions(
    input: &RuntimeHandle<'_>,
    needle: &RuntimeHandle<'_>,
    all: bool,
    output: &mut List<'_>,
    budget: &mut Budget,
    memory: &MemoryBudget,
) -> Result<(), EngineError> {
    each(input, needle, budget, memory, |index, budget| {
        output.push(index as f64, budget)?;
        Ok(!all)
    })
}

/// Visit non-overlapping positions without retaining a position list. The
/// consumer may allocate/collect and returns true to stop immediately.
pub(super) fn each(
    input: &RuntimeHandle<'_>,
    needle: &RuntimeHandle<'_>,
    budget: &mut Budget,
    memory: &MemoryBudget,
    visit: impl FnMut(usize, &mut Budget) -> Result<bool, EngineError>,
) -> Result<(), EngineError> {
    let source = subject(*input)?;
    let pattern = subject(*needle)?;
    each_bound(&source, &pattern, budget, memory, visit)
}

pub(super) fn each_bound(
    source: &BoundSubject<HeapSubject<'_>>,
    pattern: &BoundSubject<HeapSubject<'_>>,
    budget: &mut Budget,
    memory: &MemoryBudget,
    visit: impl FnMut(usize, &mut Budget) -> Result<bool, EngineError>,
) -> Result<(), EngineError> {
    let n = source
        .with_view(|s| s.len_utf16())
        .map_err(EngineError::Subject)?;
    let m = pattern
        .with_view(|s| s.len_utf16())
        .map_err(EngineError::Subject)?;
    let mut source = Units::new(source)?;
    let mut left = Units::new(pattern)?;
    let mut right = Units::new(pattern)?;
    each_readers(
        (n, m),
        &mut source,
        (&mut left, &mut right),
        budget,
        memory,
        visit,
    )
}

/// One KMP implementation for the host's two literal-string domains. Only
/// failure offsets are retained; readers reborrow original storage per step.
pub(super) fn each_readers<S: Read, N: Read>(
    (n, m): (usize, usize),
    source: &mut S,
    (left, right): (&mut N, &mut N),
    budget: &mut Budget,
    memory: &MemoryBudget,
    mut visit: impl FnMut(usize, &mut Budget) -> Result<bool, EngineError>,
) -> Result<(), EngineError> {
    if m == 0 {
        for i in 0..=n {
            host::charge(budget, 1)?;
            if visit(i, budget)? {
                break;
            }
            if i % super::perex_api::QUANTUM == 0 {
                host::poll()?;
            }
        }
        return Ok(());
    }
    if m > n {
        return Ok(());
    }
    let mut failure = Buffer::<usize>::new(memory, m)?;
    let mut j = 0;
    for i in 1..m {
        let unit = left.at(i, budget)?;
        loop {
            if unit == right.at(j, budget)? {
                j += 1;
                break;
            }
            if j == 0 {
                break;
            }
            j = failure[j - 1];
        }
        failure[i] = j;
    }
    let mut q = 0;
    for i in 0..n {
        let unit = source.at(i, budget)?;
        loop {
            if unit == right.at(q, budget)? {
                q += 1;
                break;
            }
            if q == 0 {
                break;
            }
            q = failure[q - 1];
        }
        if q == m {
            if visit(i + 1 - m, budget)? {
                break;
            }
            q = 0;
        }
    }
    Ok(())
}
