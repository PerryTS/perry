//! A CommonJS module body's frame renders its own location.
//!
//! Kept apart from `error_stack_frames.rs` (the file that renders the frame) so
//! the regression test is not part of the code under test. Included from
//! `error.rs` with `#[cfg(test)] #[path = "error_module_frame_label_tests.rs"]
//! mod module_frame_label_tests;`.

use super::stack_frames::{encode_pcs, render_frames};
use super::MAX_CAPTURED_FRAMES;

/// Characters `encode_pcs` writes per captured address.
const PC_CHARS: usize = 8;
/// Frame budget `materialize_error_stack` passes to `render_frames`.
const RENDER_LIMIT: usize = 10;

/// A module body is registered as `Object.<anonymous> (<path>)`: the frame
/// must print that location verbatim. Appending `(<anonymous>)` gave
/// fastify-plugin's stack parser a line without a file name, and it threw
/// `Cannot convert undefined or null to object` at startup.
#[test]
fn a_module_body_frame_renders_its_own_location() {
    const MODULE_START: usize = 0x0000_7d00_0000_0000;
    const MODULE_LABEL: &[u8] = b"Object.<anonymous> (/app/node_modules/plugin/index.js)";
    crate::builtins::test_publish_bundle_name(MODULE_START as *const u8 as usize, MODULE_LABEL);

    let mut blob = [0u8; MAX_CAPTURED_FRAMES * PC_CHARS];
    let len = encode_pcs(&[MODULE_START + 32], &mut blob);
    assert_eq!(
        render_frames(&blob[..len], RENDER_LIMIT).as_deref(),
        Some("    at Object.<anonymous> (/app/node_modules/plugin/index.js)"),
    );
}
