//! Bounded HTTP/1 message decoder shared by the client and server.
use super::{after, deadline, Error, FieldMap};
use crate::http::body::{IncomingBodySender, StreamContext, CHUNK_SIZE};
use bytes::Bytes;
use futures_util::io::{AsyncRead, AsyncReadExt};
use std::time::Duration;
use turnloop::{backend::Platform, ExecutorHandle, Instant};
use turnloop_http::http1::{Decoder, Event, Head, Header, Limits, Mode};

pub(crate) fn fields(headers: Vec<Header>, context: StreamContext) -> Result<FieldMap, Error> {
    let entries = headers
        .into_iter()
        .map(|h| {
            let value =
                http::HeaderValue::from_bytes(&h.value).map_err(|_| Error::HttpProtocolError)?;
            Ok((h.name, value))
        })
        .collect::<Result<Vec<_>, Error>>()?;
    FieldMap::from_received(entries).map_err(|error| match error {
        crate::http::FieldMapError::TooManyFields | crate::http::FieldMapError::TotalSizeTooBig => {
            match context {
                StreamContext::Request => Error::HttpRequestHeaderSectionSize(None),
                StreamContext::Response => Error::HttpResponseHeaderSectionSize(None),
            }
        }
        _ => Error::HttpProtocolError,
    })
}

enum OwnedEvent {
    Head(Head),
    Information,
    Data(Bytes),
    Trailers(Vec<Header>),
    End,
}
pub(crate) struct ReadMessage<R> {
    reader: R,
    decoder: Decoder,
    buffer: Vec<u8>,
    eof: bool,
    context: StreamContext,
}
impl<R: AsyncRead + Unpin> ReadMessage<R> {
    pub(crate) fn response(reader: R, method: &str) -> Self {
        let mut decoder = Decoder::new(Mode::Response, Limits::default());
        decoder.response_to(method);
        Self {
            reader,
            decoder,
            buffer: Vec::with_capacity(CHUNK_SIZE),
            eof: false,
            context: StreamContext::Response,
        }
    }
    pub(crate) fn request(reader: R) -> Self {
        Self {
            reader,
            decoder: Decoder::new(Mode::Request, Limits::default()),
            buffer: Vec::with_capacity(CHUNK_SIZE),
            eof: false,
            context: StreamContext::Request,
        }
    }
    fn incomplete(&self) -> Error {
        match self.context {
            StreamContext::Request => Error::HttpProtocolError,
            StreamContext::Response => Error::HttpResponseIncomplete,
        }
    }
    async fn next(
        &mut self,
        io: &ExecutorHandle<Platform>,
        at: Option<Instant>,
        between: Duration,
    ) -> Result<OwnedEvent, Error> {
        loop {
            let step = self
                .decoder
                .receive(&self.buffer)
                .map_err(|_| Error::HttpProtocolError)?;
            let consumed = step.consumed;
            let event = match step.event {
                Some(Event::Head(h)) => Some(OwnedEvent::Head(h)),
                Some(Event::Informational(_)) => Some(OwnedEvent::Information),
                Some(Event::Body(bytes)) => Some(OwnedEvent::Data(Bytes::copy_from_slice(bytes))),
                Some(Event::Trailers(t)) => Some(OwnedEvent::Trailers(t)),
                Some(Event::End) => Some(OwnedEvent::End),
                Some(Event::Upgrade) => return Err(Error::HttpUpgradeFailed),
                None => None,
            };
            self.buffer.drain(..consumed);
            if let Some(event) = event {
                return Ok(event);
            }
            if consumed != 0 {
                continue;
            }
            if self.eof {
                return Err(self.incomplete());
            }
            if self.buffer.len() >= CHUNK_SIZE {
                return Err(Error::HttpProtocolError);
            }
            let mut bytes = [0; 16384];
            let limit = bytes.len().min(CHUNK_SIZE - self.buffer.len());
            let at = match at {
                Some(at) => at,
                None => after(io, between)?,
            };
            let n = deadline(io, at, Error::ConnectionReadTimeout, async {
                self.reader
                    .read(&mut bytes[..limit])
                    .await
                    .map_err(|_| Error::ConnectionTerminated)
            })
            .await?;
            if n == 0 {
                self.eof = true;
                self.decoder.eof().map_err(|_| self.incomplete())?;
            } else {
                self.buffer.extend_from_slice(&bytes[..n]);
            }
        }
    }
    pub(crate) async fn head(
        &mut self,
        io: &ExecutorHandle<Platform>,
        at: Instant,
    ) -> Result<Head, Error> {
        // Bound informational responses as well as the total wait for a head.
        for _ in 0..128 {
            match self.next(io, Some(at), Duration::ZERO).await? {
                OwnedEvent::Head(head) => return Ok(head),
                OwnedEvent::Information => {}
                _ => return Err(Error::HttpProtocolError),
            }
        }
        Err(Error::HttpResponseHeaderSectionSize(None))
    }
    pub(crate) async fn body(
        &mut self,
        io: &ExecutorHandle<Platform>,
        between: Duration,
        output: &mut IncomingBodySender,
        discard_closed: bool,
    ) -> Result<Option<FieldMap>, Error> {
        let mut trailers = None;
        loop {
            // The between-byte clock measures the next network read, excluding
            // time spent waiting for a guest to consume a full bounded queue.
            match self.next(io, None, between).await? {
                OwnedEvent::Data(bytes) => {
                    if let Err(error) = output.push(bytes).await {
                        if !discard_closed || !output.consumer_closed() {
                            return Err(error);
                        }
                    }
                }
                OwnedEvent::Trailers(fields_) => trailers = Some(fields(fields_, self.context)?),
                OwnedEvent::End => return Ok(trailers),
                _ => return Err(Error::HttpProtocolError),
            }
        }
    }
}
