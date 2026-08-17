//! Blocking Windows named-pipe stream used by fork IPC.
//!
//! The child end is opened with `FILE_FLAG_OVERLAPPED`, as required by Node's
//! libuv IPC bootstrap. Perry drives that same handle with one OVERLAPPED
//! operation per blocking read/write so the transport also works when the
//! forked executable is another compiled Perry program.

use std::io::{self, Read, Write};
use std::sync::{
    atomic::{AtomicIsize, Ordering},
    Arc, Mutex,
};

use windows_sys::Win32::Foundation::{
    CloseHandle, ERROR_BROKEN_PIPE, ERROR_HANDLE_EOF, ERROR_IO_PENDING, ERROR_OPERATION_ABORTED,
    ERROR_PIPE_NOT_CONNECTED, HANDLE, INVALID_HANDLE_VALUE,
};
use windows_sys::Win32::Storage::FileSystem::{ReadFile, WriteFile};
use windows_sys::Win32::System::Pipes::DisconnectNamedPipe;
use windows_sys::Win32::System::Threading::CreateEventW;
use windows_sys::Win32::System::IO::{CancelIoEx, GetOverlappedResult, OVERLAPPED};

struct Inner {
    handle: AtomicIsize,
    overlapped: bool,
    server: bool,
    write_lock: Mutex<()>,
}

impl Drop for Inner {
    fn drop(&mut self) {
        let handle = self.handle.swap(0, Ordering::AcqRel);
        if handle != 0 {
            unsafe {
                CloseHandle(handle as HANDLE);
            }
        }
    }
}

/// Cloneable full-duplex byte stream. Clones deliberately share one OS handle:
/// closing the IPC channel must wake the reader clone as well as stop writes.
pub(crate) struct IpcStream {
    inner: Arc<Inner>,
    read_remaining: u32,
}

impl Clone for IpcStream {
    fn clone(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
            read_remaining: 0,
        }
    }
}

impl IpcStream {
    /// Take ownership of a valid named-pipe handle.
    ///
    /// # Safety
    /// `handle` must be uniquely owned by the caller and must not be closed
    /// after this call.
    pub(crate) unsafe fn from_raw_handle(handle: HANDLE, overlapped: bool, server: bool) -> Self {
        debug_assert!(!handle.is_null() && handle != INVALID_HANDLE_VALUE);
        Self {
            inner: Arc::new(Inner {
                handle: AtomicIsize::new(handle as isize),
                overlapped,
                server,
                write_lock: Mutex::new(()),
            }),
            read_remaining: 0,
        }
    }

    pub(crate) fn try_clone(&self) -> io::Result<Self> {
        Ok(self.clone())
    }

    pub(crate) fn shutdown(&self) -> io::Result<()> {
        let handle = self.inner.handle.swap(0, Ordering::AcqRel);
        if handle == 0 {
            return Ok(());
        }
        unsafe {
            // Cancel every outstanding overlapped read/write before closing.
            // The server disconnect also wakes synchronous parent-side reads.
            let _ = CancelIoEx(handle as HANDLE, std::ptr::null());
            if self.inner.server {
                let _ = DisconnectNamedPipe(handle as HANDLE);
            }
            if CloseHandle(handle as HANDLE) == 0 {
                return Err(io::Error::last_os_error());
            }
        }
        Ok(())
    }

    fn handle(&self) -> io::Result<HANDLE> {
        let handle = self.inner.handle.load(Ordering::Acquire);
        if handle == 0 {
            Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "IPC channel closed",
            ))
        } else {
            Ok(handle as HANDLE)
        }
    }

    fn raw_io(&self, buf: *mut u8, len: usize, write: bool) -> io::Result<usize> {
        let handle = self.handle()?;
        let len = len.min(u32::MAX as usize) as u32;
        if !self.inner.overlapped {
            let mut transferred = 0u32;
            let ok = unsafe {
                if write {
                    WriteFile(
                        handle,
                        buf.cast_const(),
                        len,
                        &mut transferred,
                        std::ptr::null_mut(),
                    )
                } else {
                    ReadFile(handle, buf, len, &mut transferred, std::ptr::null_mut())
                }
            };
            if ok != 0 {
                return Ok(transferred as usize);
            }
            return pipe_result_error();
        }

        let event = unsafe { CreateEventW(std::ptr::null(), 1, 0, std::ptr::null()) };
        if event.is_null() {
            return Err(io::Error::last_os_error());
        }
        let mut overlapped = OVERLAPPED {
            hEvent: event,
            ..Default::default()
        };
        let mut transferred = 0u32;
        let ok = unsafe {
            if write {
                WriteFile(
                    handle,
                    buf.cast_const(),
                    len,
                    &mut transferred,
                    &mut overlapped,
                )
            } else {
                ReadFile(handle, buf, len, &mut transferred, &mut overlapped)
            }
        };
        let result = if ok != 0 {
            Ok(transferred as usize)
        } else {
            let error = io::Error::last_os_error();
            if error.raw_os_error() == Some(ERROR_IO_PENDING as i32) {
                let completed =
                    unsafe { GetOverlappedResult(handle, &overlapped, &mut transferred, 1) };
                if completed != 0 {
                    Ok(transferred as usize)
                } else {
                    pipe_result_error()
                }
            } else {
                pipe_error(error)
            }
        };
        unsafe {
            CloseHandle(event);
        }
        result
    }

    fn raw_read_exact(&self, mut buf: &mut [u8]) -> io::Result<()> {
        while !buf.is_empty() {
            match self.raw_io(buf.as_mut_ptr(), buf.len(), false)? {
                0 => return Err(io::Error::from(io::ErrorKind::UnexpectedEof)),
                n => buf = &mut buf[n..],
            }
        }
        Ok(())
    }

    fn raw_write_all(&self, mut buf: &[u8]) -> io::Result<()> {
        while !buf.is_empty() {
            match self.raw_io(buf.as_ptr().cast_mut(), buf.len(), true)? {
                0 => return Err(io::Error::from(io::ErrorKind::WriteZero)),
                n => buf = &buf[n..],
            }
        }
        Ok(())
    }

    /// Strip libuv's Windows IPC frame and expose only its data payload.
    fn begin_read_frame(&mut self) -> io::Result<()> {
        const HAS_DATA: u32 = 0x01;
        const HAS_SOCKET_XFER: u32 = 0x02;
        const XFER_IS_TCP_CONNECTION: u32 = 0x04;
        const VALID_FLAGS: u32 = HAS_DATA | HAS_SOCKET_XFER | XFER_IS_TCP_CONNECTION;
        const SOCKET_XFER_SIZE: usize = 632;

        loop {
            let mut header = [0u8; 16];
            self.raw_read_exact(&mut header)?;
            let flags = u32::from_le_bytes(header[0..4].try_into().unwrap());
            let data_length = u32::from_le_bytes(header[8..12].try_into().unwrap());
            let reserved2 = u32::from_le_bytes(header[12..16].try_into().unwrap());
            let xfer_flags = flags & (HAS_SOCKET_XFER | XFER_IS_TCP_CONNECTION);
            let valid_xfer = xfer_flags == 0
                || xfer_flags == HAS_SOCKET_XFER
                || xfer_flags == (HAS_SOCKET_XFER | XFER_IS_TCP_CONNECTION);
            if flags & !VALID_FLAGS != 0
                || reserved2 != 0
                || !valid_xfer
                || (flags & HAS_DATA == 0 && data_length != 0)
            {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "invalid libuv IPC frame",
                ));
            }
            if flags & HAS_SOCKET_XFER != 0 {
                // Perry does not yet expose transferred TCP handles on Windows,
                // but consuming the libuv metadata preserves any accompanying
                // user message and keeps the byte stream synchronized.
                let mut xfer = [0u8; SOCKET_XFER_SIZE];
                self.raw_read_exact(&mut xfer)?;
            }
            self.read_remaining = data_length;
            if self.read_remaining != 0 {
                return Ok(());
            }
        }
    }
}

fn pipe_result_error() -> io::Result<usize> {
    pipe_error(io::Error::last_os_error())
}

fn pipe_error(error: io::Error) -> io::Result<usize> {
    match error.raw_os_error().map(|code| code as u32) {
        Some(
            ERROR_BROKEN_PIPE
            | ERROR_HANDLE_EOF
            | ERROR_OPERATION_ABORTED
            | ERROR_PIPE_NOT_CONNECTED,
        ) => Ok(0),
        _ => Err(error),
    }
}

impl Read for IpcStream {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        if self.read_remaining == 0 {
            if let Err(error) = self.begin_read_frame() {
                return if error.kind() == io::ErrorKind::UnexpectedEof {
                    Ok(0)
                } else {
                    Err(error)
                };
            }
        }
        let len = buf.len().min(self.read_remaining as usize);
        let read = self.raw_io(buf.as_mut_ptr(), len, false)?;
        self.read_remaining -= read as u32;
        Ok(read)
    }
}

impl Write for IpcStream {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        let len: u32 = buf
            .len()
            .try_into()
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "IPC frame too large"))?;
        let _guard = self
            .inner
            .write_lock
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let mut header = [0u8; 16];
        header[0..4].copy_from_slice(&1u32.to_le_bytes());
        header[8..12].copy_from_slice(&len.to_le_bytes());
        self.raw_write_all(&header)?;
        self.raw_write_all(buf)?;
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
