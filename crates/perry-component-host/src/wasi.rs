//! Per-instance WASI Preview 2 capabilities. Nothing inherits ambient authority.
#[cfg(test)]
#[path = "wasi_http_tests.rs"]
mod http_execution_tests;
#[cfg(test)]
#[path = "wasi_network_tests.rs"]
mod network_execution_tests;

use anyhow::{Context, Result};
use perry_wasi::{filesystem::FsPerms, WasiCtx, WasiCtxView, WasiView};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::net::{IpAddr, SocketAddr};
use wasmtime::component::ResourceTable;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields, rename_all = "camelCase")]
pub struct WasiOptions {
    pub args: Vec<String>,
    pub env: BTreeMap<String, String>,
    pub inherit_stdin: bool,
    pub inherit_stdout: bool,
    pub inherit_stderr: bool,
    pub preopens: Vec<Preopen>,
    pub network: Network,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct Preopen {
    pub host_path: String,
    pub guest_path: String,
    #[serde(default = "read_only")]
    pub read_only: bool,
}

fn read_only() -> bool {
    true
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields, rename_all = "camelCase")]
pub struct Network {
    pub tcp: bool,
    pub udp: bool,
    pub dns: bool,
    pub addresses: Vec<Address>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct Address {
    /// An IP literal; names and CIDR ranges are deliberately not accepted.
    pub address: IpAddr,
    /// Omission explicitly grants every port on this address.
    pub port: Option<u16>,
}

impl Address {
    pub(crate) fn allows(&self, address: SocketAddr) -> bool {
        self.address == address.ip() && self.port.is_none_or(|port| port == address.port())
    }
}

pub(crate) struct State {
    ctx: WasiCtx,
    table: ResourceTable,
}

impl State {
    pub(crate) fn view(&mut self) -> WasiCtxView<'_> {
        WasiCtxView {
            ctx: &mut self.ctx,
            table: &mut self.table,
        }
    }
}

impl WasiOptions {
    pub(crate) fn build(&self) -> Result<State> {
        // Bound native work and persistent transports independently per
        // component. No process-global runtime or inherited network authority.
        let mut ctx = WasiCtx::new(64).map_err(anyhow::Error::from)?;
        ctx.cli.arguments = self.args.clone();
        ctx.cli.environment = self
            .env
            .iter()
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect();
        if self.inherit_stdin {
            ctx.cli.inherit_stdin();
        }
        if self.inherit_stdout {
            ctx.cli.inherit_stdout();
        }
        if self.inherit_stderr {
            ctx.cli.inherit_stderr();
        }
        for preopen in &self.preopens {
            ctx.preopened_dir(
                &preopen.host_path,
                &preopen.guest_path,
                if preopen.read_only {
                    FsPerms::ReadOnly
                } else {
                    FsPerms::ReadWrite
                },
            )
            .map_err(anyhow::Error::from)
            .with_context(|| format!("opening WASI preopen {}", preopen.guest_path))?;
        }
        ctx.sockets
            .allow_tcp(self.network.tcp)
            .allow_udp(self.network.udp)
            .allow_ip_name_lookup(self.network.dns);
        ctx.http.policy = perry_wasi::http::transport::DestinationPolicy {
            tcp: self.network.tcp,
            dns: self.network.dns,
            addresses: self
                .network
                .addresses
                .iter()
                .map(|entry| (entry.address, entry.port))
                .collect(),
        };
        let addresses = self.network.addresses.clone();
        ctx.sockets.socket_addr_check(move |address, _use| {
            let allowed = addresses.iter().any(|entry| entry.allows(address));
            Box::pin(async move { allowed })
        });
        Ok(State {
            ctx,
            table: ResourceTable::new(),
        })
    }
}

impl WasiView for super::StoreData {
    fn ctx(&mut self) -> WasiCtxView<'_> {
        let state = self
            .wasi
            .as_mut()
            .expect("WASI providers require explicit options");
        state.view()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use perry_wasi::p2::bindings::cli::environment::Host;

    #[test]
    fn defaults_are_closed_and_preopens_are_read_only() {
        let options: WasiOptions =
            serde_json::from_str(r#"{"preopens":[{"hostPath":".","guestPath":"/data"}]}"#).unwrap();
        assert!(options.preopens[0].read_only);
        assert!(options.args.is_empty() && options.env.is_empty());
        assert!(!options.inherit_stdin && !options.inherit_stdout && !options.inherit_stderr);
        assert!(!options.network.tcp && !options.network.udp && !options.network.dns);
        assert!(options.network.addresses.is_empty());
        assert!(serde_json::from_str::<WasiOptions>(r#"{"inheritEnv":true}"#).is_err());
        assert!(serde_json::from_str::<Address>(r#"{"address":"example.com"}"#).is_err());
    }

    #[test]
    fn address_grants_do_not_expand_to_other_ips_or_ports() {
        let grant = Address {
            address: "127.0.0.1".parse().unwrap(),
            port: Some(8080),
        };
        assert!(grant.allows("127.0.0.1:8080".parse().unwrap()));
        assert!(!grant.allows("127.0.0.1:8081".parse().unwrap()));
        assert!(!grant.allows("127.0.0.2:8080".parse().unwrap()));
        assert!(!grant.allows("[::1]:8080".parse().unwrap()));
        assert!(Address {
            port: None,
            ..grant
        }
        .allows("127.0.0.1:90".parse().unwrap()));
    }

    #[test]
    fn contexts_keep_args_and_environment_isolated() {
        let a = WasiOptions {
            args: vec!["first".into()],
            env: BTreeMap::from([("MARKER".into(), "one".into())]),
            ..Default::default()
        };
        let b = WasiOptions {
            args: vec!["second".into()],
            env: BTreeMap::from([("MARKER".into(), "two".into())]),
            ..Default::default()
        };
        for options in [&a, &b, &a, &WasiOptions::default()] {
            let mut state = options.build().unwrap();
            let mut view = perry_wasi::cli::WasiCliCtxView {
                ctx: &mut state.ctx.cli,
                table: &mut state.table,
            };
            assert_eq!(view.get_arguments().unwrap(), options.args);
            assert_eq!(
                view.get_environment().unwrap(),
                options
                    .env
                    .iter()
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect::<Vec<_>>()
            );
        }
    }
}

#[cfg(test)]
mod provider_execution_tests {
    use super::*;
    use crate::{Host, Imports, Options};
    use perry_wasi::p2::bindings::sockets::{instance_network, network, tcp, tcp_create_socket};
    use perry_wasi::sockets::WasiSocketsCtxView;

    #[test]
    fn declared_wasi_imports_are_linked_only_with_explicit_opt_in() {
        let world = perry_wit::parse(
            r#"
            package wasi:clocks@0.2.0;
            interface monotonic-clock { now: func() -> u64; }
            world plugin { import monotonic-clock; export monotonic-clock; }
        "#,
            None,
        )
        .unwrap();
        let bytes = wat::parse_str(
            r#"(component
            (import "wasi:clocks/monotonic-clock@0.2.0" (instance $clock
                (export "now" (func (result u64)))))
            (alias export $clock "now" (func $now))
            (core func $lower (canon lower (func $now)))
            (core module $code
                (import "env" "now" (func $now (result i64)))
                (func (export "now") (result i64) (call $now)))
            (core instance $env (export "now" (func $lower)))
            (core instance $code (instantiate $code (with "env" (instance $env))))
            (alias core export $code "now" (core func $forward))
            (func $forward (result u64) (canon lift (core func $forward)))
            (instance $exports (export "now" (func $forward)))
            (export "wasi:clocks/monotonic-clock@0.2.0" (instance $exports))
        )"#,
        )
        .unwrap();
        assert!(Host::new(&bytes, &world, &Imports::new(), &Options::default()).is_err());
        let mut host = Host::new(
            &bytes,
            &world,
            &Imports::new(),
            &Options {
                wasi: Some(WasiOptions::default()),
                ..Default::default()
            },
        )
        .unwrap();
        let now = host
            .call("wasi:clocks/monotonic-clock@0.2.0", "now", &[])
            .unwrap()
            .unwrap();
        assert!(now["$bigint"].as_str().unwrap().parse::<u64>().unwrap() > 0);
        host.dispose().unwrap();
    }

    fn bind(options: &WasiOptions, address: SocketAddr) -> Result<(), network::ErrorCode> {
        futures_lite::future::block_on(async {
            let mut state = options.build().unwrap();
            let mut view = WasiSocketsCtxView {
                ctx: &mut state.ctx.sockets,
                table: &mut state.table,
            };
            let net = instance_network::Host::instance_network(&mut view).unwrap();
            let socket = tcp_create_socket::Host::create_tcp_socket(
                &mut view,
                network::IpAddressFamily::Ipv4,
            )
            .map_err(|error| error.downcast().unwrap())?;
            tcp::HostTcpSocket::start_bind(&mut view, socket, net, address.into())
                .await
                .map_err(|error| error.downcast().unwrap())
        })
    }

    #[test]
    fn actual_socket_provider_requires_protocol_and_address_grants() {
        let address = "127.0.0.1:0".parse().unwrap();
        assert_eq!(
            bind(&WasiOptions::default(), address),
            Err(network::ErrorCode::AccessDenied)
        );
        let mut options = WasiOptions::default();
        options.network.tcp = true;
        assert_eq!(
            bind(&options, address),
            Err(network::ErrorCode::AccessDenied)
        );
        options.network.addresses.push(Address {
            address: "127.0.0.1".parse().unwrap(),
            port: Some(0),
        });
        assert_eq!(bind(&options, address), Ok(()));
        // An independently created context does not inherit the previous grant.
        assert_eq!(
            bind(&WasiOptions::default(), address),
            Err(network::ErrorCode::AccessDenied)
        );
        options.network.addresses[0].port = Some(1);
        assert_eq!(
            bind(&options, address),
            Err(network::ErrorCode::AccessDenied)
        );
    }
}

#[cfg(test)]
#[path = "wasi_contract_tests.rs"]
mod contract_tests;

#[cfg(test)]
#[path = "wasi_network_legacy_tests.rs"]
mod legacy_network_execution_tests;
