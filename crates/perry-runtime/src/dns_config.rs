//! The resolv.conf subset consumed by Perry's DNS client. Search/options
//! directives are owned by their callers; this parser only selects servers.
use std::net::{IpAddr, SocketAddr};

pub(super) fn nameservers(data: &[u8]) -> Vec<SocketAddr> {
    let Ok(text) = std::str::from_utf8(data) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for line in text.lines() {
        let line = line.split(['#', ';']).next().unwrap_or("");
        let mut fields = line.split_whitespace();
        if fields.next() != Some("nameserver") {
            continue;
        }
        let Some(address) = fields.next() else {
            return Vec::new();
        };
        if fields.next().is_some() {
            return Vec::new();
        }
        // Preserve the existing ScopedIp -> IpAddr conversion: interface
        // scopes are discarded here, rather than reinterpreted as ports.
        let address = address.split_once('%').map_or(address, |(ip, _)| ip);
        let Ok(ip) = address.parse::<IpAddr>() else {
            return Vec::new();
        };
        out.push(SocketAddr::new(ip, 53));
    }
    out
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn hosts_comments_ipv6_and_unrelated_options() {
        let result = nameservers(b"# generated\nsearch example.com\noptions ndots:5\nnameserver 127.0.0.53 # local\nnameserver 2001:db8::1 ; backup\nnameserver fe80::1%eth0\n");
        assert_eq!(
            result,
            [
                "127.0.0.53:53".parse().unwrap(),
                "[2001:db8::1]:53".parse().unwrap(),
                "[fe80::1]:53".parse().unwrap()
            ]
        );
        for input in [
            b"nameserver".as_slice(),
            b"nameserver invalid",
            b"nameserver 1.1.1.1 extra",
            b"\xff",
        ] {
            assert!(nameservers(input).is_empty());
        }
    }
}
