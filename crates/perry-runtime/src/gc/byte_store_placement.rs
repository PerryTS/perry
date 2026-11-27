#[cfg(test)]
use std::cell::Cell;

/// Placement owns both the Native cutoff and Node Buffer pooling eligibility.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ByteStorePlacement {
    Inline,
    Native,
    PoolView { size: u32 },
}
/// Recheck this cutoff on the unified B4 layout in REPORT.md.
pub(crate) const INLINE_MAX: usize = 4096;
#[cfg(test)]
thread_local! { static TEST_INLINE_MAX: Cell<Option<usize>> = const { Cell::new(None) }; }
#[cfg(test)]
pub(crate) struct ByteStorePolicyTestGuard(Option<usize>);
#[cfg(test)]
impl ByteStorePolicyTestGuard {
    pub(crate) fn new(max: usize) -> Self {
        Self(TEST_INLINE_MAX.with(|c| c.replace(Some(max))))
    }
}
#[cfg(test)]
impl Drop for ByteStorePolicyTestGuard {
    fn drop(&mut self) {
        TEST_INLINE_MAX.with(|c| c.set(self.0));
    }
}
#[inline]
pub(crate) fn byte_store_placement(
    brand: u8,
    init: &crate::buffer::store::Init<'_>,
    byte_len: usize,
) -> ByteStorePlacement {
    use crate::buffer::store::Init;
    if brand == super::GC_TYPE_BUFFER
        && byte_len != 0
        && matches!(init, Init::PoolCopy | Init::PoolUnsafe)
    {
        let requested = crate::object::native_module::buffer_pool_size();
        // The threshold is JavaScript's ToUint32(poolSize) >>> 1.
        let size = if requested.is_finite() {
            requested.trunc().rem_euclid(4294967296.0) as u32
        } else {
            0
        };
        if byte_len < (size >> 1) as usize {
            // The store entry validates capacity only when a new pool is needed.
            return ByteStorePlacement::PoolView { size };
        }
    }
    #[cfg(test)]
    let max = TEST_INLINE_MAX.with(|c| c.get().unwrap_or(INLINE_MAX));
    #[cfg(not(test))]
    let max = INLINE_MAX;
    if byte_len <= max {
        ByteStorePlacement::Inline
    } else {
        ByteStorePlacement::Native
    }
}

#[cfg(test)]
pub(crate) fn byte_store_test_collection_count() -> u64 {
    super::progress::gc_total_collection_count()
}
#[cfg(test)]
pub(crate) fn byte_store_test_external_live_bytes() -> usize {
    super::policy::external_side_live_bytes()
}
