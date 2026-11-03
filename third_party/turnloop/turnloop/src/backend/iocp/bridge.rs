//! A migrated file object cannot change its IOCP association. The low event bit
//! suppresses old-port packets; a registered event wait forwards to the new port.
use super::{
    bool_result, os_error,
    port::{Port, owned},
};
use crate::Result;
use std::{
    ffi::c_void,
    os::windows::io::{AsRawHandle, OwnedHandle},
    ptr,
    sync::{
        Arc,
        atomic::{AtomicI32, AtomicU32, Ordering},
    },
};
use windows_sys::Win32::{
    Foundation::*,
    System::{
        IO::{CancelIoEx, GetOverlappedResult, OVERLAPPED, PostQueuedCompletionStatus},
        Threading::*,
    },
};

pub(super) const KEY: usize = 2;
#[cfg(test)]
thread_local! { pub(super) static REGISTRATIONS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) }; }
struct Context {
    port: Arc<Port>,
    handle: usize,
    overlapped: usize,
    error: AtomicI32,
    bytes: AtomicU32,
}
pub(super) struct Bridge {
    context: Box<Context>,
    event: Option<OwnedHandle>,
    wait: HANDLE,
}
unsafe extern "system" fn complete(context: *mut c_void, _: bool) {
    // SAFETY: stable context remains owned until UnregisterWaitEx joins this callback.
    let context = unsafe { &*context.cast::<Context>() };
    let mut bytes = 0;
    // SAFETY: signaled event proves native I/O is complete; handle/OVERLAPPED are retained.
    let ok = unsafe {
        GetOverlappedResult(
            context.handle as HANDLE,
            context.overlapped as *const OVERLAPPED,
            &mut bytes,
            0,
        )
    };
    let error = if ok == 0 {
        std::io::Error::last_os_error()
            .raw_os_error()
            .unwrap_or(ERROR_GEN_FAILURE as i32)
    } else {
        0
    };
    context.bytes.store(bytes, Ordering::Relaxed);
    context.error.store(error, Ordering::Release);
    // SAFETY: port and operation are retained; pointer is only an opaque routing identity.
    if unsafe {
        PostQueuedCompletionStatus(
            context.port.raw(),
            bytes,
            KEY,
            context.overlapped as *const OVERLAPPED,
        )
    } == 0
    {
        // A lost completion could let caller buffers be freed while teardown waits forever.
        std::process::abort();
    }
}
impl Bridge {
    pub(super) fn new(port: Arc<Port>, overlapped: *mut OVERLAPPED) -> Result<Self> {
        Ok(Self {
            context: Box::new(Context {
                port,
                handle: 0,
                overlapped: overlapped as usize,
                error: AtomicI32::new(0),
                bytes: AtomicU32::new(0),
            }),
            event: None,
            wait: ptr::null_mut(),
        })
    }
    pub(super) fn prepare(&mut self, handle: HANDLE) -> Result<HANDLE> {
        assert!(self.wait.is_null());
        self.context.handle = handle as usize;
        self.context.error.store(0, Ordering::Relaxed);
        if self.event.is_none() {
            // SAFETY: lazily create a noninherited, owned manual-reset event.
            self.event = Some(unsafe { owned(CreateEventW(ptr::null(), 1, 0, ptr::null())) }?);
        }
        let event = self.event.as_ref().expect("event").as_raw_handle();
        // SAFETY: live owned event, no outstanding operation or callback.
        bool_result(unsafe { ResetEvent(event) })?;
        Ok(event)
    }
    pub(super) fn start(&mut self) -> Result<()> {
        #[cfg(test)]
        {
            REGISTRATIONS.with(|n| n.set(n.get() + 1));
        }
        // SAFETY: context is stable, event is owned, callback executes at most once;
        // finish/drop joins it before either context or kernel storage is reused.
        if unsafe {
            RegisterWaitForSingleObject(
                &mut self.wait,
                self.event.as_ref().expect("event").as_raw_handle(),
                Some(complete),
                ptr::from_mut(&mut *self.context).cast(),
                INFINITE,
                WT_EXECUTEONLYONCE,
            )
        } == 0
        {
            let error = os_error();
            let mut bytes = 0;
            // SAFETY: setup failed after native submission; cancel and synchronously
            // quiesce before returning an error or releasing caller memory.
            unsafe {
                CancelIoEx(
                    self.context.handle as HANDLE,
                    self.context.overlapped as *const OVERLAPPED,
                );
                GetOverlappedResult(
                    self.context.handle as HANDLE,
                    self.context.overlapped as *const OVERLAPPED,
                    &mut bytes,
                    1,
                );
            }
            self.wait = ptr::null_mut();
            return Err(error);
        }
        Ok(())
    }
    pub(super) fn finish(&mut self) -> Result<Result<u32>> {
        if !self.wait.is_null() {
            // SAFETY: called by the owning loop, never from its callback; joins it.
            bool_result(unsafe { UnregisterWaitEx(self.wait, INVALID_HANDLE_VALUE) })?;
            self.wait = ptr::null_mut();
        }
        let error = self.context.error.load(Ordering::Acquire);
        Ok(if error == 0 {
            Ok(self.context.bytes.load(Ordering::Relaxed))
        } else {
            Err(super::socket::error(error))
        })
    }
}
impl Drop for Bridge {
    fn drop(&mut self) {
        if !self.wait.is_null() && self.finish().is_err() {
            std::process::abort();
        }
    }
}
