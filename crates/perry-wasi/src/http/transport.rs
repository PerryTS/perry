//! Owned HTTP/1 exchanges on the component context's existing Turnloop driver.
//! A request starts immediately, so guest upload backpressure cannot depend on
//! polling its response. Only owned bytes and immutable grants enter the driver.
pub(crate) mod reader;
#[cfg(test)]
mod tests;
mod wire;

use super::{
    body::{BodyReceiver, IncomingBody},
    Error, FieldMap,
};
use futures_channel::oneshot;
use futures_util::future::{select, Either};
use perry_component_io::{Request, Submitter};
use std::{
    future::Future,
    net::{IpAddr, SocketAddr},
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use turnloop::{backend::Platform, ExecutorHandle, Instant};
use turnloop_tls::{
    asynchronous::{TlsStream, Transport},
    ClientConfig, ClientOptions,
};

/// Exact address grants are checked *after* resolution, immediately before each
/// connect. DNS permission alone never grants a resolved destination. This data
/// contains no embedder callback and cannot execute JavaScript on the I/O thread.
#[derive(Clone, Default)]
pub struct DestinationPolicy {
    pub tcp: bool,
    pub dns: bool,
    pub addresses: Vec<(IpAddr, Option<u16>)>,
}
impl DestinationPolicy {
    fn permits(&self, addr: SocketAddr) -> bool {
        self.tcp
            && self
                .addresses
                .iter()
                .any(|(ip, port)| *ip == addr.ip() && port.is_none_or(|port| port == addr.port()))
    }
}
#[derive(Clone)]
pub struct Options {
    pub connect_timeout: Duration,
    pub first_byte_timeout: Duration,
    pub between_bytes_timeout: Duration,
    /// Explicit additional trust anchors; no ambient environment/file lookup.
    pub extra_ca_pem: Vec<u8>,
}
impl Default for Options {
    fn default() -> Self {
        Self {
            connect_timeout: Duration::from_secs(60),
            first_byte_timeout: Duration::from_secs(60),
            between_bytes_timeout: Duration::from_secs(60),
            extra_ca_pem: Vec::new(),
        }
    }
}
pub struct RequestSpec {
    pub method: String,
    pub uri: http::Uri,
    pub headers: FieldMap,
}
pub struct Response {
    pub status: u16,
    pub headers: FieldMap,
    pub body: IncomingBody,
}
/// Owns cancellation until a response transfers it to the incoming body. Neither
/// a response nor its trailer future can outlive the context's native driver.
pub struct PendingResponse {
    receive: oneshot::Receiver<Result<Response, Error>>,
    task: Option<std::sync::Arc<Request<()>>>,
    #[cfg(feature = "p3")]
    processing:
        futures_util::future::Shared<futures_util::future::BoxFuture<'static, Result<(), Error>>>,
}
impl PendingResponse {
    #[cfg(feature = "p3")]
    pub(crate) fn processing(&self) -> futures_util::future::BoxFuture<'static, Result<(), Error>> {
        let task = self.task.as_ref().unwrap().clone();
        let result = self.processing.clone();
        Box::pin(async move {
            let _task = task;
            result.await
        })
    }
    pub async fn receive(mut self) -> Result<Response, Error> {
        let mut response = (&mut self.receive)
            .await
            .map_err(|_| Error::ConnectionTerminated)??;
        response.body.retain_transport(self.task.take().unwrap());
        Ok(response)
    }
}
impl Drop for PendingResponse {
    fn drop(&mut self) {
        if let Some(task) = &self.task {
            task.cancel();
        }
    }
}

struct Endpoint {
    host: String,
    authority: String,
    port: u16,
    tls: bool,
    path: String,
}
impl Endpoint {
    fn new(spec: &RequestSpec) -> Result<Self, Error> {
        let method = http::Method::from_bytes(spec.method.as_bytes())
            .map_err(|_| Error::HttpRequestMethodInvalid)?;
        if method == http::Method::CONNECT {
            return Err(Error::HttpRequestMethodInvalid);
        }
        let tls = match spec.uri.scheme_str() {
            Some("https") => true,
            Some("http") => false,
            _ => return Err(Error::HttpRequestUriInvalid),
        };
        let authority = spec.uri.authority().ok_or(Error::HttpRequestUriInvalid)?;
        if authority.as_str().contains('@') || authority.host().is_empty() {
            return Err(Error::HttpRequestUriInvalid);
        }
        let port_suffix = &authority.as_str()[authority.host().len()..];
        let port = if port_suffix.is_empty() {
            if tls {
                443
            } else {
                80
            }
        } else {
            port_suffix
                .strip_prefix(':')
                .filter(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()))
                .and_then(|s| s.parse::<u16>().ok())
                .ok_or(Error::HttpRequestUriInvalid)?
        };
        let host = authority
            .host()
            .trim_start_matches('[')
            .trim_end_matches(']')
            .to_owned();
        let path = spec
            .uri
            .path_and_query()
            .map_or("/", |p| p.as_str())
            .to_owned();
        if !path.starts_with('/') || path.len() > 8192 {
            return Err(Error::HttpRequestUriInvalid);
        }
        Ok(Self {
            host,
            authority: authority.to_string(),
            port,
            tls,
            path,
        })
    }
}

pub fn send(
    submitter: &Submitter,
    policy: DestinationPolicy,
    options: Options,
    spec: RequestSpec,
    mut body: BodyReceiver,
) -> Result<PendingResponse, Error> {
    let endpoint = Endpoint::new(&spec)?;
    if !policy.tcp {
        return Err(Error::HttpRequestDenied);
    }
    let (send, receive) = oneshot::channel();
    #[cfg(feature = "p3")]
    let (completed, completion) = oneshot::channel();
    let task = submitter
        .submit(move |io| async move {
            let result = connect(&io, &endpoint, &policy, &options).await;
            let result = match result {
                Ok(stream) => {
                    wire::exchange(&io, stream, endpoint, spec, &mut body, options, send).await
                }
                Err(error) => {
                    body.fail(error.clone());
                    let _ = send.send(Err(error.clone()));
                    Err(error)
                }
            };
            #[cfg(feature = "p3")]
            let _ = completed.send(result);
            #[cfg(not(feature = "p3"))]
            let _ = result;
        })
        .map_err(|e| match e {
            perry_component_io::Error::Capacity => Error::ConnectionLimitReached,
            _ => Error::ConnectionTerminated,
        })?;
    Ok(PendingResponse {
        receive,
        task: Some(std::sync::Arc::new(task)),
        #[cfg(feature = "p3")]
        processing: {
            use futures_util::FutureExt;
            async move { completion.await.unwrap_or(Err(Error::ConnectionTerminated)) }
                .boxed()
                .shared()
        },
    })
}

pub(crate) fn after(io: &ExecutorHandle<Platform>, duration: Duration) -> Result<Instant, Error> {
    io.now()
        .checked_add(duration)
        .ok_or(Error::ConfigurationError)
}

pub(crate) async fn deadline<T>(
    io: &ExecutorHandle<Platform>,
    at: Instant,
    error: Error,
    future: impl Future<Output = Result<T, Error>>,
) -> Result<T, Error> {
    let future = std::pin::pin!(future);
    let timer = std::pin::pin!(io.sleep_until(at));
    match select(future, timer).await {
        Either::Left((value, _)) => value,
        Either::Right((_, _)) => Err(error),
    }
}

async fn connect(
    io: &ExecutorHandle<Platform>,
    endpoint: &Endpoint,
    policy: &DestinationPolicy,
    options: &Options,
) -> Result<Transport<turnloop::AsyncIo<Platform>>, Error> {
    let at = after(io, options.connect_timeout)?;
    let addresses = if let Ok(ip) = endpoint.host.parse::<IpAddr>() {
        vec![SocketAddr::new(ip, endpoint.port)]
    } else {
        if !policy.dns {
            return Err(Error::HttpRequestDenied);
        }
        deadline(io, at, Error::DnsTimeout, async {
            io.resolve(turnloop::DnsRequest {
                host: endpoint.host.clone(),
                port: endpoint.port,
            })
            .await
            .map_err(|_| Error::DnsError {
                rcode: None,
                info_code: None,
            })
        })
        .await?
    };
    let mut error = if addresses.is_empty() {
        Error::DestinationNotFound
    } else {
        Error::DestinationIpProhibited
    };
    for address in addresses {
        if !policy.permits(address) {
            continue;
        }
        match deadline(io, at, Error::ConnectionTimeout, async {
            io.connect(address, turnloop::TcpOpts::default())
                .await
                .map_err(|e| {
                    match e
                        .os
                        .map(std::io::Error::from_raw_os_error)
                        .map(|e| e.kind())
                    {
                        Some(std::io::ErrorKind::ConnectionRefused) => Error::ConnectionRefused,
                        Some(std::io::ErrorKind::TimedOut) => Error::ConnectionTimeout,
                        _ => Error::DestinationUnavailable,
                    }
                })
        })
        .await
        {
            Ok(stream) if !endpoint.tls => return Ok(Transport::Plain(stream)),
            Ok(stream) => {
                let now = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map_err(|_| Error::ConfigurationError)?
                    .as_secs();
                let tls = ClientConfig::new(
                    ClientOptions {
                        extra_ca_pem: options.extra_ca_pem.clone(),
                        ..ClientOptions::default()
                    },
                    now,
                )
                .map_err(|_| Error::ConfigurationError)?;
                let name =
                    turnloop_tls::rustls::pki_types::ServerName::try_from(endpoint.host.clone())
                        .map_err(|_| Error::HttpRequestUriInvalid)?;
                let tls = TlsStream::connect(stream, &tls, name, io, at, now)
                    .await
                    .map_err(|e| {
                        if e.kind() == std::io::ErrorKind::TimedOut {
                            Error::ConnectionTimeout
                        } else if e
                            .get_ref()
                            .and_then(|e| e.downcast_ref::<turnloop_tls::rustls::Error>())
                            .is_some_and(|e| {
                                matches!(e, turnloop_tls::rustls::Error::InvalidCertificate(_))
                            })
                        {
                            Error::TlsCertificateError
                        } else {
                            Error::TlsProtocolError
                        }
                    })?;
                if tls.alpn_protocol().is_some_and(|p| p != b"http/1.1") {
                    return Err(Error::TlsProtocolError);
                }
                return Ok(Transport::Tls(Box::new(tls)));
            }
            Err(e) => error = e,
        }
    }
    Err(error)
}
