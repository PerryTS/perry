use crate::Error;
use std::cell::Cell;
use std::mem::size_of;

/// Counts initialized-buffer capacity, including overlapping owners during
/// growth. Allocator bookkeeping and stack-resident metadata are separate.
pub(crate) struct Memory {
    limit: usize,
    live: Cell<usize>,
    peak: Cell<usize>,
}
impl Memory {
    pub fn new(limit: usize) -> Self {
        Self {
            limit,
            live: Cell::new(0),
            peak: Cell::new(0),
        }
    }
    pub fn live(&self) -> usize {
        self.live.get()
    }
    pub fn peak(&self) -> usize {
        self.peak.get()
    }
    fn add(&self, bytes: usize) -> Result<(), Error> {
        let live = self
            .live
            .get()
            .checked_add(bytes)
            .filter(|n| *n <= self.limit)
            .ok_or(Error::MemoryLimit)?;
        self.live.set(live);
        self.peak.set(self.peak.get().max(live));
        Ok(())
    }
}
struct Reservation<'a> {
    memory: &'a Memory,
    bytes: usize,
}
impl Drop for Reservation<'_> {
    fn drop(&mut self) {
        self.memory.live.set(self.memory.live.get() - self.bytes);
    }
}
pub(crate) struct Buffer<'a, T> {
    values: Vec<T>,
    // Release the native allocation before releasing its accounting.
    _reservation: Reservation<'a>,
}
impl<'a, T: Default + Copy> Buffer<'a, T> {
    pub fn new(memory: &'a Memory, count: usize) -> Result<Self, Error> {
        let bytes = count
            .checked_mul(size_of::<T>())
            .ok_or(Error::MemoryLimit)?;
        memory.add(bytes)?;
        let mut reservation = Reservation { memory, bytes };
        let mut values = Vec::new();
        values
            .try_reserve_exact(count)
            .map_err(|_| Error::Allocation)?;
        let actual = values
            .capacity()
            .checked_mul(size_of::<T>())
            .ok_or(Error::MemoryLimit)?;
        if actual > bytes {
            memory.add(actual - bytes)?;
            reservation.bytes = actual;
        }
        values.resize(count, T::default());
        Ok(Self {
            values,
            _reservation: reservation,
        })
    }
    pub fn as_mut(&mut self) -> &mut [T] {
        &mut self.values
    }
}

pub(crate) fn words(count: usize, limit: usize) -> Result<Vec<u32>, Error> {
    if count
        .checked_mul(size_of::<u32>())
        .is_none_or(|bytes| bytes > limit)
    {
        return Err(Error::MemoryLimit);
    }
    let mut words = Vec::new();
    words
        .try_reserve_exact(count)
        .map_err(|_| Error::Allocation)?;
    if words
        .capacity()
        .checked_mul(size_of::<u32>())
        .is_none_or(|bytes| bytes > limit)
    {
        return Err(Error::MemoryLimit);
    }
    words.resize(count, 0);
    Ok(words)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn overlapping_growth_is_bounded_and_failure_releases_reservations() {
        let memory = Memory::new(96);
        let first = Buffer::<u64>::new(&memory, 8).unwrap();
        assert_eq!(memory.live(), 64);
        assert!(matches!(
            Buffer::<u64>::new(&memory, 8),
            Err(Error::MemoryLimit)
        ));
        assert_eq!(memory.live(), 64);
        let second = Buffer::<u64>::new(&memory, 4).unwrap();
        assert_eq!(memory.peak(), 96);
        drop(first);
        assert_eq!(memory.live(), 32);
        drop(second);
        assert_eq!(memory.live(), 0);
        assert!(matches!(
            Buffer::<u64>::new(&memory, usize::MAX),
            Err(Error::MemoryLimit)
        ));
        assert_eq!(memory.live(), 0);
    }
}
