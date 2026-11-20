use super::*;
use crate::http::body::{BodyFrame, CHUNK_SIZE};
use futures_channel::mpsc;
use futures_util::{
    future::{select, Either, Shared},
    FutureExt, Stream, StreamExt,
};
use std::{
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};
use wasmtime::{
    component::{Source, StreamConsumer, StreamResult},
    StoreContextMut,
};
type BodyError = Shared<BoxFuture<'static, Option<Error>>>;
struct Output {
    send: mpsc::Sender<bytes::Bytes>,
    error: Option<oneshot::Sender<Option<Error>>>,
    expected: Option<u64>,
    written: u64,
    size_error: fn(Option<u64>) -> ErrorCode,
    closed: bool,
}
impl Output {
    fn fail(&mut self, error: Error) {
        self.closed = true;
        if let Some(send) = self.error.take() {
            let _ = send.send(Some(error));
        }
        self.send.close_channel();
    }
}
impl Drop for Output {
    fn drop(&mut self) {
        if !self.closed && self.expected.is_some_and(|n| n != self.written) {
            self.fail((self.size_error)(Some(self.written)).into());
        }
        if let Some(send) = self.error.take() {
            let _ = send.send(None);
        }
    }
}
impl<T> StreamConsumer<T> for Output {
    type Item = u8;
    fn poll_consume(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        store: StoreContextMut<T>,
        source: Source<u8>,
        finish: bool,
    ) -> Poll<wasmtime::Result<StreamResult>> {
        match self.send.poll_ready(cx) {
            Poll::Pending if finish => return Poll::Ready(Ok(StreamResult::Cancelled)),
            Poll::Pending => return Poll::Pending,
            Poll::Ready(Err(_)) => {
                self.fail(Error::ConnectionTerminated);
                return Poll::Ready(Ok(StreamResult::Dropped));
            }
            Poll::Ready(Ok(())) => (),
        }
        let mut source = source.as_direct(store);
        let count = source.remaining().len().min(CHUNK_SIZE);
        let next = self.written.checked_add(count as u64);
        if next.is_none()
            || self
                .expected
                .zip(next)
                .is_some_and(|(expected, next)| next > expected)
        {
            let error = (self.size_error)(next).into();
            self.fail(error);
            return Poll::Ready(Ok(StreamResult::Dropped));
        }
        if count == 0 {
            return Poll::Ready(Ok(StreamResult::Completed));
        }
        let bytes = bytes::Bytes::copy_from_slice(&source.remaining()[..count]);
        if self.send.start_send(bytes).is_err() {
            self.fail(Error::ConnectionTerminated);
            return Poll::Ready(Ok(StreamResult::Dropped));
        }
        self.written = next.unwrap();
        source.mark_read(count);
        Poll::Ready(Ok(StreamResult::Completed))
    }
}
struct Frames {
    contents: Option<mpsc::Receiver<bytes::Bytes>>,
    trailers: Option<oneshot::Receiver<Result<Option<FieldMap>, Error>>>,
    ready_trailers: Option<Option<FieldMap>>,
    error: Option<BodyError>,
    ended: bool,
}
impl Stream for Frames {
    type Item = Result<BodyFrame, Error>;
    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        if self.ended {
            return Poll::Ready(None);
        }
        if let Some(error) = &mut self.error {
            if let Poll::Ready(error) = Pin::new(error).poll(cx) {
                self.error = None;
                if let Some(error) = error {
                    self.ended = true;
                    return Poll::Ready(Some(Err(error)));
                }
            }
        }
        // An error cancels transmission even when the contents stream is
        // indefinitely pending. Successful trailers wait for body EOF.
        if let Some(trailers) = &mut self.trailers {
            if let Poll::Ready(trailers) = Pin::new(trailers).poll(cx) {
                self.trailers = None;
                match trailers.unwrap_or(Err(Error::HttpProtocolError)) {
                    Ok(fields) => self.ready_trailers = Some(fields),
                    Err(error) => {
                        self.ended = true;
                        self.contents = None;
                        return Poll::Ready(Some(Err(error)));
                    }
                }
            }
        }
        if let Some(contents) = &mut self.contents {
            match Pin::new(contents).poll_next(cx) {
                Poll::Ready(Some(bytes)) => return Poll::Ready(Some(Ok(BodyFrame::Data(bytes)))),
                Poll::Pending => return Poll::Pending,
                Poll::Ready(None) => self.contents = None,
            }
        }
        let Some(trailers) = self.ready_trailers.take() else {
            return Poll::Pending;
        };
        self.ended = true;
        Poll::Ready(trailers.map(|fields| Ok(BodyFrame::Trailers(fields))))
    }
}
pub(super) fn convert<T: 'static>(
    store: &mut Access<'_, T, WasiHttp>,
    contents: Option<StreamReader<u8>>,
    trailers: FutureReader<TrailerResult>,
    result: Processing,
    processing: BoxFuture<'static, Result<(), Error>>,
    expected: Option<u64>,
    size_error: fn(Option<u64>) -> ErrorCode,
) -> wasmtime::Result<BodyReceiver> {
    let (trailers_tx, trailers_rx) = oneshot::channel();
    let getter = store.getter();
    helpers::pipe(&mut *store, trailers, move |data, trailers| {
        let trailers = match trailers {
            Ok(Some(resource)) => {
                let mut fields = getter(data).table.delete(resource)?;
                fields.set_immutable();
                Ok(Some(fields))
            }
            Ok(None) => Ok(None),
            Err(error) => Err(error.into()),
        };
        let _ = trailers_tx.send(trailers);
        Ok(())
    })?;
    let (error_tx, error_rx) = oneshot::channel();
    let error = async move { error_rx.await.unwrap_or(Some(Error::HttpProtocolError)) }
        .boxed()
        .shared();
    let contents = if let Some(contents) = contents {
        // A zero-capacity mpsc channel gives its sole sender one reserved slot:
        // at most one 64 KiB chunk can wait for native transport consumption.
        let (send, receive) = mpsc::channel(0);
        contents.pipe(
            &mut *store,
            Output {
                send,
                error: Some(error_tx),
                expected,
                written: 0,
                size_error,
                closed: false,
            },
        )?;
        Some(receive)
    } else {
        let failure = expected
            .filter(|&n| n != 0)
            .map(|_| size_error(Some(0)).into());
        let _ = error_tx.send(failure);
        None
    };
    let completion_error = error.clone();
    let _ = result.send(Box::pin(async move {
        match select(processing, completion_error).await {
            Either::Left((result, _)) => result,
            Either::Right((Some(error), _)) => Err(error),
            Either::Right((None, processing)) => processing.await,
        }
    }));
    Ok(BodyReceiver::from_frames(
        Frames {
            contents,
            trailers: Some(trailers_rx),
            ready_trailers: None,
            error: Some(error),
            ended: false,
        }
        .boxed(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trailer_error_interrupts_pending_body_and_success_waits_for_eof() {
        let (send, receive) = mpsc::channel(0);
        let (tx, rx) = oneshot::channel();
        let mut frames = Frames {
            contents: Some(receive),
            trailers: Some(rx),
            ready_trailers: None,
            error: None,
            ended: false,
        };
        let mut cx = Context::from_waker(futures_util::task::noop_waker_ref());
        assert!(Pin::new(&mut frames).poll_next(&mut cx).is_pending());
        tx.send(Err(Error::LoopDetected)).unwrap();
        assert!(matches!(
            Pin::new(&mut frames).poll_next(&mut cx),
            Poll::Ready(Some(Err(Error::LoopDetected)))
        ));
        assert!(send.is_closed());
        assert!(matches!(
            Pin::new(&mut frames).poll_next(&mut cx),
            Poll::Ready(None)
        ));

        let (mut send, receive) = mpsc::channel(0);
        let (tx, rx) = oneshot::channel();
        let mut fields = FieldMap::new_mutable(1024);
        fields.append("X-End".into(), b"done".to_vec()).unwrap();
        tx.send(Ok(Some(fields))).unwrap();
        let mut frames = Frames {
            contents: Some(receive),
            trailers: Some(rx),
            ready_trailers: None,
            error: None,
            ended: false,
        };
        assert!(Pin::new(&mut frames).poll_next(&mut cx).is_pending());
        send.try_send(bytes::Bytes::from_static(b"body")).unwrap();
        assert!(
            matches!(Pin::new(&mut frames).poll_next(&mut cx), Poll::Ready(Some(Ok(BodyFrame::Data(bytes)))) if bytes == b"body"[..])
        );
        assert!(Pin::new(&mut frames).poll_next(&mut cx).is_pending());
        drop(send);
        assert!(
            matches!(Pin::new(&mut frames).poll_next(&mut cx), Poll::Ready(Some(Ok(BodyFrame::Trailers(fields)))) if fields["x-end"] == "done")
        );
        assert!(matches!(
            Pin::new(&mut frames).poll_next(&mut cx),
            Poll::Ready(None)
        ));
    }
}
