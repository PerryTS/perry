//! Datagram-preserving operations on executor-owned staging slots.
use super::*;

/// An owned UDP socket with independent receive and send operation slots.
///
/// Packet boundaries and source addresses are preserved, including empty
/// packets. Pending operations borrow no caller memory. The executor's
/// `buffer_size` bounds a packet: larger sends fail before submission; receives
/// may be truncated by the backend. Configure at least 65,535 bytes to receive
/// full ordinary UDP packets. Dropping this adapter cancels pending operations
/// and closes the socket, retaining native buffers until terminal acknowledgement.
pub struct AsyncDatagram<B: Backend> {
    io: AsyncIo<B>,
}
impl<B: Backend> ExecutorHandle<B> {
    /// Adopt an existing loop-owned UDP socket. Do not give the same handle to
    /// another adapter or close it through the driver while this owner is live.
    pub fn datagram(&self, handle: Handle) -> AsyncDatagram<B> {
        AsyncDatagram {
            io: self.io(handle),
        }
    }
}
impl<B: Backend> AsyncDatagram<B> {
    /// The owned handle, for driver address queries and socket controls.
    pub fn handle(&self) -> Handle {
        self.io.handle()
    }

    /// Receive one packet and its source address. Empty packets return length
    /// zero, never EOF. If `buf` is smaller than the completed packet, the tail
    /// is discarded, not returned as a second packet. An empty `buf` therefore
    /// consumes one packet. Pending polls retain only executor-owned storage;
    /// the next poll may supply a different buffer.
    pub fn poll_recv_from(
        &mut self,
        cx: &mut Context<'_>,
        buf: &mut [u8],
    ) -> Poll<io::Result<(usize, SocketAddr)>> {
        let this = &mut self.io;
        if this.closed {
            return Poll::Ready(Err(io::ErrorKind::NotConnected.into()));
        }
        let key = match this.read {
            Some(key) => key,
            None => {
                let key = this.shared.reserve(cx).map_err(io_error)?;
                let buffer = {
                    let mut slots = this.shared.slots.borrow_mut();
                    let bytes = &mut slots[key.index].bytes;
                    // SAFETY: fixed slot storage outlives native access, including
                    // abandoned requests awaiting terminal acknowledgement.
                    ReadBuf::Provided(unsafe {
                        IoBufMut::from_raw_parts(bytes.as_mut_ptr(), bytes.len())
                    })
                };
                let result = this
                    .shared
                    .driver
                    .borrow_mut()
                    .recv(this.handle, buffer, key.token());
                match result {
                    Ok(op) => {
                        this.shared.slots.borrow_mut()[key.index].op = Some(op);
                        this.read = Some(key);
                        key
                    }
                    Err(error) => {
                        this.shared.free(key);
                        return Poll::Ready(Err(io_error(error)));
                    }
                }
            }
        };
        let Some(result) = this.shared.result(key, cx) else {
            return Poll::Pending;
        };
        let result = match result {
            OpResult::RecvFrom { n, from, .. } => {
                let slots = this.shared.slots.borrow();
                let bytes = &slots[key.index].bytes;
                let take = buf.len().min(n);
                buf[..take].copy_from_slice(&bytes[..take]);
                Ok((take, from))
            }
            OpResult::Err(error) => Err(io_error(error)),
            OpResult::Cancelled => Err(io_error(Error::new(ErrorKind::Cancelled))),
            _ => Err(io::Error::other("unexpected datagram receive completion")),
        };
        this.shared.free(key);
        this.read = None;
        Poll::Ready(result)
    }

    /// Accept exactly one packet for sending, including an empty packet. A
    /// successful return means the adapter owns a copy; call `poll_flush` to
    /// observe native completion/errors. A pending return consumes no bytes or
    /// destination from this call: it waits for an earlier accepted packet.
    /// Packets larger than the executor buffer are rejected without splitting.
    pub fn poll_send_to(
        &mut self,
        cx: &mut Context<'_>,
        buf: &[u8],
        to: SocketAddr,
    ) -> Poll<io::Result<usize>> {
        if self.io.closed {
            return Poll::Ready(Err(io::ErrorKind::NotConnected.into()));
        }
        std::task::ready!(self.poll_flush(cx))?;
        let this = &mut self.io;
        let key = this.shared.reserve(cx).map_err(io_error)?;
        let buffer = {
            let mut slots = this.shared.slots.borrow_mut();
            let bytes = &mut slots[key.index].bytes;
            if buf.len() > bytes.len() {
                drop(slots);
                this.shared.free(key);
                return Poll::Ready(Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "datagram exceeds executor buffer",
                )));
            }
            bytes[..buf.len()].copy_from_slice(buf);
            // SAFETY: copied bytes remain immutable and owned by this slot until
            // native terminal completion, even if this adapter is dropped.
            WriteBuf::Provided(unsafe { IoBuf::from_raw_parts(bytes.as_ptr(), buf.len()) })
        };
        let result = this
            .shared
            .driver
            .borrow_mut()
            .send_to(this.handle, buffer, to, key.token());
        match result {
            Ok(op) => {
                this.shared.slots.borrow_mut()[key.index].op = Some(op);
                this.write = Some(key);
                Poll::Ready(Ok(buf.len()))
            }
            Err(error) => {
                this.shared.free(key);
                Poll::Ready(Err(io_error(error)))
            }
        }
    }

    /// Wait for the last accepted packet's native completion. This does not
    /// imply delivery to the remote application; UDP has no acknowledgement.
    pub fn poll_flush(&mut self, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.io).poll_flush(cx)
    }

    /// Cancel and discard this adapter's pending receive. It may already have
    /// consumed a packet. Native storage remains alive until cancellation is
    /// acknowledged; a subsequent receive uses a different slot. Sends survive.
    pub fn cancel_receive(&mut self) {
        if let Some(key) = self.io.read.take() {
            self.io.shared.abandon(key);
        }
    }

    /// Flush accepted sends, then close the socket and cancel pending receives.
    pub fn poll_close(&mut self, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        std::task::ready!(self.poll_flush(cx))?;
        self.cancel_receive();
        Pin::new(&mut self.io).poll_close(cx)
    }
}
