use wasmtime::{ensure, format_err, ResourceLimiter, Result, StoreLimits, StoreLimitsBuilder};

/// One budget for all linear memories in a component, including composed core
/// instances. A per-memory cap alone would multiply the grant by memory count.
pub(crate) struct Limits {
    maximum: usize,
    allocated: usize,
    pending: usize,
    counts: StoreLimits,
}

impl Limits {
    pub fn new(maximum: usize, instances: usize) -> Self {
        Self {
            maximum,
            allocated: 0,
            pending: 0,
            counts: StoreLimitsBuilder::new()
                .instances(instances)
                .memories(instances)
                .tables(instances)
                .table_elements(100_000)
                .trap_on_grow_failure(true)
                .build(),
        }
    }
}

impl ResourceLimiter for Limits {
    fn memory_growing(
        &mut self,
        current: usize,
        desired: usize,
        maximum: Option<usize>,
    ) -> Result<bool> {
        self.pending = 0;
        ensure!(
            self.counts.memory_growing(current, desired, maximum)?,
            "memory growth denied"
        );
        let delta = desired
            .checked_sub(current)
            .ok_or_else(|| format_err!("invalid memory growth"))?;
        let allocated = self
            .allocated
            .checked_add(delta)
            .ok_or_else(|| format_err!("memory budget overflow"))?;
        ensure!(
            allocated <= self.maximum,
            "component total memory limit exceeded"
        );
        self.allocated = allocated;
        self.pending = delta;
        Ok(true)
    }

    fn memory_grow_failed(&mut self, error: wasmtime::Error) -> Result<()> {
        self.allocated -= std::mem::take(&mut self.pending);
        self.counts.memory_grow_failed(error)
    }

    fn table_growing(
        &mut self,
        current: usize,
        desired: usize,
        maximum: Option<usize>,
    ) -> Result<bool> {
        self.counts.table_growing(current, desired, maximum)
    }
    fn table_grow_failed(&mut self, error: wasmtime::Error) -> Result<()> {
        self.counts.table_grow_failed(error)
    }
    fn instances(&self) -> usize {
        self.counts.instances()
    }
    fn memories(&self) -> usize {
        self.counts.memories()
    }
    fn tables(&self) -> usize {
        self.counts.tables()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn budget_is_shared_and_failed_growth_is_rolled_back() {
        let mut limits = Limits::new(2 * 65536, 8);
        assert!(limits.memory_growing(0, 65536, None).unwrap());
        assert!(limits.memory_growing(0, 65536, None).unwrap());
        assert!(limits.memory_growing(0, 65536, None).is_err());
        let mut limits = Limits::new(65536, 8);
        assert!(limits.memory_growing(0, 65536, None).unwrap());
        assert!(limits
            .memory_grow_failed(format_err!("allocation failed"))
            .is_err());
        assert!(limits.memory_growing(0, 65536, None).unwrap());
    }
}
