//! Body ownership stays in the Store until canonical handles have been piped.
//! Only bounded owned byte queues and native completion futures cross to I/O.
mod bridge;
mod incoming;
mod outgoing;
use super::{
    bindings::http::types::{ErrorCode, Trailers},
    helpers,
};
use crate::http::{
    body::{BodyReceiver, IncomingBody},
    p2::{WasiHttp, WasiHttpCtxView},
    Error, FieldMap,
};
use futures_channel::oneshot;
use futures_util::future::BoxFuture;
use std::sync::Arc;
use wasmtime::{
    component::{Access, FutureReader, Resource, StreamReader},
    AsContextMut,
};

pub struct Request {
    pub method: http::Method,
    pub scheme: Option<http::uri::Scheme>,
    pub authority: Option<http::uri::Authority>,
    pub path_with_query: Option<http::uri::PathAndQuery>,
    pub headers: FieldMap,
    pub options: Option<Arc<crate::http::p2::RequestOptions>>,
    pub(crate) body: Body,
}
pub struct Response {
    pub status: http::StatusCode,
    pub headers: FieldMap,
    pub(crate) body: Body,
}
impl Request {
    /// Embedder authority supplies an incoming request; this grants no outbound access.
    pub fn from_native(
        request: crate::http::p2::HostIncomingRequest,
    ) -> (Self, BoxFuture<'static, Result<(), Error>>) {
        let body = request.body.unwrap_or_else(|| {
            let (body, sender) =
                IncomingBody::new(crate::http::body::StreamContext::Request, Some(0));
            sender.finish(None).expect("empty HTTP body");
            body
        });
        let (body, processing) = Body::native(body);
        let scheme = match request.scheme {
            crate::http::p2::bindings::http::types::Scheme::Http => Some(http::uri::Scheme::HTTP),
            crate::http::p2::bindings::http::types::Scheme::Https => Some(http::uri::Scheme::HTTPS),
            crate::http::p2::bindings::http::types::Scheme::Other(scheme) => scheme.parse().ok(),
        };
        (
            Self {
                method: request.method,
                scheme,
                authority: request
                    .uri
                    .authority()
                    .cloned()
                    .or_else(|| request.authority.parse().ok()),
                path_with_query: request.uri.path_and_query().cloned(),
                headers: request.headers,
                options: None,
                body,
            },
            processing,
        )
    }
}
impl Response {
    /// Convert guest streams to bounded native queues, keeping final transport
    /// completion separate from accepting bytes into those queues.
    pub fn into_native<T: 'static>(
        self,
        store: &mut Access<'_, T, WasiHttp>,
        completion: BoxFuture<'static, Result<(), Error>>,
    ) -> super::HttpResult<crate::http::p2::HostOutgoingResponse> {
        let expected = match crate::http::p2::get_content_length(&self.headers) {
            Ok(expected) => expected,
            Err(error) => {
                self.body
                    .fail(&mut *store, error.clone())
                    .map_err(super::HttpError::trap)?;
                return Err(error.into());
            }
        };
        let body = self
            .body
            .into_native(store, completion, expected, ErrorCode::HttpResponseBodySize)
            .map_err(super::HttpError::trap)?;
        Ok(crate::http::p2::HostOutgoingResponse {
            status: self.status,
            headers: self.headers,
            body: Some(body),
        })
    }
}
pub(super) type Outcome = Result<(), ErrorCode>;
pub(super) type TrailerResult = Result<Option<Resource<Trailers>>, ErrorCode>;
pub(super) type Processing = oneshot::Sender<BoxFuture<'static, Result<(), Error>>>;
pub(crate) enum Body {
    Guest {
        contents: Option<StreamReader<u8>>,
        trailers: FutureReader<TrailerResult>,
        result: Processing,
    },
    Native {
        body: IncomingBody,
        result: Processing,
    },
}
impl Body {
    pub fn new_guest<T: 'static>(
        store: &mut Access<'_, T, WasiHttp>,
        contents: Option<StreamReader<u8>>,
        trailers: FutureReader<TrailerResult>,
    ) -> wasmtime::Result<(Self, FutureReader<Outcome>)> {
        let (result, receive) = oneshot::channel::<BoxFuture<'static, Result<(), Error>>>();
        let future = FutureReader::new(&mut *store, async move {
            let outcome = match receive.await {
                Ok(future) => future.await,
                Err(_) => Ok(()),
            };
            wasmtime::error::Ok(outcome.map_err(Into::into))
        })?;
        Ok((
            Self::Guest {
                contents,
                trailers,
                result,
            },
            future,
        ))
    }
    pub fn native(body: IncomingBody) -> (Self, BoxFuture<'static, Result<(), Error>>) {
        let (result, receive) = oneshot::channel::<BoxFuture<'static, Result<(), Error>>>();
        (
            Self::Native { body, result },
            Box::pin(async move {
                match receive.await {
                    Ok(future) => future.await,
                    Err(_) => Ok(()),
                }
            }),
        )
    }
    pub fn consume<T: 'static>(
        self,
        mut store: Access<'_, T, WasiHttp>,
        processing: FutureReader<Outcome>,
        getter: fn(&mut T) -> WasiHttpCtxView<'_>,
    ) -> wasmtime::Result<(StreamReader<u8>, FutureReader<TrailerResult>)> {
        match self {
            Self::Guest {
                contents,
                trailers,
                result,
            } => {
                let (send, receive) = oneshot::channel();
                helpers::pipe(&mut store, processing, move |_, outcome| {
                    let _ = send.send(outcome.map_err(Error::from));
                    Ok(())
                })?;
                let processing = Box::pin(async move { receive.await.unwrap_or(Ok(())) });
                let body = outgoing::convert(
                    &mut store,
                    contents,
                    trailers,
                    result,
                    processing,
                    None,
                    ErrorCode::HttpRequestBodySize,
                )?;
                bridge::consume(&mut store, body)
            }
            Self::Native { body, result } => {
                let abort = body.abort_handle();
                helpers::pipe(&mut store, processing, move |_, outcome| {
                    let outcome = outcome.map_err(Error::from);
                    if let Err(error) = &outcome {
                        abort(error.clone());
                    }
                    let _ = result.send(Box::pin(async move { outcome }));
                    Ok(())
                })?;
                incoming::consume(&mut store, body, getter)
            }
        }
    }
    pub fn fail(self, store: impl AsContextMut, error: Error) -> wasmtime::Result<()> {
        match self {
            Self::Guest {
                contents,
                trailers,
                result,
            } => {
                let _ = result.send(Box::pin(async move { Err(error) }));
                Self::close_guest(store, contents, trailers)
            }
            Self::Native { body, result } => {
                body.abort_handle()(error.clone());
                let _ = result.send(Box::pin(async move { Err(error) }));
                Ok(())
            }
        }
    }
    fn close_guest(
        mut store: impl AsContextMut,
        contents: Option<StreamReader<u8>>,
        mut trailers: FutureReader<TrailerResult>,
    ) -> wasmtime::Result<()> {
        if let Some(mut contents) = contents {
            contents.close(&mut store)?;
        }
        trailers.close(store)
    }
    pub fn drop(self, mut store: impl AsContextMut) -> wasmtime::Result<()> {
        if let Self::Guest {
            contents,
            mut trailers,
            ..
        } = self
        {
            if let Some(mut contents) = contents {
                contents.close(&mut store)?;
            }
            trailers.close(store)?;
        }
        Ok(())
    }
    pub fn into_native<T: 'static>(
        self,
        store: &mut Access<'_, T, WasiHttp>,
        processing: BoxFuture<'static, Result<(), Error>>,
        expected: Option<u64>,
        size_error: fn(Option<u64>) -> ErrorCode,
    ) -> wasmtime::Result<BodyReceiver> {
        match self {
            Self::Guest {
                contents,
                trailers,
                result,
            } => outgoing::convert(
                store, contents, trailers, result, processing, expected, size_error,
            ),
            Self::Native { body, result } => {
                // A forwarded native body still crosses the same bounded canonical
                // bridge so trailers, length checks and processing results agree.
                let getter = store.getter();
                let (contents, trailers) = incoming::consume(store, body, getter)?;
                outgoing::convert(
                    store,
                    Some(contents),
                    trailers,
                    result,
                    processing,
                    expected,
                    size_error,
                )
            }
        }
    }
}
