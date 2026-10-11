//! Non-overlapping StringIndexOf positions, before replacement callbacks.
//! KMP retains only failure offsets; both strings remain in original storage.
use super::perex_memory::{Buffer, MemoryBudget};
use super::perex_output::Offsets;
use super::perex_owner::HeapSubject;
use super::perex_replace_storage::Units;
use super::perex_runtime::{self as host, EngineError};
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

/// Non-overlapping positions of `pattern` in `source` (all of them, or the
/// first), for a replacement that collects them before it builds.
pub(super) fn positions(
    source: &BoundSubject<HeapSubject<'_>>,
    pattern: &BoundSubject<HeapSubject<'_>>,
    all: bool,
    output: &mut Offsets,
    budget: &mut Budget,
    memory: &MemoryBudget,
) -> Result<(), EngineError> {
    if ascii_positions(source, pattern, all, output, budget)? {
        return Ok(());
    }
    each_bound(source, pattern, budget, memory, |index, _| {
        output.push(index)?;
        Ok(!all)
    })
}

/// How many bytes of an ASCII subject one step of `ascii_positions` reads
/// between polls.
const ASCII_STEP: usize = 64 * 1024;

/// `positions` when both strings are ASCII: KMP over the bytes in place, the
/// same automaton `each_readers` runs over UTF-16 units, with no per-unit
/// cursor. Returns false, having done nothing, for any other pair.
fn ascii_positions(
    source: &BoundSubject<HeapSubject<'_>>,
    pattern: &BoundSubject<HeapSubject<'_>>,
    all: bool,
    output: &mut Offsets,
    budget: &mut Budget,
) -> Result<bool, EngineError> {
    let Some(needle) = pattern
        .with_view(|p| {
            p.ascii_bytes()
                .map(smallvec::SmallVec::<[u8; 16]>::from_slice)
        })
        .map_err(EngineError::Subject)?
    else {
        return Ok(false);
    };
    let Some(n) = source
        .with_view(|s| s.ascii_bytes().map(<[u8]>::len))
        .map_err(EngineError::Subject)?
    else {
        return Ok(false);
    };
    #[cfg(test)]
    ASCII_HEAP_BUFFERS.with(|n| n.set(n.get() + usize::from(needle.spilled())));
    let m = needle.len();
    if m == 0 {
        for i in 0..=n {
            host::charge(budget, 1)?;
            output.push(i)?;
            if !all {
                break;
            }
        }
        return Ok(true);
    }
    if m > n {
        return Ok(true);
    }
    let mut failure = smallvec::SmallVec::<[usize; 16]>::new();
    failure
        .try_reserve_exact(m)
        .map_err(|_| super::perex_memory::StorageError::Allocation)?;
    failure.resize(m, 0usize);
    #[cfg(test)]
    ASCII_HEAP_BUFFERS.with(|n| n.set(n.get() + usize::from(failure.spilled())));
    let mut j = 0;
    for i in 1..m {
        while j > 0 && needle[i] != needle[j] {
            j = failure[j - 1];
        }
        if needle[i] == needle[j] {
            j += 1;
        }
        failure[i] = j;
    }
    let (mut q, mut i) = (0, 0);
    while i < n {
        let end = (i + ASCII_STEP).min(n);
        host::charge(budget, end - i)?;
        let stop = source
            .with_view(|s| {
                let hay = s.ascii_bytes().ok_or(EngineError::InvalidSpan)?;
                for (k, &unit) in hay[i..end].iter().enumerate() {
                    while q > 0 && unit != needle[q] {
                        q = failure[q - 1];
                    }
                    if unit == needle[q] {
                        q += 1;
                    }
                    if q == m {
                        output.push(i + k + 1 - m)?;
                        if !all {
                            return Ok(true);
                        }
                        q = 0;
                    }
                }
                Ok::<bool, EngineError>(false)
            })
            .map_err(EngineError::Subject)??;
        if stop {
            break;
        }
        i = end;
        if i < n {
            host::poll()?;
        }
    }
    Ok(true)
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

#[cfg(test)]
crate::perry_thread_local! {
    static ASCII_HEAP_BUFFERS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}
#[cfg(test)]
pub(super) fn heap_buffers() -> usize {
    ASCII_HEAP_BUFFERS.with(std::cell::Cell::get)
}
