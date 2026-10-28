//! Guest bodies retain the same bounded framing and trailer ordering as native I/O.
use super::*;
use crate::http::body::{BodyFrame, CHUNK_SIZE};
use bytes::{Buf, Bytes};
use std::{
    pin::Pin,
    task::{Context, Poll},
};
use wasmtime::{
    component::{Destination, StreamProducer, StreamResult, VecBuffer},
    StoreContextMut,
};

type Next = (BodyReceiver, Result<Option<BodyFrame>, Error>);
struct Input {
    body: Option<BodyReceiver>,
    pending: Option<BoxFuture<'static, Next>>,
    bytes: Bytes,
    trailers: Option<FieldMap>,
    end: Option<oneshot::Sender<Result<Option<FieldMap>, Error>>>,
}
impl Drop for Input {
    fn drop(&mut self) {
        if let Some(end) = self.end.take() {
            let _ = end.send(Ok(None));
        }
    }
}
impl<T> StreamProducer<T> for Input {
    type Item = u8;
    type Buffer = VecBuffer<u8>;
    fn poll_produce<'a>(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        mut store: StoreContextMut<'a, T>,
        dst: Destination<'a, u8, VecBuffer<u8>>,
        finish: bool,
    ) -> Poll<wasmtime::Result<StreamResult>> {
        if self.end.is_none() {
            return Poll::Ready(Ok(StreamResult::Dropped));
        }
        if dst.remaining(&mut store) == Some(0) {
            return Poll::Ready(Ok(StreamResult::Completed));
        }
        while self.bytes.is_empty() {
            if self.pending.is_none() {
                let mut body = self.body.take().unwrap();
                self.pending = Some(Box::pin(async move {
                    let result = body.next().await;
                    (body, result)
                }));
            }
            let (body, frame) = match self.pending.as_mut().unwrap().as_mut().poll(cx) {
                Poll::Ready(result) => result,
                Poll::Pending if finish => return Poll::Ready(Ok(StreamResult::Cancelled)),
                Poll::Pending => return Poll::Pending,
            };
            self.pending = None;
            self.body = Some(body);
            let result = match frame {
                Ok(Some(BodyFrame::Data(bytes))) => {
                    self.bytes = bytes;
                    continue;
                }
                Ok(Some(BodyFrame::Trailers(fields))) => {
                    self.trailers = Some(fields);
                    continue;
                }
                Ok(None) => Ok(self.trailers.take()),
                Err(error) => Err(error),
            };
            self.body = None;
            let _ = self.end.take().unwrap().send(result);
            return Poll::Ready(Ok(StreamResult::Dropped));
        }
        let mut dst = dst.as_direct(store, CHUNK_SIZE);
        let count = self.bytes.len().min(dst.remaining().len());
        dst.remaining()[..count].copy_from_slice(&self.bytes[..count]);
        dst.mark_written(count);
        self.bytes.advance(count);
        Poll::Ready(Ok(StreamResult::Completed))
    }
}
pub(super) fn consume<T: 'static>(
    store: &mut Access<'_, T, WasiHttp>,
    body: BodyReceiver,
) -> wasmtime::Result<(StreamReader<u8>, FutureReader<TrailerResult>)> {
    let (end, receive) = oneshot::channel();
    let contents = StreamReader::new(
        &mut *store,
        Input {
            body: Some(body),
            pending: None,
            bytes: Bytes::new(),
            trailers: None,
            end: Some(end),
        },
    )?;
    let getter = store.getter();
    let trailers = helpers::future(
        &mut *store,
        async move {
            receive
                .await
                .map_err(|_| wasmtime::format_err!("HTTP body completion was lost"))
        },
        move |data, result| {
            Ok(match result {
                Ok(Some(fields)) => Ok(Some(getter(data).table.push(fields)?)),
                Ok(None) => Ok(None),
                Err(error) => Err(error.into()),
            })
        },
    )?;
    Ok((contents, trailers))
}
