use super::{Detached, Kind, Native, invalid, port::owned};
use crate::{PipeName, Result};
use std::{os::windows::ffi::OsStrExt, ptr};
use windows_sys::Win32::{Foundation::*, Storage::FileSystem::*, System::Pipes::*};

pub(super) fn name(name: &PipeName) -> Result<Vec<u16>> {
    let mut value: Vec<u16> = name.0.as_os_str().encode_wide().collect();
    let prefix: Vec<u16> = r"\\.\pipe\".encode_utf16().collect();
    if !value.starts_with(&prefix) || value.len() == prefix.len() || value.contains(&0) {
        return Err(invalid());
    }
    value.push(0);
    Ok(value)
}
pub(super) fn instance(name: &[u16], first: bool) -> Result<Detached> {
    // SAFETY: validated terminated local name; exclusively owned overlapped byte
    // pipe. FIRST_PIPE_INSTANCE rejects accidental binding to another server.
    let handle = unsafe {
        owned(CreateNamedPipeW(
            name.as_ptr(),
            PIPE_ACCESS_DUPLEX
                | FILE_FLAG_OVERLAPPED
                | if first {
                    FILE_FLAG_FIRST_PIPE_INSTANCE
                } else {
                    0
                },
            PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
            PIPE_UNLIMITED_INSTANCES,
            65536,
            65536,
            0,
            ptr::null(),
        ))
    }?;
    Ok(Detached::new(
        Native::Handle(handle),
        Kind::PipeListener,
        false,
    ))
}
pub(super) fn connect(name: &[u16]) -> Result<Detached> {
    // SAFETY: validated terminated name; noninherited, overlapped client endpoint.
    let handle = unsafe {
        owned(CreateFileW(
            name.as_ptr(),
            GENERIC_READ | GENERIC_WRITE,
            0,
            ptr::null(),
            OPEN_EXISTING,
            FILE_FLAG_OVERLAPPED | SECURITY_SQOS_PRESENT | SECURITY_IDENTIFICATION,
            ptr::null_mut(),
        ))
    }?;
    Ok(Detached::new(Native::Handle(handle), Kind::Pipe, false))
}

// Keys below this belong to ordinary I/O, bridge and synchronous-worker packets.
pub(super) const FIRST_KEY: usize = 4;
use super::{
    bool_result, os_error,
    port::{Entry, Port},
};
use crate::{Error, ErrorKind};
use std::{
    cell::UnsafeCell,
    os::windows::io::{AsRawHandle, OwnedHandle},
    sync::Arc,
};
use windows_sys::Win32::System::{
    IO::{CancelIoEx, GetOverlappedResult, OVERLAPPED},
    Threading::CreateEventW,
};

struct Slot {
    event: OwnedHandle,
    pipe: Option<Detached>,
    waiting: bool,
    ready: Option<Result<()>>,
}
pub(super) struct Listener {
    name: Vec<u16>,
    slots: Box<[Slot]>,
    kernel: Box<[UnsafeCell<OVERLAPPED>]>,
    port: Arc<Port>,
    key: usize,
    cursor: usize,
}
impl Listener {
    pub(super) fn new(name: Vec<u16>, backlog: u32, port: Arc<Port>, key: usize) -> Result<Self> {
        let mut slots = Vec::new();
        slots
            .try_reserve_exact(backlog.max(1) as usize)
            .map_err(|_| Error::new(ErrorKind::ResourceLimit))?;
        for _ in 0..backlog.max(1) {
            // SAFETY: owned, noninherited manual-reset event for connect teardown.
            let event = unsafe { owned(CreateEventW(ptr::null(), 1, 0, ptr::null())) }?;
            slots.push(Slot {
                event,
                pipe: None,
                waiting: false,
                ready: None,
            });
        }
        let kernel = slots
            .iter()
            .map(|_| {
                // SAFETY: inactive OVERLAPPED accepts zero initialization; this
                // separate slab never shares Rust references with mutable metadata.
                UnsafeCell::new(unsafe { std::mem::zeroed() })
            })
            .collect();
        let mut listener = Self {
            kernel,
            name,
            slots: slots.into_boxed_slice(),
            port,
            key,
            cursor: 0,
        };
        for i in 0..listener.slots.len() {
            listener.arm(i, i == 0)?;
        }
        Ok(listener)
    }
    pub(super) fn key(&self) -> usize {
        self.key
    }
    fn arm(&mut self, i: usize, first: bool) -> Result<()> {
        let mut pipe = instance(&self.name, first)?;
        pipe.kind = Kind::Pipe;
        // SAFETY: fresh overlapped pipe, exclusively owned and unassociated.
        unsafe { self.port.associate(pipe.native.raw(), self.key) }?;
        // SAFETY: freshly created named pipe supports skip-success notifications.
        bool_result(unsafe { SetFileCompletionNotificationModes(pipe.native.raw(), 1) })?;
        pipe.port = Some(Arc::clone(&self.port));
        let slot = &mut self.slots[i];
        assert!(!slot.waiting);
        slot.pipe = Some(pipe);
        slot.ready = None;
        // SAFETY: prior operation was dequeued, or this is initial setup. Slab is
        // pinned; event remains owned through all kernel accesses and teardown.
        unsafe {
            *self.kernel[i].get() = std::mem::zeroed();
            (*self.kernel[i].get()).hEvent = slot.event.as_raw_handle();
        }
        // SAFETY: live overlapped pipe with stable, exclusively owned output storage.
        let ok = unsafe {
            ConnectNamedPipe(
                slot.pipe.as_ref().expect("pipe").native.raw(),
                self.kernel[i].get(),
            )
        };
        if ok != 0 {
            slot.ready = Some(Ok(()));
        } else {
            let error = os_error();
            match error.os {
                Some(code) if code == ERROR_IO_PENDING as i32 => slot.waiting = true,
                Some(code) if code == ERROR_PIPE_CONNECTED as i32 => slot.ready = Some(Ok(())),
                _ => return Err(error),
            }
        }
        Ok(())
    }
    pub(super) fn completed(&mut self, entry: &Entry) -> Result<()> {
        let i = self
            .kernel
            .iter()
            .position(|slot| slot.get() as usize == entry.overlapped)
            .ok_or_else(super::invalid)?;
        let slot = &mut self.slots[i];
        if !slot.waiting {
            return Err(super::invalid());
        }
        slot.waiting = false;
        slot.ready = Some(if entry.status < 0 {
            // SAFETY: pure NTSTATUS conversion with no pointers.
            Err(super::socket::error(unsafe {
                windows_sys::Win32::Foundation::RtlNtStatusToDosError(entry.status)
            } as i32))
        } else {
            Ok(())
        });
        Ok(())
    }
    pub(super) fn accept(&mut self) -> Result<Option<Detached>> {
        for offset in 0..self.slots.len() {
            let i = (self.cursor + offset) % self.slots.len();
            if let Some(result) = self.slots[i].ready.take() {
                let pipe = self.slots[i].pipe.take();
                self.cursor = (i + 1) % self.slots.len();
                // Refill the fixed slot at accept re-arm, with no Rust allocation.
                if let Err(error) = self.arm(i, false) {
                    self.slots[i].ready = Some(Err(error));
                }
                result?;
                return Ok(pipe);
            }
        }
        Ok(None)
    }
}

pub(super) struct Connect {
    pub name: Vec<u16>,
    buffer: Vec<u64>,
    bytes: u32,
    event: OwnedHandle,
}
pub(super) fn open(name: Vec<u16>) -> Result<(Detached, Option<Connect>)> {
    match connect(&name) {
        Ok(pipe) => return Ok((pipe, None)),
        Err(error) if error.os == Some(ERROR_PIPE_BUSY as i32) => {}
        Err(error) => return Err(error),
    }
    // FSCTL_PIPE_WAIT is the overlapped equivalent of WaitNamedPipeW. Opening the
    // local NPFS root lets the same IOCP wait and CancelIoEx handle availability;
    // no worker, polling timer, or blocking open is needed on the host thread.
    let root: Vec<u16> = "\\\\.\\pipe\\\0".encode_utf16().collect();
    // SAFETY: terminated local pipe filesystem path, newly owned overlapped handle.
    let handle = unsafe {
        owned(CreateFileW(
            root.as_ptr(),
            GENERIC_READ,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            ptr::null(),
            OPEN_EXISTING,
            FILE_FLAG_OVERLAPPED | FILE_FLAG_BACKUP_SEMANTICS,
            ptr::null_mut(),
        ))
    }?;
    use windows_sys::Wdk::Storage::FileSystem::FILE_PIPE_WAIT_FOR_BUFFER;
    let pipe_name = &name[9..name.len() - 1];
    let bytes = std::mem::offset_of!(FILE_PIPE_WAIT_FOR_BUFFER, Name) + pipe_name.len() * 2;
    let bytes = u32::try_from(bytes).map_err(|_| super::invalid())?;
    let mut buffer = vec![
        0u64;
        (bytes as usize)
            .max(size_of::<FILE_PIPE_WAIT_FOR_BUFFER>())
            .div_ceil(8)
    ];
    // SAFETY: u64 storage is aligned and large enough for the header and full name;
    // it stays fixed for every overlapped request and uses the SDK's native layout.
    unsafe {
        let header = buffer.as_mut_ptr().cast::<FILE_PIPE_WAIT_FOR_BUFFER>();
        (*header).Timeout = -i64::MAX; // cancellable indefinite NT relative wait
        (*header).TimeoutSpecified = true;
        (*header).NameLength = (pipe_name.len() * 2) as u32;
        ptr::copy_nonoverlapping(
            pipe_name.as_ptr(),
            ptr::addr_of_mut!((*header).Name).cast(),
            pipe_name.len(),
        );
    }
    // SAFETY: owned manual-reset event retained with the pending connection state.
    let event = unsafe { owned(CreateEventW(ptr::null(), 1, 0, ptr::null())) }?;
    Ok((
        Detached::new(Native::Handle(handle), Kind::PipeConnecting, false),
        Some(Connect {
            name,
            buffer,
            bytes,
            event,
        }),
    ))
}
impl Connect {
    /// # Safety
    /// The handle/OVERLAPPED and this state must remain live until acknowledgement.
    pub(super) unsafe fn wait(&self, handle: HANDLE, overlapped: *mut OVERLAPPED) -> NTSTATUS {
        use windows_sys::Wdk::Storage::FileSystem::{FSCTL_PIPE_WAIT, NtFsControlFile};
        // SAFETY: caller retains the inactive pinned operation, the root handle and
        // this event/input storage. The event's low bit is clear for IOCP delivery.
        // OVERLAPPED's first two fields are the native IO_STATUS_BLOCK; the APC
        // context carries its original address in the completion-port packet.
        unsafe {
            (*overlapped).Internal = STATUS_PENDING as usize;
            (*overlapped).hEvent = self.event.as_raw_handle();
            NtFsControlFile(
                handle,
                (*overlapped).hEvent,
                None,
                overlapped.cast(),
                overlapped.cast(),
                FSCTL_PIPE_WAIT,
                self.buffer.as_ptr().cast(),
                self.bytes,
                ptr::null_mut(),
                0,
            )
        }
    }
}

impl Drop for Listener {
    fn drop(&mut self) {
        for (slot, kernel) in self.slots.iter().zip(&self.kernel) {
            if slot.waiting {
                // SAFETY: exact owned connect; slab/event stay pinned until the
                // second pass joins kernel access, including during setup failure.
                unsafe {
                    CancelIoEx(
                        slot.pipe.as_ref().expect("instance").native.raw(),
                        kernel.get(),
                    );
                }
            }
        }
        for (slot, kernel) in self.slots.iter().zip(&self.kernel) {
            if slot.waiting {
                let mut bytes = 0;
                // SAFETY: retained operation and owned event; waits only for this
                // cancelled connect, never a client or application buffer.
                let ok = unsafe {
                    GetOverlappedResult(
                        slot.pipe.as_ref().expect("instance").native.raw(),
                        kernel.get(),
                        &mut bytes,
                        1,
                    )
                };
                if ok == 0 && os_error().os == Some(ERROR_IO_INCOMPLETE as i32) {
                    std::process::abort();
                }
            }
        }
        // Queued packets contain opaque pointers only. Listener keys are never
        // reused, so Iocp discards them after release without dereferencing memory.
    }
}

#[cfg(all(test, not(loom)))]
mod tests {
    use crate::*;
    use std::time::Duration;

    fn receive(driver: &mut Loop, h: Handle, out: &mut Completions) -> OpId {
        let op = driver
            .read(h, ReadBuf::Pooled, Token(3))
            .expect("pending read");
        driver
            .turn(Timeout::Now, out)
            .expect("arm read without data");
        assert!(out.is_empty());
        op
    }
    fn send(driver: &mut Loop, h: Handle, out: &mut Completions) -> OpId {
        driver
            .write(h, WriteBuf::Owned(vec![0x9b; 16]), Token(4))
            .expect("write");
        let deadline = driver.now() + Duration::from_secs(5);
        loop {
            assert!(driver.now() < deadline);
            driver
                .turn(Timeout::Until(deadline), out)
                .expect("write completion");
            if out.is_empty() {
                continue;
            }
            assert_eq!(out.len(), 1);
            assert!(matches!(out[0].result, OpResult::Wrote(16)));
            return out[0].op.expect("write identity");
        }
    }
    #[test]
    fn accepted_pipe_uses_no_bridge_until_moved_to_another_port() {
        let mut driver = Loop::new(Config::default()).expect("loop");
        let name = PipeName(format!(r"\\.\pipe\tl-own-port-{}", std::process::id()).into());
        let listener = driver
            .pipe_listen(
                &name,
                &ListenOpts {
                    backlog: 2,
                    ..ListenOpts::default()
                },
            )
            .expect("listener");
        driver.accept(listener, Token(1)).expect("accept");
        let client = driver.pipe_connect(&name, Token(2)).expect("client");
        let mut out = Completions::with_capacity(1);
        let mut server = None;
        let mut connected = 0;
        let deadline = driver.now() + Duration::from_secs(10);
        while server.is_none() || connected == 0 {
            assert!(driver.now() < deadline);
            driver
                .turn(Timeout::Until(deadline), &mut out)
                .expect("pair");
            for c in out.drain() {
                match c.result {
                    OpResult::PipeAccepted { conn } => {
                        assert!(server.replace(conn).is_none());
                    }
                    OpResult::Connected => connected += 1,
                    other => panic!("unexpected {other:?}"),
                }
            }
        }
        let mut server = server.expect("accepted pipe");
        super::super::bridge::REGISTRATIONS.with(|n| n.set(0));
        let mut exchanges = 0;
        for reattach in [false, true] {
            if reattach {
                let detached = driver.detach(server).expect("detach same port");
                server = driver
                    .attach(detached, Token(0))
                    .expect("reattach same port");
            }
            for _ in 0..32 {
                for (reader, writer) in [(server, client), (client, server)] {
                    let op = receive(&mut driver, reader, &mut out);
                    driver
                        .write(writer, WriteBuf::Owned(vec![0x9b; 16]), Token(4))
                        .expect("write");
                    let mut seen = [false; 2];
                    while !seen.into_iter().all(|v| v) {
                        assert!(driver.now() < deadline);
                        driver
                            .turn(Timeout::Until(deadline), &mut out)
                            .expect("exchange");
                        for c in out.drain() {
                            match c.result {
                                OpResult::Read {
                                    n: 16,
                                    lease: Some(bytes),
                                } => {
                                    assert_eq!(c.op, Some(op));
                                    assert_eq!(bytes.as_slice(), [0x9b; 16]);
                                    assert!(!std::mem::replace(&mut seen[0], true));
                                }
                                OpResult::Wrote(16) => {
                                    assert!(!std::mem::replace(&mut seen[1], true));
                                }
                                other => panic!("unexpected {other:?}"),
                            }
                        }
                    }
                    exchanges += 1;
                }
            }
        }
        assert_eq!(exchanges, 128);
        assert_eq!(
            super::super::bridge::REGISTRATIONS.with(std::cell::Cell::get),
            0,
            "same-port accepted pipe must never register a wait"
        );
        let mut other = Loop::new(Config::default()).expect("other port");
        let server = other
            .attach(
                driver.detach(server).expect("detach for migration"),
                Token(0),
            )
            .expect("migrate");
        let op = receive(&mut other, server, &mut out);
        assert_eq!(
            super::super::bridge::REGISTRATIONS.with(std::cell::Cell::get),
            1,
            "positive control: foreign-port pending I/O registers a bridge"
        );
        send(&mut driver, client, &mut out);
        loop {
            assert!(other.now() < deadline);
            other
                .turn(Timeout::Until(deadline), &mut out)
                .expect("routed completion");
            if out.is_empty() {
                continue;
            }
            assert_eq!(out.len(), 1);
            assert_eq!(out[0].op, Some(op));
            let OpResult::Read {
                n: 16,
                lease: Some(data),
            } = &out[0].result
            else {
                panic!("missing routed read")
            };
            assert_eq!(data.as_slice(), [0x9b; 16]);
            break;
        }
    }
}

#[cfg(all(test, not(loom)))]
#[test]
fn unassociated_import_uses_direct_iocp_without_a_bridge() {
    use crate::*;
    use std::{io::Write, time::Duration};
    let listener =
        std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).expect("listener");
    let stream =
        std::net::TcpStream::connect(listener.local_addr().expect("address")).expect("client");
    let (mut peer, _) = listener.accept().expect("peer");
    let mut driver = Loop::new(Config::default()).expect("loop");
    let h = driver
        .attach(
            Detached::from_socket(stream.into()).expect("unassociated import"),
            Token(0),
        )
        .expect("attach");
    super::bridge::REGISTRATIONS.with(|n| n.set(0));
    let op = driver
        .read(h, ReadBuf::Pooled, Token(1))
        .expect("idle read");
    let mut out = Completions::with_capacity(1);
    driver
        .turn(Timeout::Now, &mut out)
        .expect("arm without data");
    assert!(out.is_empty());
    assert_eq!(super::bridge::REGISTRATIONS.with(std::cell::Cell::get), 0);
    peer.write_all(b"direct").expect("send peer bytes");
    let until = driver.now() + Duration::from_secs(5);
    loop {
        assert!(driver.now() < until);
        driver
            .turn(Timeout::Until(until), &mut out)
            .expect("imported direct completion");
        if out.is_empty() {
            continue;
        }
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].op, Some(op));
        let OpResult::Read {
            n: 6,
            lease: Some(bytes),
        } = &out[0].result
        else {
            panic!("missing bytes")
        };
        assert_eq!(bytes.as_slice(), b"direct");
        break;
    }
    assert_eq!(super::bridge::REGISTRATIONS.with(std::cell::Cell::get), 0);
}
