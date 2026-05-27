//! Feature parity matrix, split by area.
//!
//! `lib.rs` historically held a single ~1800-line `pub const FEATURES`. As
//! new entries kept landing, the file kept brushing up against the 2000-LOC
//! repo limit, so it's now organised into topical sub-modules. Each module
//! exports a `pub(crate) const ROWS: &[Feature]` that `lib.rs` concatenates
//! into the public [`crate::FEATURES`] via [`LazyLock`].

pub(crate) mod app;
pub(crate) mod interaction;
pub(crate) mod state;
pub(crate) mod styling;
pub(crate) mod system;
pub(crate) mod widgets;

use crate::Feature;

/// All per-area slices in display order. Concatenated lazily by
/// [`crate::FEATURES`].
pub(crate) const ALL: &[&[Feature]] = &[
    app::ROWS,
    widgets::ROWS,
    state::ROWS,
    styling::ROWS,
    interaction::ROWS,
    system::ROWS,
];
