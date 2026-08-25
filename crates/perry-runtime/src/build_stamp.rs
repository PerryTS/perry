//! Build identity embedded in every `libperry_runtime` archive.
//!
//! The compiler reads this marker before linking. Keeping it in the runtime
//! crate (rather than a packaging sidecar) means copied archives, Cargo-built
//! archives, compressed npm archives, and platform-suffixed archives all carry
//! their identity with them.

/// Revision/fingerprint produced by `build.rs` from the compiler/runtime
/// contract sources. Clean checkouts use `git:<commit>`; dirty or source-only
/// builds use `src:<sha256>`.
pub const PERRY_RUNTIME_BUILD_ID: &str = env!("PERRY_RUNTIME_BUILD_ID");

/// NUL-terminated record deliberately stored as plain ASCII so the CLI can
/// find it by streaming over either an ar archive (`.a`) or a COFF library
/// (`.lib`) without invoking platform-specific archive tools.
pub const PERRY_RUNTIME_BUILD_STAMP: &str = concat!(
    "PERRY_RUNTIME_BUILD_STAMP_V1|version=",
    env!("CARGO_PKG_VERSION"),
    "|build=",
    env!("PERRY_RUNTIME_BUILD_ID"),
    "\0",
);

// `#[used]` keeps both this reference and its string data in the rlib object
// set copied by perry-runtime-static into libperry_runtime. The symbol stays
// mangled so linking a stdlib archive that also contains perry-runtime cannot
// create a duplicate public C symbol.
#[used]
#[doc(hidden)]
pub static PERRY_RUNTIME_BUILD_STAMP_EMBEDDED: &[u8] = PERRY_RUNTIME_BUILD_STAMP.as_bytes();
