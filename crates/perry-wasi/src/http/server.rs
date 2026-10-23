//! Embedding-owned HTTP/1.1 listener. Binding is explicit host authority and
//! does not change a guest's outbound WASI grants. One bounded native task owns
//! each accepted exchange, including its TLS session and both streaming halves.
#[cfg(test)]
mod tests;
mod wire;

use super::{p2::HostIncomingRequest, p2::HostOutgoingResponse, transport::after, Error};
use futures_channel::oneshot;
use perry_component_io::{Driver, Resource, ResourceRequest};
use std::{net::SocketAddr, time::Duration};
use turnloop::{backend::Platform, AsyncIo};
use turnloop_tls::{asynchronous::TlsStream, asynchronous::Transport, ServerConfig};

#[derive(Clone)]
pub struct Options {
    pub handshake_timeout: Duration,
    pub request_head_timeout: Duration,
    pub response_head_timeout: Duration,
    pub between_bytes_timeout: Duration,
    pub linger_timeout: Duration,
    /// Host-supplied credentials and ALPN; no ambient file/environment lookup.
    pub tls: Option<ServerConfig>,
}
impl Default for Options {
    fn default() -> Self {
        Self {
            handshake_timeout: Duration::from_secs(10),
            request_head_timeout: Duration::from_secs(30),
            response_head_timeout: Duration::from_secs(60),
            between_bytes_timeout: Duration::from_secs(60),
            linger_timeout: Duration::from_secs(1),
            tls: None,
        }
    }
}
/// Dropping the server cancels accepts/exchanges and joins its native driver.
/// Connection reuse, HTTP/2, upgrades and proxy CONNECT are not implemented.
pub struct Server {
    listener: Resource<AsyncIo<Platform>>,
    address: SocketAddr,
    options: Options,
    _driver: Driver,
}
/// Owns a pending accept and exchange. Dropping it cancels both.
pub struct PendingRequest {
    receive: oneshot::Receiver<Result<Request, Error>>,
    task: ResourceRequest<Result<(), Error>>,
}
pub struct Request {
    pub request: HostIncomingRequest,
    pub response: oneshot::Sender<Result<HostOutgoingResponse, Error>>,
}
/// Keep this owner while the guest handles the request. Dropping a guest input
/// body does not cancel the response; dropping this owner cancels the exchange.
/// `finish` reports transport completion, not proof that the peer consumed it.
pub struct Completion(ResourceRequest<Result<(), Error>>);
impl Completion {
    pub async fn finish(self) -> Result<(), Error> {
        self.0.await.map_err(driver_error)?
    }
    pub async fn cancel(self) {
        self.0.cancel();
        let _ = self.0.await;
    }
}
impl PendingRequest {
    pub async fn receive(self) -> Result<(Request, Completion), Error> {
        match self.receive.await {
            Ok(Ok(request)) => Ok((request, Completion(self.task))),
            received => {
                // Keep the task alive to flush its rejection and finish native
                // teardown; release admission before returning a failed accept.
                let completed = self.task.await;
                match received {
                    Ok(Err(error)) => Err(error),
                    _ => match completed {
                        Ok(Err(error)) => Err(error),
                        _ => Err(Error::ConnectionTerminated),
                    },
                }
            }
        }
    }

    pub async fn cancel(self) {
        self.task.cancel();
        let _ = self.task.await;
    }
}
fn driver_error(error: perry_component_io::Error) -> Error {
    match error {
        perry_component_io::Error::Capacity => Error::ConnectionLimitReached,
        _ => Error::ConnectionTerminated,
    }
}
impl Server {
    pub async fn bind(address: SocketAddr, options: Options, limit: usize) -> Result<Self, Error> {
        let driver = Driver::new(limit).map_err(driver_error)?;
        let native_options = options.clone();
        let (send, receive) = oneshot::channel();
        let listener = driver
            .submitter()
            .register(move |io| async move {
                // Reject unrepresentable deadlines before publishing a listener.
                for duration in [
                    native_options.handshake_timeout,
                    native_options.request_head_timeout,
                    native_options.response_head_timeout,
                    native_options.between_bytes_timeout,
                    native_options.linger_timeout,
                ] {
                    after(&io, duration)?;
                }
                if native_options.tls.as_ref().is_some_and(|tls| {
                    tls.rustls_config()
                        .alpn_protocols
                        .iter()
                        .any(|p| p != b"http/1.1")
                }) {
                    return Err(Error::ConfigurationError);
                }
                let handle = io
                    .driver()
                    .tcp_listen(address, &turnloop::ListenOpts::default())
                    .map_err(|_| Error::DestinationUnavailable)?;
                let stream = io.io(handle);
                let address = io
                    .driver()
                    .local_addr(handle)
                    .map_err(|_| Error::DestinationUnavailable)?;
                let _ = send.send(address);
                Ok(stream)
            })
            .map_err(driver_error)?
            .await
            .map_err(driver_error)??;
        let address = receive.await.map_err(|_| Error::ConnectionTerminated)?;
        Ok(Self {
            listener,
            address,
            options,
            _driver: driver,
        })
    }
    pub fn local_addr(&self) -> SocketAddr {
        self.address
    }
    pub fn accept(&self) -> Result<PendingRequest, Error> {
        let options = self.options.clone();
        let (send, receive) = oneshot::channel();
        let task = self
            .listener
            .call(move |io, listener| async move {
                let handle = listener.borrow_mut().handle();
                let accepted = io.accept(handle).await;
                drop(listener);
                let stream = match accepted {
                    Ok(stream) => stream,
                    Err(_) => {
                        let _ = send.send(Err(Error::ConnectionTerminated));
                        return Err(Error::ConnectionTerminated);
                    }
                };
                let connected: Result<_, Error> = async {
                    if let Some(tls) = &options.tls {
                        let now = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .map_err(|_| Error::ConfigurationError)?
                            .as_secs();
                        let stream = TlsStream::accept(
                            stream,
                            tls,
                            &io,
                            after(&io, options.handshake_timeout)?,
                            now,
                        )
                        .await
                        .map_err(|e| {
                            if e.kind() == std::io::ErrorKind::TimedOut {
                                Error::ConnectionTimeout
                            } else {
                                Error::TlsProtocolError
                            }
                        })?;
                        Ok(Transport::Tls(Box::new(stream)))
                    } else {
                        Ok(Transport::Plain(stream))
                    }
                }
                .await;
                match connected {
                    Ok(stream) => wire::exchange(&io, stream, &options, send).await,
                    Err(error) => {
                        let _ = send.send(Err(error.clone()));
                        Err(error)
                    }
                }
            })
            .map_err(driver_error)?;
        Ok(PendingRequest { receive, task })
    }
}
