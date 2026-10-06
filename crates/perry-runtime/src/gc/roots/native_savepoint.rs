//! Native frames are described by statepoints. Non-local exits restore only
//! expression temporaries here; frame roots need no TLS savepoint.
//! Captured GC values belong to `ExceptionState`'s growing catch snapshots,
//! whose scanner visits only the initialized `[..try_depth]` prefix.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) struct FrameRootSavepoint(usize);

#[inline]
pub(crate) fn frame_root_savepoint() -> FrameRootSavepoint {
    FrameRootSavepoint(super::temp_roots::temp_root_depth())
}

pub(crate) fn frame_root_restore(point: FrameRootSavepoint) {
    super::temp_roots::temp_roots_restore(point.0);
}
