// Adapted from Wasmtime 48.0.5; Apache-2.0 WITH LLVM-exception. See PROVENANCE.md.
use crate::{
    io::poll::{subscribe, DynPollable, Pollable},
    p2::{
        bindings::sockets::{
            ip_name_lookup,
            network::{ErrorCode, IpAddress},
        },
        network::Network,
        SocketError, SocketResult,
    },
    sockets::WasiSocketsCtxView,
};
use perry_component_io::Request;
use std::{
    future::Future,
    net::IpAddr,
    pin::Pin,
    task::{Context, Poll},
};
use wasmtime::component::Resource;

enum Resolution {
    Pending(Request<Result<Vec<IpAddr>, turnloop::Error>>),
    Ready(Result<std::vec::IntoIter<IpAddr>, ErrorCode>),
    Failed(String),
}

pub struct ResolveAddressStream(Resolution);
impl ResolveAddressStream {
    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<wasmtime::Result<()>> {
        if let Resolution::Pending(request) = &mut self.0 {
            self.0 = match std::task::ready!(Pin::new(request).poll(cx)) {
                Ok(Ok(addrs)) => Resolution::Ready(Ok(addrs.into_iter())),
                Ok(Err(error))
                    if matches!(
                        error.kind,
                        turnloop::ErrorKind::Cancelled | turnloop::ErrorKind::ResourceLimit
                    ) =>
                {
                    Resolution::Failed(error.to_string())
                }
                // Preserve the upstream resolver contract: native name lookup
                // failures map to name-unresolvable, separate from driver loss.
                Ok(Err(_)) => Resolution::Ready(Err(ErrorCode::NameUnresolvable)),
                Err(error) => Resolution::Failed(error.to_string()),
            };
        }
        Poll::Ready(match &self.0 {
            Resolution::Failed(error) => {
                Err(wasmtime::format_err!("WASI DNS driver failed: {error}"))
            }
            Resolution::Ready(_) => Ok(()),
            Resolution::Pending(_) => unreachable!("completed DNS request is still pending"),
        })
    }
}

impl ip_name_lookup::Host for WasiSocketsCtxView<'_> {
    fn resolve_addresses(
        &mut self,
        network: Resource<Network>,
        name: String,
    ) -> SocketResult<Resource<ResolveAddressStream>> {
        self.table.get(&network)?;
        if !self.ctx.allow_ip_name_lookup {
            return Err(ErrorCode::PermanentResolverFailure.into());
        }
        let parsed = parse_name(&name)?;
        let state = match parsed {
            url::Host::Ipv4(addr) => Resolution::Ready(Ok(vec![IpAddr::V4(addr)].into_iter())),
            url::Host::Ipv6(addr) => {
                Resolution::Ready(Ok(vec![IpAddr::V6(addr).to_canonical()].into_iter()))
            }
            url::Host::Domain(host) => Resolution::Pending(
                self.ctx
                    .submitter
                    .submit(move |io| async move {
                        io.resolve(turnloop::DnsRequest { host, port: 0 })
                            .await
                            .map(|addrs| addrs.into_iter().map(|a| a.ip().to_canonical()).collect())
                    })
                    .map_err(SocketError::trap)?,
            ),
        };
        Ok(self.table.push(ResolveAddressStream(state))?)
    }
}

impl ip_name_lookup::HostResolveAddressStream for WasiSocketsCtxView<'_> {
    fn resolve_next_address(
        &mut self,
        resource: Resource<ResolveAddressStream>,
    ) -> SocketResult<Option<IpAddress>> {
        let stream = self.table.get_mut(&resource)?;
        let mut cx = Context::from_waker(futures_util::task::noop_waker_ref());
        match stream.poll_ready(&mut cx) {
            Poll::Pending => Err(ErrorCode::WouldBlock.into()),
            Poll::Ready(Err(error)) => Err(SocketError::trap(error)),
            Poll::Ready(Ok(())) => match &mut stream.0 {
                Resolution::Ready(Ok(addrs)) => Ok(addrs.next().map(Into::into)),
                Resolution::Ready(Err(error)) => Err((*error).into()),
                _ => unreachable!("DNS readiness reported without a result"),
            },
        }
    }

    fn subscribe(
        &mut self,
        resource: Resource<ResolveAddressStream>,
    ) -> wasmtime::Result<Resource<DynPollable>> {
        subscribe(self.table, resource)
    }

    fn drop(&mut self, resource: Resource<ResolveAddressStream>) -> wasmtime::Result<()> {
        self.table.delete(resource)?;
        Ok(())
    }
}

#[async_trait::async_trait]
impl Pollable for ResolveAddressStream {
    async fn ready(&mut self) -> wasmtime::Result<()> {
        std::future::poll_fn(|cx| self.poll_ready(cx)).await
    }
}

fn parse_name(name: &str) -> SocketResult<url::Host> {
    // Match upstream validation and Unicode -> IDNA conversion, including
    // bare IPv6 literals which URL Host parsing requires brackets around.
    match url::Host::parse(name) {
        Ok(host) => Ok(host),
        Err(_) => match name.parse::<std::net::Ipv6Addr>() {
            Ok(addr) => Ok(url::Host::Ipv6(addr)),
            Err(_) => Err(ErrorCode::InvalidArgument.into()),
        },
    }
}

#[cfg(test)]
mod tests;
