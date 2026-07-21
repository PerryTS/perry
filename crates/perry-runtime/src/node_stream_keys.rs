//! node:stream — hidden-property key accessors (split out of node_stream.rs for the 2000-line
//! file-size gate, #1987). Shares the parent module's constants, hidden-key
//! accessors and state primitives via `use super::*`.
use super::*;

#[inline]
pub(super) fn hidden_chunks_key() -> *mut crate::string::StringHeader {
    hidden_key(READABLE_CHUNKS_KEY)
}

#[inline]
pub(super) fn hidden_error_key() -> *mut crate::string::StringHeader {
    hidden_key(READABLE_ERROR_KEY)
}

#[inline]
pub(super) fn hidden_read_key() -> *mut crate::string::StringHeader {
    hidden_key(READABLE_READ_KEY)
}

#[inline]
pub(super) fn hidden_read_invoked_key() -> *mut crate::string::StringHeader {
    hidden_key(READABLE_READ_INVOKED_KEY)
}

#[inline]
pub(super) fn hidden_default_read_error_key() -> *mut crate::string::StringHeader {
    hidden_key(READABLE_DEFAULT_READ_ERROR_KEY)
}

#[inline]
pub(super) fn hidden_drain_scheduled_key() -> *mut crate::string::StringHeader {
    hidden_key(STREAM_DRAIN_SCHEDULED_KEY)
}

#[inline]
pub(super) fn hidden_readable_scheduled_key() -> *mut crate::string::StringHeader {
    hidden_key(STREAM_READABLE_SCHEDULED_KEY)
}

#[inline]
pub(super) fn hidden_end_scheduled_key() -> *mut crate::string::StringHeader {
    hidden_key(STREAM_END_SCHEDULED_KEY)
}

#[inline]
pub(super) fn hidden_end_emitted_key() -> *mut crate::string::StringHeader {
    hidden_key(STREAM_END_EMITTED_KEY)
}

#[inline]
pub(super) fn hidden_ended_key() -> *mut crate::string::StringHeader {
    hidden_key(STREAM_ENDED_KEY)
}

#[inline]
pub(super) fn hidden_max_listeners_key() -> *mut crate::string::StringHeader {
    hidden_key(STREAM_MAX_LISTENERS_KEY)
}

#[inline]
pub(super) fn hidden_capture_rejections_key() -> *mut crate::string::StringHeader {
    hidden_key(STREAM_CAPTURE_REJECTIONS_KEY)
}

#[inline]
pub(super) fn hidden_write_key() -> *mut crate::string::StringHeader {
    hidden_key(WRITABLE_WRITE_KEY)
}

#[inline]
pub(super) fn hidden_finish_scheduled_key() -> *mut crate::string::StringHeader {
    hidden_key(WRITABLE_FINISH_SCHEDULED_KEY)
}

#[inline]
pub(super) fn hidden_finish_emitted_key() -> *mut crate::string::StringHeader {
    hidden_key(WRITABLE_FINISH_EMITTED_KEY)
}

#[inline]
pub(super) fn hidden_writable_corked_key() -> *mut crate::string::StringHeader {
    hidden_key(WRITABLE_CORKED_KEY)
}

#[inline]
pub(super) fn hidden_writable_buffered_key() -> *mut crate::string::StringHeader {
    hidden_key(WRITABLE_BUFFERED_KEY)
}

#[inline]
pub(super) fn hidden_writable_length_key() -> *mut crate::string::StringHeader {
    hidden_key(WRITABLE_LENGTH_KEY)
}

#[inline]
pub(super) fn hidden_writable_need_drain_key() -> *mut crate::string::StringHeader {
    hidden_key(WRITABLE_NEED_DRAIN_KEY)
}

#[inline]
pub(super) fn hidden_writable_object_mode_key() -> *mut crate::string::StringHeader {
    hidden_key(WRITABLE_OBJECT_MODE_KEY)
}

#[inline]
pub(super) fn hidden_writable_decode_strings_key() -> *mut crate::string::StringHeader {
    hidden_key(WRITABLE_DECODE_STRINGS_KEY)
}

#[inline]
pub(super) fn hidden_writable_default_encoding_key() -> *mut crate::string::StringHeader {
    hidden_key(WRITABLE_DEFAULT_ENCODING_KEY)
}

#[inline]
pub(super) fn hidden_writable_pending_finish_callback_key() -> *mut crate::string::StringHeader {
    hidden_key(WRITABLE_PENDING_FINISH_CALLBACK_KEY)
}

#[inline]
pub(super) fn hidden_writev_key() -> *mut crate::string::StringHeader {
    hidden_key(WRITABLE_WRITEV_KEY)
}

#[inline]
pub(super) fn hidden_writable_final_key() -> *mut crate::string::StringHeader {
    hidden_key(WRITABLE_FINAL_KEY)
}

#[inline]
pub(super) fn hidden_writable_final_invoked_key() -> *mut crate::string::StringHeader {
    hidden_key(WRITABLE_FINAL_INVOKED_KEY)
}

#[inline]
pub(super) fn hidden_writable_final_pending_key() -> *mut crate::string::StringHeader {
    hidden_key(WRITABLE_FINAL_PENDING_KEY)
}

#[inline]
pub(super) fn hidden_transform_callback_key() -> *mut crate::string::StringHeader {
    hidden_key(TRANSFORM_CALLBACK_KEY)
}

#[inline]
pub(super) fn hidden_transform_flush_key() -> *mut crate::string::StringHeader {
    hidden_key(TRANSFORM_FLUSH_KEY)
}

#[inline]
pub(super) fn hidden_transform_passthrough_key() -> *mut crate::string::StringHeader {
    hidden_key(TRANSFORM_PASSTHROUGH_KEY)
}

#[inline]
pub(super) fn hidden_transform_finishing_key() -> *mut crate::string::StringHeader {
    hidden_key(TRANSFORM_FINISHING_KEY)
}

#[inline]
pub(super) fn hidden_readable_flag_key() -> *mut crate::string::StringHeader {
    hidden_key(READABLE_FLAG_KEY)
}

#[inline]
pub(super) fn hidden_writable_flag_key() -> *mut crate::string::StringHeader {
    hidden_key(WRITABLE_FLAG_KEY)
}

#[inline]
pub(super) fn hidden_disturbed_key() -> *mut crate::string::StringHeader {
    hidden_key(STREAM_DISTURBED_KEY)
}

#[inline]
pub(super) fn hidden_buffered_key() -> *mut crate::string::StringHeader {
    hidden_key(READABLE_BUFFERED_KEY)
}

#[inline]
pub(super) fn hidden_hwm_key() -> *mut crate::string::StringHeader {
    hidden_key(READABLE_HWM_KEY)
}

#[inline]
pub(super) fn hidden_readable_pending_key() -> *mut crate::string::StringHeader {
    hidden_key(READABLE_PENDING_KEY)
}

#[inline]
pub(super) fn hidden_readable_resume_scheduled_key() -> *mut crate::string::StringHeader {
    hidden_key(READABLE_RESUME_SCHEDULED_KEY)
}

#[inline]
pub(super) fn hidden_stream_pipes_key() -> *mut crate::string::StringHeader {
    hidden_key(STREAM_PIPES_KEY)
}

#[inline]
pub(super) fn hidden_readable_base64_remainder_key() -> *mut crate::string::StringHeader {
    hidden_key(READABLE_BASE64_REMAINDER_KEY)
}

#[inline]
pub(super) fn hidden_stream_pipe_no_end_key() -> *mut crate::string::StringHeader {
    hidden_key(STREAM_PIPE_NO_END_KEY)
}

#[inline]
pub(super) fn hidden_stream_pipe_end_pending_key() -> *mut crate::string::StringHeader {
    hidden_key(STREAM_PIPE_END_PENDING_KEY)
}

#[inline]
pub(super) fn hidden_stream_auto_destroy_key() -> *mut crate::string::StringHeader {
    hidden_key(STREAM_AUTO_DESTROY_KEY)
}

#[inline]
pub(super) fn hidden_stream_emit_close_key() -> *mut crate::string::StringHeader {
    hidden_key(STREAM_EMIT_CLOSE_KEY)
}

#[inline]
pub(super) fn hidden_pipeline_callback_done_key() -> *mut crate::string::StringHeader {
    hidden_key(STREAM_PIPELINE_CALLBACK_DONE_KEY)
}

#[inline]
pub(super) fn hidden_compose_live_pipe_consume_key() -> *mut crate::string::StringHeader {
    hidden_key(STREAM_COMPOSE_LIVE_PIPE_CONSUME_KEY)
}

#[inline]
pub(super) fn readable_flowing_key() -> *mut crate::string::StringHeader {
    hidden_key(b"readableFlowing")
}
