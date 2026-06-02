//! Shared C-ABI declarations used across the runtime.
//!
//! Modules under `ffi::` exist to give us ONE canonical place to declare
//! each external C symbol. Re-declaring the same symbol in multiple
//! files with different parameter types is technically UB even when the
//! ABI happens to round-trip the bits cleanly (see issue #856 for the
//! `_setjmp` precedent).

pub mod setjmp;
