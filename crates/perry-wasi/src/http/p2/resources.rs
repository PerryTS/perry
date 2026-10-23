use super::{
    bindings::http::types::{Method, Scheme},
    *,
};
use crate::{
    http::{
        body::{BodyReceiver, FutureTrailers, IncomingBody},
        Error,
    },
    io::poll::Pollable,
};
use futures_channel::oneshot;
use std::{
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};
use wasmtime::component::Resource;

pub struct HostOutgoingRequest {
    pub method: Method,
    pub path_with_query: Option<String>,
    pub scheme: Option<Scheme>,
    pub authority: Option<String>,
    pub headers: FieldMap,
    pub body: Option<BodyReceiver>,
}
pub struct HostIncomingRequest {
    pub method: http::Method,
    pub uri: http::Uri,
    pub scheme: Scheme,
    pub authority: String,
    pub headers: FieldMap,
    pub body: Option<IncomingBody>,
}
pub struct HostOutgoingResponse {
    pub status: http::StatusCode,
    pub headers: FieldMap,
    pub body: Option<BodyReceiver>,
}
pub struct HostIncomingResponse {
    pub status: u16,
    pub headers: FieldMap,
    pub body: Option<IncomingBody>,
}
/// Observes whether the guest called response-outparam.set, including set(err).
/// It does not promise that the peer has received the response.
#[derive(Clone, Default)]
pub struct ResponseStatus(std::sync::Arc<std::sync::atomic::AtomicBool>);
impl ResponseStatus {
    pub fn is_set(&self) -> bool {
        self.0.load(std::sync::atomic::Ordering::Acquire)
    }
    pub(crate) fn set(&self) {
        self.0.store(true, std::sync::atomic::Ordering::Release);
    }
}
pub struct HostResponseOutparam {
    pub(crate) status: ResponseStatus,
    pub(crate) send: oneshot::Sender<Result<HostOutgoingResponse, Error>>,
}
impl HostResponseOutparam {
    pub fn observer(&self) -> ResponseStatus {
        self.status.clone()
    }
}
impl WasiHttpCtxView<'_> {
    pub fn new_response_outparam(
        &mut self,
        send: oneshot::Sender<Result<HostOutgoingResponse, Error>>,
    ) -> wasmtime::Result<Resource<HostResponseOutparam>> {
        Ok(self.table.push(HostResponseOutparam {
            send,
            status: ResponseStatus::default(),
        })?)
    }
}
// A Send future carries only the native response channel + task cancellation
// handle; polling it and lifting its owned result happen on the calling thread.
type ResponseFuture = Pin<Box<dyn Future<Output = Result<transport::Response, Error>> + Send>>;
pub enum HostFutureIncomingResponse {
    Pending(ResponseFuture),
    Ready(Result<transport::Response, Error>),
    Consumed,
}
impl HostFutureIncomingResponse {
    pub(crate) fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<()> {
        if let Self::Pending(future) = self {
            *self = Self::Ready(std::task::ready!(future.as_mut().poll(cx)));
        }
        Poll::Ready(())
    }
    pub(crate) fn get(&mut self) -> Option<Result<Result<transport::Response, Error>, ()>> {
        let mut cx = Context::from_waker(futures_util::task::noop_waker_ref());
        if self.poll_ready(&mut cx).is_pending() {
            return None;
        }
        match std::mem::replace(self, Self::Consumed) {
            Self::Ready(value) => Some(Ok(value)),
            Self::Consumed => Some(Err(())),
            Self::Pending(_) => unreachable!(),
        }
    }
}
#[async_trait::async_trait]
impl Pollable for HostFutureIncomingResponse {
    async fn ready(&mut self) -> wasmtime::Result<()> {
        futures_lite::future::poll_fn(|cx| self.poll_ready(cx)).await;
        Ok(())
    }
}
#[async_trait::async_trait]
impl Pollable for FutureTrailers {
    async fn ready(&mut self) -> wasmtime::Result<()> {
        FutureTrailers::ready(self).await;
        Ok(())
    }
}
impl From<http::Method> for Method {
    fn from(method: http::Method) -> Self {
        match method.as_str() {
            "GET" => Self::Get,
            "HEAD" => Self::Head,
            "POST" => Self::Post,
            "PUT" => Self::Put,
            "DELETE" => Self::Delete,
            "CONNECT" => Self::Connect,
            "OPTIONS" => Self::Options,
            "TRACE" => Self::Trace,
            "PATCH" => Self::Patch,
            other => Self::Other(other.to_owned()),
        }
    }
}
impl TryFrom<Method> for http::Method {
    type Error = http::method::InvalidMethod;
    fn try_from(method: Method) -> Result<Self, Self::Error> {
        Ok(match method {
            Method::Get => Self::GET,
            Method::Head => Self::HEAD,
            Method::Post => Self::POST,
            Method::Put => Self::PUT,
            Method::Delete => Self::DELETE,
            Method::Connect => Self::CONNECT,
            Method::Options => Self::OPTIONS,
            Method::Trace => Self::TRACE,
            Method::Patch => Self::PATCH,
            Method::Other(other) => Self::from_bytes(other.as_bytes())?,
        })
    }
}
