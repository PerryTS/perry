//! Streams and their state objects use the ordinary shape-driven JSON walk.
pub(crate) unsafe fn try_stringify_node_stream_json(_: *const u8, _: &mut String) -> bool {
    false
}
