//! Stream option parsing and method-arity registration, split out of
//! `stream.rs` to keep it under the 2000-line cap.

use super::*;

/// Starting a ReadStream must deliver a constructor-time open failure. Without
/// this transition, event-backed consumers wait forever for data/end/error
/// after `createReadStream()` recorded an invalid path (#9616).
pub(super) fn emit_pending_read_error(id: usize) -> bool {
    let pending = STREAM_REGISTRY.with(|registry| {
        registry.borrow().get(&id).and_then(|state| {
            (state.kind == StreamKind::Read && !state.errored)
                .then(|| state.error_msg.clone())
                .flatten()
        })
    });
    let Some(message) = pending else {
        return false;
    };
    record_stream_error(id, message);
    maybe_close_stream(id, false);
    true
}

pub(super) fn register_stream_method_arities() {
    crate::closure::js_register_closure_arity(write_stream_write_impl as *const u8, 3);
    crate::closure::js_register_closure_arity(write_stream_end_impl as *const u8, 3);
    crate::closure::js_register_closure_arity(write_stream_on_impl as *const u8, 2);
    crate::closure::js_register_closure_arity(write_stream_once_impl as *const u8, 2);
    crate::closure::js_register_closure_arity(write_stream_close_impl as *const u8, 1);
    crate::closure::js_register_closure_arity(stream_emit_impl as *const u8, 2);
    crate::closure::js_register_closure_arity(write_stream_turn_impl as *const u8, 0);
    crate::closure::js_register_closure_arity(read_stream_on_impl as *const u8, 2);
    crate::closure::js_register_closure_arity(read_stream_once_impl as *const u8, 2);
    crate::closure::js_register_closure_arity(read_stream_pipe_impl as *const u8, 2);
    crate::closure::js_register_closure_arity(read_stream_pause_impl as *const u8, 0);
    crate::closure::js_register_closure_arity(read_stream_resume_impl as *const u8, 0);
    crate::closure::js_register_closure_arity(read_stream_is_paused_impl as *const u8, 0);
    crate::closure::js_register_closure_arity(read_stream_close_impl as *const u8, 1);
    crate::closure::js_register_closure_arity(read_stream_resume_from_drain_impl as *const u8, 0);
    crate::closure::js_register_closure_arity(utf8_stream_write_impl as *const u8, 1);
    crate::closure::js_register_closure_arity(utf8_stream_flush_impl as *const u8, 1);
    crate::closure::js_register_closure_arity(utf8_stream_flush_sync_impl as *const u8, 0);
    crate::closure::js_register_closure_arity(utf8_stream_end_impl as *const u8, 0);
    crate::closure::js_register_closure_arity(utf8_stream_destroy_impl as *const u8, 0);
    crate::closure::js_register_closure_arity(utf8_stream_reopen_impl as *const u8, 1);
    crate::closure::js_register_closure_arity(utf8_stream_on_impl as *const u8, 2);
    crate::closure::js_register_closure_arity(utf8_stream_once_impl as *const u8, 2);
    crate::closure::js_register_closure_arity(utf8_stream_off_impl as *const u8, 2);
    crate::closure::js_register_closure_arity(utf8_stream_remove_all_impl as *const u8, 1);
    crate::closure::js_register_closure_arity(utf8_stream_listener_count_impl as *const u8, 1);
    crate::closure::js_register_closure_arity(utf8_stream_emit_impl as *const u8, 2);
    crate::closure::js_register_closure_arity(utf8_periodic_flush_impl as *const u8, 0);
    crate::closure::js_register_closure_arity(utf8_async_open_impl as *const u8, 0);
    crate::closure::js_register_closure_arity(utf8_async_open_done_impl as *const u8, 2);
    crate::closure::js_register_closure_arity(utf8_async_mkdir_done_impl as *const u8, 1);
    crate::closure::js_register_closure_arity(utf8_close_events_impl as *const u8, 0);
}

pub(super) fn init_read_state_from_options(
    path_value: f64,
    options_value: f64,
    supplied_fd: Option<(i32, Option<f64>)>,
) -> StreamState {
    let mut state = StreamState::new(StreamKind::Read);
    state.path = path_from_value(path_value);
    state.flags = file_options_flag(options_value, "r");
    state.high_water_mark =
        option_usize_default(options_value, b"highWaterMark", READ_STREAM_DEFAULT_HWM);
    state.start = option_u64(options_value, b"start");
    state.end = option_u64(options_value, b"end");
    state.position = state.start.unwrap_or(0);
    state.encoding = fs_encoding_option(options_value).filter(|encoding| encoding != "buffer");
    state.auto_close = option_bool_default(options_value, b"autoClose", true);
    state.emit_close = option_bool_default(options_value, b"emitClose", true);

    if let Some((fd, handle)) =
        supplied_fd.or_else(|| options_fd(options_value).map(|fd| (fd, None)))
    {
        state.fd = Some(fd);
        state.owner = handle.map(FdOwner::FileHandle).unwrap_or(FdOwner::External);
        state.position = state.start.unwrap_or_else(|| current_position_for_fd(fd));
        state.opened = fd_is_registered(fd);
        if !state.opened {
            state.error_msg = Some("bad file descriptor".to_string());
        }
        return state;
    }

    if let Some(fd) = numeric_fd_value(path_value) {
        state.fd = Some(fd);
        state.owner = FdOwner::External;
        state.position = state.start.unwrap_or_else(|| current_position_for_fd(fd));
        state.opened = fd_is_registered(fd);
        if !state.opened {
            state.error_msg = Some("bad file descriptor".to_string());
        }
        return state;
    }

    let flag_value = make_flag_value(&state.flags);
    match unsafe { fs_open_sync_result(path_value, flag_value) } {
        Ok(fd) => {
            state.fd = Some(fd);
            state.owner = FdOwner::Path;
            state.opened = true;
        }
        Err((err, _path)) => {
            state.error_msg = Some(err.to_string());
        }
    }
    state
}

pub(super) fn init_write_state_from_options(
    path_value: f64,
    options_value: f64,
    supplied_fd: Option<(i32, Option<f64>)>,
) -> StreamState {
    let mut state = StreamState::new(StreamKind::Write);
    state.path = path_from_value(path_value);
    state.flags = file_options_flag(options_value, "w");
    state.high_water_mark =
        option_usize_default(options_value, b"highWaterMark", WRITE_STREAM_DEFAULT_HWM);
    state.start = option_u64(options_value, b"start");
    state.position = state.start.unwrap_or(0);
    state.auto_close = option_bool_default(options_value, b"autoClose", true);
    state.emit_close = option_bool_default(options_value, b"emitClose", true);

    if let Some((fd, handle)) =
        supplied_fd.or_else(|| options_fd(options_value).map(|fd| (fd, None)))
    {
        state.fd = Some(fd);
        state.owner = handle.map(FdOwner::FileHandle).unwrap_or(FdOwner::External);
        state.opened = fd_is_registered(fd);
        state.position =
            if matches!(state.flags.as_str(), "a" | "a+" | "ax" | "ax+") || fd_append_mode(fd) {
                end_position_for_fd(fd)
            } else {
                state.start.unwrap_or_else(|| current_position_for_fd(fd))
            };
        if !state.opened {
            state.error_msg = Some("bad file descriptor".to_string());
        }
        return state;
    }

    if let Some(fd) = numeric_fd_value(path_value) {
        state.fd = Some(fd);
        state.owner = FdOwner::External;
        state.opened = fd_is_registered(fd);
        state.position = state.start.unwrap_or_else(|| current_position_for_fd(fd));
        if !state.opened {
            state.error_msg = Some("bad file descriptor".to_string());
        }
        return state;
    }

    // #9493: Node's constructor validates the path and opens it on a later
    // turn (`_construct` → `fs.open` on the pool): `fd` stays `null` and
    // `pending` true until then, and a `process.exit()` in this tick leaves
    // no file behind. The throwing validation stays here, on the calling
    // turn; `write_stream_open_step` performs the open.
    crate::fs::validate::validate_path("path", path_value);
    if let Some(decoded) = unsafe { decode_path_value(path_value) } {
        state.path = decoded;
    }
    state.owner = FdOwner::Path;
    state
}
