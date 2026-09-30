//! Windows pipes have no write-side shutdown. Drain on a worker before closing.
use super::*;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use windows_sys::Win32::Foundation::{DuplicateHandle, DUPLICATE_SAME_ACCESS};
use windows_sys::Win32::Storage::FileSystem::FlushFileBuffers;
use windows_sys::Win32::System::Threading::GetCurrentProcess;

pub(super) fn is_pipe(driver: &turnloop::Loop, entry: &Entry) -> bool {
    matches!(
        driver.raw_transport(entry.handle),
        Ok(turnloop::RawTransport::Handle(_))
    )
}

/// Windows duplex pipes cannot half-close. Match libuv's 50 ms read grace
/// after draining writes, refreshing it whenever incoming data arrives.
pub(super) fn arm_eof(
    driver: &mut turnloop::Loop,
    id: i64,
    entry: &mut Entry,
) -> Result<(), Error> {
    let at = driver.now() + std::time::Duration::from_millis(50);
    if let Some(handle) = entry.pipe_eof_timer {
        if driver.timer_reset(handle, at) {
            return Ok(());
        }
        let _ = driver.close(handle, token(OP_PIPE_EOF, id));
    }
    let handle = driver.timer(at, None, token(OP_PIPE_EOF, id))?;
    let _ = driver.set_ref(handle, false);
    entry.pipe_eof_timer = Some(handle);
    Ok(())
}

pub(super) fn cancel_eof(driver: &mut turnloop::Loop, id: i64, entry: &mut Entry) {
    if let Some(handle) = entry.pipe_eof_timer.take() {
        let _ = driver.close(handle, token(OP_PIPE_EOF, id));
    }
}

pub(super) fn submit(driver: &mut turnloop::Loop, id: i64, entry: &mut Entry) -> Result<(), Error> {
    let turnloop::RawTransport::Handle(raw) = driver.raw_transport(entry.handle)? else {
        return Err(Error::new(ErrorKind::InvalidInput));
    };
    let mut duplicate = std::ptr::null_mut();
    // SAFETY: the loop still owns the original; the worker receives its own owner.
    let process = unsafe { GetCurrentProcess() };
    if unsafe {
        DuplicateHandle(
            process,
            raw as _,
            process,
            &mut duplicate,
            0,
            0,
            DUPLICATE_SAME_ACCESS,
        )
    } == 0
    {
        return Err(std::io::Error::last_os_error().into());
    }
    let handle = unsafe { OwnedHandle::from_raw_handle(duplicate.cast()) };
    let op = driver.blocking_with(
        move |_| {
            // Pipe FlushFileBuffers waits for the peer to read all buffered bytes.
            // Keep it off the agent, and retain the handle for the duration of the call.
            if unsafe { FlushFileBuffers(handle.as_raw_handle().cast()) } == 0 {
                let error = std::io::Error::last_os_error();
                if matches!(error.raw_os_error(), Some(109 | 232 | 233)) {
                    Ok(turnloop::Payload::U64(0))
                } else {
                    Err(error.into())
                }
            } else {
                Ok(turnloop::Payload::U64(0))
            }
        },
        turnloop::Occupancy::Long,
        token(OP_PIPE_DRAIN, id),
    )?;
    entry.pipe_drain = Some(op);
    census::note_submit(OP_PIPE_DRAIN);
    Ok(())
}
