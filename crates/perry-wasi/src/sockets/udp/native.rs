use super::*;
use perry_component_io::LocalResource;
use std::{future::poll_fn, task::Poll};

struct Direction {
    local: LocalResource<NativeUdp>,
    write: bool,
}
impl Direction {
    fn acquire(local: LocalResource<NativeUdp>, write: bool) -> Result<Self, Failure> {
        {
            let mut native = local.borrow_mut();
            let active = if write {
                &mut native.writing
            } else {
                &mut native.reading
            };
            if *active {
                return Err(Failure::Socket(ErrorCode::ConcurrencyConflict));
            }
            *active = true;
        }
        Ok(Self { local, write })
    }
}
impl Drop for Direction {
    fn drop(&mut self) {
        let mut native = self.local.borrow_mut();
        if self.write {
            native.writing = false;
        } else {
            native.reading = false;
        }
    }
}
pub(super) fn receive(
    socket: &Resource<NativeUdp>,
    remote: Option<SocketAddr>,
) -> Result<ReceiveTask, perry_component_io::Error> {
    socket.call(move |_, local| async move {
        let direction = Direction::acquire(local, false)?;
        let mut bytes = vec![0; 65_535];
        let (n, from) = poll_fn(|cx| {
            // Restrict connected streams before exposing any datagram. Limit
            // filtering work per poll so other sockets/timers keep progressing.
            for _ in 0..16 {
                let (n, from) = std::task::ready!(direction
                    .local
                    .borrow_mut()
                    .io
                    .poll_recv_from(cx, &mut bytes))
                .map_err(Failure::from)?;
                if remote.is_none_or(|expected| expected == from) {
                    return Poll::Ready(Ok::<_, Failure>((n, from)));
                }
            }
            cx.waker().wake_by_ref();
            Poll::Pending
        })
        .await?;
        bytes.truncate(n);
        Ok(Datagram { data: bytes, from })
    })
}
pub(super) fn send(
    socket: &Resource<NativeUdp>,
    bytes: Vec<u8>,
    destination: SocketAddr,
) -> Result<SendTask, perry_component_io::Error> {
    socket.call(move |_, local| async move {
        let direction = Direction::acquire(local, true)?;
        let n = poll_fn(|cx| {
            direction
                .local
                .borrow_mut()
                .io
                .poll_send_to(cx, &bytes, destination)
        })
        .await
        .map_err(Failure::from)?;
        if n != bytes.len() {
            return Err(Failure::Socket(ErrorCode::Unknown));
        }
        poll_fn(|cx| direction.local.borrow_mut().io.poll_flush(cx))
            .await
            .map_err(Failure::from)
    })
}
