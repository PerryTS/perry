//! Private cooperative named-pipe control protocol. Only sockets are transferred;
//! WSADuplicateSocket targets the pipe's OS-reported peer PID, never a supplied PID.
use super::*;
use windows_sys::Win32::System::Pipes::{
    GetNamedPipeClientProcessId, GetNamedPipeInfo, GetNamedPipeServerProcessId, PIPE_SERVER_END,
};
const MAGIC: u32 = 0x314c_5457; // WTL1
const LENGTH: usize = 4 + size_of::<WSAPROTOCOL_INFOW>();

impl Iocp {
    pub(super) fn ipc_step(
        &mut self,
        i: usize,
        p: &mut Pending,
    ) -> Result<Option<(Outcome<Detached>, bool)>> {
        let raw = self.get(p.request.handle)?.transport.native.raw();
        let send = matches!(p.request.operation, Operation::SendHandle(_));
        if let Some(result) = p.completion.take() {
            let n = result? as usize;
            if n == 0 || n > LENGTH - p.offset {
                return Err(Error::new(ErrorKind::BrokenPipe));
            }
            p.offset += n;
            if p.offset == LENGTH {
                if send {
                    return Ok(Some((Outcome::HandleSent, true)));
                }
                // SAFETY: completed read initialized the complete frame; the word
                // array ensures alignment and initialized padding in protocol data.
                let k = unsafe { &*self.kernel[i].get() };
                if k.wire[0] != MAGIC {
                    return Err(invalid());
                }
                // SAFETY: full validated-size frame; WSAPROTOCOL_INFOW contains no
                // pointers and is only accepted through cooperative local IPC.
                let info = unsafe { &*k.wire.as_ptr().add(1).cast::<WSAPROTOCOL_INFOW>() };
                let socket = socket::reconstruct(info)?;
                let transport = Detached::from_socket(socket)?;
                return Ok(Some((Outcome::HandleReceived(transport), true)));
            }
        }
        let k = if p.stage == Stage::Start {
            let k = self.prepare(i, p)?;
            if send {
                let mut flags = 0;
                let mut pid = 0;
                // SAFETY: live named pipe, valid output; OS identifies the actual peer.
                bool_result(unsafe {
                    GetNamedPipeInfo(
                        raw,
                        &mut flags,
                        ptr::null_mut(),
                        ptr::null_mut(),
                        ptr::null_mut(),
                    )
                })?;
                // SAFETY: connected named pipe and writable PID output.
                bool_result(unsafe {
                    if flags & PIPE_SERVER_END != 0 {
                        GetNamedPipeClientProcessId(raw, &mut pid)
                    } else {
                        GetNamedPipeServerProcessId(raw, &mut pid)
                    }
                })?;
                let socket = p.passed.as_ref().expect("independent send ownership");
                // SAFETY: quiescent aligned word storage, large enough for the frame;
                // duplicate source remains owned through terminal acknowledgement.
                unsafe {
                    (*k).wire[0] = MAGIC;
                    socket::check(WSADuplicateSocketW(
                        socket.as_raw_socket() as usize,
                        pid,
                        ptr::addr_of_mut!((*k).wire).cast::<u32>().add(1).cast(),
                    ))?;
                }
            }
            k
        } else {
            let k = self.kernel[i].get();
            // SAFETY: previous I/O and bridge callback completed. Preserve the wire
            // frame while resetting only OVERLAPPED for a partial transfer.
            let event = unsafe { (*k).overlapped.hEvent };
            // SAFETY: quiescent OVERLAPPED can be reset without touching the payload.
            unsafe {
                ptr::write(ptr::addr_of_mut!((*k).overlapped), std::mem::zeroed());
                (*k).overlapped.hEvent = event;
            }
            if self.get(p.request.handle)?.transport.routed
                && let Some(bridge) = &mut self.bridges[i]
            {
                bridge.prepare(raw)?;
            }
            k
        };
        let mut bytes = 0;
        // SAFETY: frame stays in pinned kernel storage until completion, and offset
        // bounds restrict every partial read/write to the remaining frame bytes.
        let ok = unsafe {
            let buffer = ptr::addr_of_mut!((*k).wire).cast::<u8>().add(p.offset);
            if send {
                WriteFile(
                    raw,
                    buffer,
                    (LENGTH - p.offset) as u32,
                    &mut bytes,
                    k.cast(),
                )
            } else {
                ReadFile(
                    raw,
                    buffer,
                    (LENGTH - p.offset) as u32,
                    &mut bytes,
                    k.cast(),
                )
            }
        } != 0;
        self.submitted(i, p, ok, bytes, os_error(), Stage::Io)?;
        Ok(None)
    }
}
