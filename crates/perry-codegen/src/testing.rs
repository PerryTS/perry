//! Test-support surface for `perry-codegen`'s own integration suites.
//!
//! # Why this module exists
//!
//! `crates/perry-codegen/tests/*.rs` are *external consumers* of this crate:
//! cargo builds the library normally and links the suite against it, so
//! `#[cfg(test)]` items — which exist only in the library's own unit-test
//! build — are unreachable from them.
//!
//! That is how #7493 happened. #7370 made native roots (RS4GC statepoints) the
//! default lowering. `NativeRootsPin` was added so a test asserting on
//! shadow-stack IR could SAY so, the in-crate unit tests were repaired with it
//! — and five integration suites, which had no pin to reach for, were left red
//! (`shadow_slot_hygiene` at 0/12) for as long as it took someone to run the
//! nightly-only tier by hand.
//!
//! # Why a cargo feature and not `#[doc(hidden)] pub`
//!
//! Three shapes were available:
//!
//! 1. `#[doc(hidden)] pub` on the pin, unconditionally. Rejected: the pin would
//!    then exist in every shipped `perry` binary, and — worse — the
//!    thread-local read it adds to `rs4gc_enabled()` would be compiled into the
//!    production decision path. A knob that is merely undocumented is still a
//!    knob; this repo's own history (`PERRY_GC_FORCE_EVACUATE`, the `--pressure`
//!    knob) is a list of modes that existed without anyone having decided they
//!    should.
//! 2. Moving the suites' bodies into in-crate `#[cfg(test)]` modules. Correct in
//!    principle (#5960: it would also put them in the per-PR `cargo-test` gate),
//!    but `native_proof_regressions.rs` alone is 14k lines and the repo caps
//!    files at 2000 (`scripts/check_file_size.sh`). Not a mechanical move.
//! 3. This: a `testing` cargo feature, enabled **only** by `perry-codegen`'s own
//!    `[dev-dependencies]` entry on itself.
//!
//! Option 3 cannot leak into production behaviour, and the argument is
//! structural rather than a promise:
//!
//! * With `testing` off, `NativeRootsPin`, its backing thread-local, and the
//!   `if let Some(pinned) = …` arm at the top of `rs4gc_enabled()` **do not
//!   exist in the artifact**. They are `#[cfg]`-ed out, not merely private or
//!   unreachable, so there is no symbol to call and no branch to take.
//! * The only manifest edge that turns the feature on is a `[dev-dependencies]`
//!   one. Cargo builds dev-dependencies for test/bench targets only, so
//!   `cargo build`, `cargo build --release` and `--profile dist` never see it.
//! * A production dependency edge that enabled it anyway would be a silent
//!   regression, so it is *gated*, not trusted:
//!   `codegen::helpers::testing_feature_gate_tests` scans every workspace
//!   manifest and fails if any non-dev section enables `perry-codegen`'s
//!   `testing` feature. That test lives in `src/`, so it runs in the per-PR
//!   `cargo-test` job — the tier this whole issue is about not being in.
//!
//! # Using it
//!
//! ```ignore
//! // in crates/perry-codegen/tests/<suite>.rs
//! use perry_codegen::testing::NativeRootsPin;
//!
//! #[test]
//! fn asserts_on_shadow_stack_ir() {
//!     let _pin = NativeRootsPin::shadow();
//!     // … assertions about js_shadow_frame_enter / slot binds …
//! }
//! ```
//!
//! The pin is thread-local and restoring, so it is safe under `cargo test`'s
//! default parallelism: one test's pin cannot retarget another's compile.

pub use crate::codegen::helpers::NativeRootsPin;
