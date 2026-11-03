use std::{
    io,
    os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle},
    ptr,
    time::Duration,
};
use windows_sys::Win32::{
    Foundation::{HANDLE, INVALID_HANDLE_VALUE, WAIT_IO_COMPLETION, WAIT_TIMEOUT},
    System::IO::{
        CreateIoCompletionPort, GetQueuedCompletionStatusEx, OVERLAPPED_ENTRY,
        PostQueuedCompletionStatus,
    },
};

pub const WAKE: usize = usize::MAX;
pub const TIMER: usize = usize::MAX - 1;
pub const STOP: usize = usize::MAX - 2;

#[derive(Debug)]
pub struct Port(OwnedHandle);

#[derive(Clone, Copy, Debug, Default)]
pub struct Entry {
    pub key: usize,
    /// Opaque identity only; never dereferenced by the port or helper thread.
    pub overlapped: usize,
    pub bytes: u32,
    /// NTSTATUS for kernel packets, not GetLastError().
    pub status: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Wait {
    Entries(usize),
    Timeout,
    Apc,
}

/// # Safety
/// The handle must be newly created and uniquely owned, or a failure sentinel.
pub unsafe fn owned(handle: HANDLE) -> io::Result<OwnedHandle> {
    if handle.is_null() || handle == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: callers pass a newly created, uniquely owned kernel handle.
    Ok(unsafe { OwnedHandle::from_raw_handle(handle) })
}

pub fn bool_result(value: i32) -> io::Result<()> {
    if value == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

impl Port {
    pub fn new() -> io::Result<Self> {
        // SAFETY: INVALID_HANDLE_VALUE + null creates an unassociated port.
        // https://learn.microsoft.com/en-us/windows/win32/api/ioapiset/nf-ioapiset-createiocompletionport
        unsafe {
            owned(CreateIoCompletionPort(
                INVALID_HANDLE_VALUE,
                ptr::null_mut(),
                0,
                1,
            ))
        }
        .map(Self)
    }

    pub fn raw(&self) -> HANDLE {
        self.0.as_raw_handle()
    }

    /// # Safety
    /// `handle` must be live, overlapped-capable and quiescent. Windows rejects
    /// a handle associated with another port without changing that association.
    pub unsafe fn associate(&self, handle: HANDLE, key: usize) -> io::Result<()> {
        if key >= STOP {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "reserved key"));
        }
        // SAFETY: caller guarantees a live overlapped handle; self owns the port.
        if unsafe { CreateIoCompletionPort(handle, self.raw(), key, 0) }.is_null() {
            Err(io::Error::last_os_error())
        } else {
            Ok(())
        }
    }

    pub fn post(&self, key: usize, bytes: u32) -> io::Result<()> {
        // SAFETY: self keeps port live. Synthetic packets have no OVERLAPPED.
        // https://learn.microsoft.com/en-us/windows/win32/api/ioapiset/nf-ioapiset-postqueuedcompletionstatus
        bool_result(unsafe { PostQueuedCompletionStatus(self.raw(), bytes, key, ptr::null()) })
    }

    /// Exactly one OS wait. APCs return to the host; they do not restart the wait.
    pub fn wait(
        &self,
        timeout: Option<Duration>,
        alertable: bool,
        out: &mut [Entry],
    ) -> io::Result<Wait> {
        if out.is_empty() {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "empty batch"));
        }
        // SAFETY: all-zero OVERLAPPED_ENTRY is valid writable output storage.
        let mut raw: [OVERLAPPED_ENTRY; 64] = unsafe { std::mem::zeroed() };
        let mut n = 0;
        let ms = timeout.map_or(u32::MAX, |d| {
            d.as_millis()
                .saturating_add(u128::from(d.subsec_nanos() % 1_000_000 != 0))
                .min(u128::from(u32::MAX - 1)) as u32
        });
        // SAFETY: both output pointers are valid for their specified lengths.
        // A successful batch can contain failed I/O; preserve each entry's NTSTATUS.
        // https://learn.microsoft.com/en-us/windows/win32/api/ioapiset/nf-ioapiset-getqueuedcompletionstatusex
        let ok = unsafe {
            GetQueuedCompletionStatusEx(
                self.raw(),
                raw.as_mut_ptr(),
                out.len().min(raw.len()) as u32,
                &mut n,
                ms,
                i32::from(alertable),
            )
        };
        if ok == 0 {
            let e = io::Error::last_os_error();
            return match e.raw_os_error().map(|e| e as u32) {
                Some(WAIT_TIMEOUT) => Ok(Wait::Timeout),
                Some(WAIT_IO_COMPLETION) => Ok(Wait::Apc),
                _ => Err(e),
            };
        }
        for (dst, src) in out.iter_mut().zip(raw.iter()).take(n as usize) {
            *dst = Entry {
                key: src.lpCompletionKey,
                overlapped: src.lpOverlapped as usize,
                bytes: src.dwNumberOfBytesTransferred,
                status: src.Internal as i32,
            };
        }
        Ok(Wait::Entries(n as usize))
    }
}
