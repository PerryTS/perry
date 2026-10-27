//! DNS never retains a Store borrow while the native resolver is pending.
use crate::p3::bindings::sockets::{
    ip_name_lookup::{self, ErrorCode},
    types::IpAddress,
};
use crate::sockets::{WasiSockets, WasiSocketsCtxView};
use wasmtime::component::Accessor;
impl ip_name_lookup::Host for WasiSocketsCtxView<'_> {}
impl<T> ip_name_lookup::HostWithStore<T> for WasiSockets {
    async fn resolve_addresses(
        store: &Accessor<T, Self>,
        name: String,
    ) -> wasmtime::Result<Result<Vec<IpAddress>, ErrorCode>> {
        let (allowed, submitter) = store.with(|mut s| {
            let ctx = s.get().ctx;
            (ctx.allow_ip_name_lookup, ctx.submitter.clone())
        });
        if !allowed {
            return Ok(Err(ErrorCode::AccessDenied));
        }
        let parsed = match url::Host::parse(&name) {
            Ok(host) => host,
            Err(_) => match name.parse::<std::net::Ipv6Addr>() {
                Ok(address) => url::Host::Ipv6(address),
                Err(_) => return Ok(Err(ErrorCode::InvalidArgument)),
            },
        };
        let mut addresses = match parsed {
            url::Host::Ipv4(address) => vec![std::net::IpAddr::V4(address)],
            url::Host::Ipv6(address) => vec![std::net::IpAddr::V6(address).to_canonical()],
            url::Host::Domain(host) => {
                let request = submitter.submit(move |io| async move {
                    io.resolve(turnloop::DnsRequest { host, port: 0 }).await
                })?;
                match request.await? {
                    Ok(addresses) => addresses
                        .into_iter()
                        .map(|a| a.ip().to_canonical())
                        .collect(),
                    Err(error)
                        if matches!(
                            error.kind,
                            turnloop::ErrorKind::Cancelled | turnloop::ErrorKind::ResourceLimit
                        ) =>
                    {
                        return Err(error.into())
                    }
                    Err(_) => return Ok(Err(ErrorCode::NameUnresolvable)),
                }
            }
        };
        if addresses.is_empty() {
            return Ok(Err(ErrorCode::NameUnresolvable));
        }
        // Preserve resolver preference while removing duplicate addresses.
        let mut unique = std::collections::HashSet::new();
        addresses.retain(|address| unique.insert(*address));
        Ok(Ok(addresses.into_iter().map(Into::into).collect()))
    }
}
