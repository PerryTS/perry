//! Actual canonical HTTP guest through the production component Host facade.
use super::*;
use crate::{Host, Imports, Options};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::time::Duration;

#[test]
fn wasi_http_requires_opt_in_and_exact_tcp_dns_address_grants() {
    let (done, wait) = std::sync::mpsc::channel();
    let task = std::thread::spawn(move || {
        let bytes = wat::parse_str(include_str!(
            "../../perry-wasi/src/http/p2/http.component.wat"
        ))
        .unwrap();
        let world = perry_wit::parse("package perry:http-fixture; world guest { export run: func(authority: string) -> u32; }", None).unwrap();
        assert!(Host::new(&bytes, &world, &Imports::new(), &Options::default()).is_err());
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let grant = Address {
            address: address.ip(),
            port: Some(address.port()),
        };
        let cases = [
            (Network::default(), address.to_string(), 1015),
            (
                Network {
                    udp: true,
                    addresses: vec![grant.clone()],
                    ..Default::default()
                },
                address.to_string(),
                1015,
            ),
            (
                Network {
                    tcp: true,
                    ..Default::default()
                },
                address.to_string(),
                1004,
            ),
            (
                Network {
                    tcp: true,
                    addresses: vec![Address {
                        port: Some(0),
                        ..grant.clone()
                    }],
                    ..Default::default()
                },
                address.to_string(),
                1004,
            ),
            (
                Network {
                    tcp: true,
                    addresses: vec![grant.clone()],
                    ..Default::default()
                },
                format!("localhost:{}", address.port()),
                1015,
            ),
        ];
        for (network, authority, expected) in cases {
            let mut host = Host::new(
                &bytes,
                &world,
                &Imports::new(),
                &Options {
                    wasi: Some(WasiOptions {
                        network,
                        ..Default::default()
                    }),
                    ..Default::default()
                },
            )
            .unwrap();
            assert_eq!(
                host.call("", "run", &[authority.into()]).unwrap(),
                Some(expected.into())
            );
            assert!(host
                .live
                .as_ref()
                .unwrap()
                .store
                .data()
                .wasi
                .as_ref()
                .unwrap()
                .table
                .is_empty());
            host.dispose().unwrap();
        }
        listener.set_nonblocking(true).unwrap();
        assert_eq!(
            listener.accept().unwrap_err().kind(),
            std::io::ErrorKind::WouldBlock
        );
        listener.set_nonblocking(false).unwrap();
        let peer = std::thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            socket
                .set_read_timeout(Some(Duration::from_secs(10)))
                .unwrap();
            let mut head = Vec::new();
            while !head.ends_with(b"\r\n\r\n") {
                let mut b = [0];
                socket.read_exact(&mut b).unwrap();
                head.push(b[0]);
            }
            assert!(head.starts_with(b"GET / HTTP/1.1\r\n"));
            let mut body = [0; 5];
            socket.read_exact(&mut body).unwrap();
            assert_eq!(&body, b"0\r\n\r\n");
            socket
                .write_all(b"HTTP/1.1 202 Accepted\r\nContent-Length: 4\r\n\r\npong")
                .unwrap();
        });
        let mut host = Host::new(
            &bytes,
            &world,
            &Imports::new(),
            &Options {
                wasi: Some(WasiOptions {
                    network: Network {
                        tcp: true,
                        dns: true,
                        addresses: vec![grant],
                        ..Default::default()
                    },
                    ..Default::default()
                }),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(
            host.call("", "run", &[format!("localhost:{}", address.port()).into()])
                .unwrap(),
            Some(202.into())
        );
        assert!(host
            .live
            .as_ref()
            .unwrap()
            .store
            .data()
            .wasi
            .as_ref()
            .unwrap()
            .table
            .is_empty());
        host.dispose().unwrap();
        peer.join().unwrap();
        done.send(()).unwrap();
    });
    wait.recv_timeout(Duration::from_secs(30))
        .expect("component HTTP host watchdog");
    task.join().unwrap();
}
