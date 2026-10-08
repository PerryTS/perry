//! node:stream — state-record slot accessors (split out of node_stream.rs for the 2000-line
//! file-size gate, #1987). Shares the parent module's constants, hidden-key
//! accessors and state primitives via `use super::*`.
use super::*;

#[inline]
pub(super) fn hidden_chunks_key() -> Slot {
    READABLE_CHUNKS_KEY
}

#[inline]
pub(super) fn hidden_error_key() -> Slot {
    READABLE_ERROR_KEY
}

#[inline]
pub(super) fn hidden_read_key() -> Slot {
    READABLE_READ_KEY
}

#[inline]
pub(super) fn hidden_read_invoked_key() -> Slot {
    READABLE_READ_INVOKED_KEY
}

#[inline]
pub(super) fn hidden_default_read_error_key() -> Slot {
    READABLE_DEFAULT_READ_ERROR_KEY
}

#[inline]
pub(super) fn hidden_drain_scheduled_key() -> Slot {
    STREAM_DRAIN_SCHEDULED_KEY
}

#[inline]
pub(super) fn hidden_readable_scheduled_key() -> Slot {
    STREAM_READABLE_SCHEDULED_KEY
}

#[inline]
pub(super) fn hidden_end_scheduled_key() -> Slot {
    STREAM_END_SCHEDULED_KEY
}

#[inline]
pub(super) fn hidden_end_emitted_key() -> Slot {
    STREAM_END_EMITTED_KEY
}

#[inline]
pub(super) fn hidden_ended_key() -> Slot {
    STREAM_ENDED_KEY
}

#[inline]
pub(super) fn hidden_capture_rejections_key() -> Slot {
    STREAM_CAPTURE_REJECTIONS_KEY
}

#[inline]
pub(super) fn hidden_write_key() -> Slot {
    WRITABLE_WRITE_KEY
}

#[inline]
pub(super) fn hidden_finish_scheduled_key() -> Slot {
    WRITABLE_FINISH_SCHEDULED_KEY
}

#[inline]
pub(super) fn hidden_finish_emitted_key() -> Slot {
    WRITABLE_FINISH_EMITTED_KEY
}

#[inline]
pub(super) fn hidden_writable_corked_key() -> Slot {
    WRITABLE_CORKED_KEY
}

#[inline]
pub(super) fn hidden_writable_buffered_key() -> Slot {
    WRITABLE_BUFFERED_KEY
}

#[inline]
pub(super) fn hidden_writable_length_key() -> Slot {
    WRITABLE_LENGTH_KEY
}

#[inline]
pub(super) fn hidden_writable_need_drain_key() -> Slot {
    WRITABLE_NEED_DRAIN_KEY
}

#[inline]
pub(super) fn hidden_writable_object_mode_key() -> Slot {
    WRITABLE_OBJECT_MODE_KEY
}

#[inline]
pub(super) fn hidden_writable_decode_strings_key() -> Slot {
    WRITABLE_DECODE_STRINGS_KEY
}

#[inline]
pub(super) fn hidden_writable_default_encoding_key() -> Slot {
    WRITABLE_DEFAULT_ENCODING_KEY
}

#[inline]
pub(super) fn hidden_writable_pending_finish_callback_key() -> Slot {
    WRITABLE_PENDING_FINISH_CALLBACK_KEY
}

#[inline]
pub(super) fn hidden_writev_key() -> Slot {
    WRITABLE_WRITEV_KEY
}

#[inline]
pub(super) fn hidden_writable_final_key() -> Slot {
    WRITABLE_FINAL_KEY
}

#[inline]
pub(super) fn hidden_writable_final_invoked_key() -> Slot {
    WRITABLE_FINAL_INVOKED_KEY
}

#[inline]
pub(super) fn hidden_writable_final_pending_key() -> Slot {
    WRITABLE_FINAL_PENDING_KEY
}

#[inline]
pub(super) fn hidden_transform_callback_key() -> Slot {
    TRANSFORM_CALLBACK_KEY
}

#[inline]
pub(super) fn hidden_transform_flush_key() -> Slot {
    TRANSFORM_FLUSH_KEY
}

#[inline]
pub(super) fn hidden_transform_passthrough_key() -> Slot {
    TRANSFORM_PASSTHROUGH_KEY
}

#[inline]
pub(super) fn hidden_transform_finishing_key() -> Slot {
    TRANSFORM_FINISHING_KEY
}

#[inline]
pub(super) fn hidden_transform_end_pending_key() -> Slot {
    TRANSFORM_END_PENDING_KEY
}

#[inline]
pub(super) fn hidden_transform_flag_key() -> Slot {
    TRANSFORM_FLAG_KEY
}

#[inline]
pub(super) fn hidden_readable_flag_key() -> Slot {
    READABLE_FLAG_KEY
}

#[inline]
pub(super) fn hidden_writable_flag_key() -> Slot {
    WRITABLE_FLAG_KEY
}

#[inline]
pub(super) fn hidden_disturbed_key() -> Slot {
    STREAM_DISTURBED_KEY
}

#[inline]
pub(super) fn hidden_buffered_key() -> Slot {
    READABLE_BUFFERED_KEY
}

#[inline]
pub(super) fn hidden_hwm_key() -> Slot {
    READABLE_HWM_KEY
}

#[inline]
pub(super) fn hidden_readable_pending_key() -> Slot {
    READABLE_PENDING_KEY
}

#[inline]
pub(super) fn hidden_readable_resume_scheduled_key() -> Slot {
    READABLE_RESUME_SCHEDULED_KEY
}

#[inline]
pub(super) fn hidden_stream_pipes_key() -> Slot {
    STREAM_PIPES_KEY
}

#[inline]
pub(super) fn hidden_readable_base64_remainder_key() -> Slot {
    READABLE_BASE64_REMAINDER_KEY
}

#[inline]
pub(super) fn hidden_readable_utf8_remainder_key() -> Slot {
    READABLE_UTF8_REMAINDER_KEY
}

#[inline]
pub(super) fn hidden_stream_pipe_no_end_key() -> Slot {
    STREAM_PIPE_NO_END_KEY
}

#[inline]
pub(super) fn hidden_stream_pipe_end_pending_key() -> Slot {
    STREAM_PIPE_END_PENDING_KEY
}

#[inline]
pub(super) fn hidden_stream_auto_destroy_key() -> Slot {
    STREAM_AUTO_DESTROY_KEY
}

#[inline]
pub(super) fn hidden_stream_emit_close_key() -> Slot {
    STREAM_EMIT_CLOSE_KEY
}

#[inline]
pub(super) fn hidden_pipeline_callback_done_key() -> Slot {
    STREAM_PIPELINE_CALLBACK_DONE_KEY
}

#[inline]
pub(super) fn hidden_readable_live_push_key() -> Slot {
    STREAM_READABLE_LIVE_PUSH_KEY
}

#[inline]
pub(super) fn readable_flowing_key() -> *mut crate::string::StringHeader {
    hidden_key(b"readableFlowing")
}
