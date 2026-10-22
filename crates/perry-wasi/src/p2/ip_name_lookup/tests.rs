use super::*;
use crate::io::sync::wasi::io::poll::HostPollable;
use crate::{
    p2::bindings::sockets::{instance_network, network},
    WasiCtx, WasiCtxView, WasiView,
};
use ip_name_lookup::{Host, HostResolveAddressStream};
use std::{
    net::{Ipv4Addr, Ipv6Addr},
    time::{Duration, Instant},
};
use wasmtime::component::ResourceTable;

fn view<'a>(ctx: &'a mut WasiCtx, table: &'a mut ResourceTable) -> WasiSocketsCtxView<'a> {
    WasiSocketsCtxView {
        ctx: &mut ctx.sockets,
        table,
    }
}

#[test]
fn dns_capability_denial_and_invalid_network_are_distinct() {
    let mut ctx = WasiCtx::new(2).unwrap();
    let mut table = ResourceTable::new();
    let mut v = view(&mut ctx, &mut table);
    let network = instance_network::Host::instance_network(&mut v).unwrap();
    for name in ["localhost", "127.0.0.1", "::1"] {
        let error = v
            .resolve_addresses(Resource::new_borrow(network.rep()), name.into())
            .unwrap_err();
        assert_eq!(
            error.downcast().unwrap(),
            ErrorCode::PermanentResolverFailure
        );
    }
    let error = v
        .resolve_addresses(Resource::new_borrow(u32::MAX), "localhost".into())
        .unwrap_err();
    assert!(
        error.downcast().is_err(),
        "invalid resource must trap before capability checks"
    );
    network::HostNetwork::drop(&mut v, network).unwrap();
    assert_eq!(ctx.outstanding_io(), 0);
}

#[test]
fn dns_literals_validate_normalize_and_exhaust_without_native_work() {
    assert_eq!(
        parse_name("bücher.example").unwrap(),
        url::Host::Domain(String::from("xn--bcher-kva.example"))
    );
    for name in ["", "bad host", "a/b", "\0", "[broken]"] {
        assert_eq!(
            parse_name(name).unwrap_err().downcast().unwrap(),
            ErrorCode::InvalidArgument
        );
    }
    let mut ctx = WasiCtx::new(2).unwrap();
    ctx.sockets.allow_ip_name_lookup(true);
    let mut table = ResourceTable::new();
    let mut v = view(&mut ctx, &mut table);
    let network = instance_network::Host::instance_network(&mut v).unwrap();
    for (name, expected) in [
        ("127.0.0.1", IpAddr::V4(Ipv4Addr::LOCALHOST)),
        ("::1", IpAddr::V6(Ipv6Addr::LOCALHOST)),
        ("[::1]", IpAddr::V6(Ipv6Addr::LOCALHOST)),
        ("::ffff:127.0.0.1", IpAddr::V4(Ipv4Addr::LOCALHOST)),
        ("[::ffff:127.0.0.1]", IpAddr::V4(Ipv4Addr::LOCALHOST)),
    ] {
        let stream = v
            .resolve_addresses(Resource::new_borrow(network.rep()), name.into())
            .unwrap();
        assert_eq!(
            v.resolve_next_address(Resource::new_borrow(stream.rep()))
                .unwrap()
                .map(IpAddr::from),
            Some(expected)
        );
        for _ in 0..2 {
            assert!(v
                .resolve_next_address(Resource::new_borrow(stream.rep()))
                .unwrap()
                .is_none());
        }
        HostResolveAddressStream::drop(&mut v, stream).unwrap();
    }
    network::HostNetwork::drop(&mut v, network).unwrap();
    assert_eq!(ctx.outstanding_io(), 0);
}

#[test]
fn dns_localhost_resolves_through_turnloop_and_pollable_lifetime() {
    let mut ctx = WasiCtx::new(2).unwrap();
    ctx.sockets.allow_ip_name_lookup(true);
    let mut table = ResourceTable::new();
    let mut v = view(&mut ctx, &mut table);
    let network = instance_network::Host::instance_network(&mut v).unwrap();
    let stream = v
        .resolve_addresses(Resource::new_borrow(network.rep()), "localhost".into())
        .unwrap();
    let pollable = v.subscribe(Resource::new_borrow(stream.rep())).unwrap();
    assert!(
        HostResolveAddressStream::drop(&mut v, Resource::new_own(stream.rep())).is_err(),
        "live pollable must retain its parent"
    );
    HostPollable::block(&mut table, Resource::new_borrow(pollable.rep())).unwrap();
    assert!(HostPollable::ready(&mut table, Resource::new_borrow(pollable.rep())).unwrap());
    HostPollable::drop(&mut table, pollable).unwrap();
    let mut v = view(&mut ctx, &mut table);
    let mut addresses = Vec::new();
    while let Some(address) = v
        .resolve_next_address(Resource::new_borrow(stream.rep()))
        .unwrap()
    {
        addresses.push(IpAddr::from(address));
    }
    assert!(!addresses.is_empty());
    assert!(addresses.iter().all(IpAddr::is_loopback));
    HostResolveAddressStream::drop(&mut v, stream).unwrap();
    network::HostNetwork::drop(&mut v, network).unwrap();
    assert_eq!(ctx.outstanding_io(), 0);
}

#[test]
fn dns_pending_drop_releases_driver_capacity() {
    let mut ctx = WasiCtx::new(1).unwrap();
    let request = ctx
        .sockets
        .submitter
        .submit(|io| async move {
            io.sleep(Duration::from_secs(60)).await?;
            Ok(vec![IpAddr::V4(Ipv4Addr::LOCALHOST)])
        })
        .unwrap();
    let mut table = ResourceTable::new();
    let stream = table
        .push(ResolveAddressStream(Resolution::Pending(request)))
        .unwrap();
    let mut v = view(&mut ctx, &mut table);
    assert_eq!(
        v.resolve_next_address(Resource::new_borrow(stream.rep()))
            .unwrap_err()
            .downcast()
            .unwrap(),
        ErrorCode::WouldBlock
    );
    HostResolveAddressStream::drop(&mut v, stream).unwrap();
    let until = Instant::now() + Duration::from_secs(5);
    while ctx.outstanding_io() != 0 {
        assert!(
            Instant::now() < until,
            "DNS cancellation did not release admission"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
}

#[test]
fn dns_native_failure_is_typed_but_driver_loss_traps_repeatedly() {
    let mut ctx = WasiCtx::new(2).unwrap();
    let mut table = ResourceTable::new();
    let request = ctx
        .sockets
        .submitter
        .submit(|_| async { Err(turnloop::Error::new(turnloop::ErrorKind::Other)) })
        .unwrap();
    let stream = table
        .push(ResolveAddressStream(Resolution::Pending(request)))
        .unwrap();
    let pollable = view(&mut ctx, &mut table)
        .subscribe(Resource::new_borrow(stream.rep()))
        .unwrap();
    HostPollable::block(&mut table, Resource::new_borrow(pollable.rep())).unwrap();
    for _ in 0..2 {
        assert_eq!(
            view(&mut ctx, &mut table)
                .resolve_next_address(Resource::new_borrow(stream.rep()))
                .unwrap_err()
                .downcast()
                .unwrap(),
            ErrorCode::NameUnresolvable
        );
    }
    HostPollable::drop(&mut table, pollable).unwrap();
    HostResolveAddressStream::drop(&mut view(&mut ctx, &mut table), stream).unwrap();

    let request = ctx
        .sockets
        .submitter
        .submit(|io| async move {
            io.sleep(Duration::from_secs(60)).await?;
            Ok(Vec::new())
        })
        .unwrap();
    let stream = table
        .push(ResolveAddressStream(Resolution::Pending(request)))
        .unwrap();
    let pollable = view(&mut ctx, &mut table)
        .subscribe(Resource::new_borrow(stream.rep()))
        .unwrap();
    drop(ctx);
    for _ in 0..2 {
        let error =
            HostPollable::block(&mut table, Resource::new_borrow(pollable.rep())).unwrap_err();
        assert!(error.to_string().contains("DNS driver failed"));
    }
    HostPollable::drop(&mut table, pollable).unwrap();
    table.delete(stream).unwrap();
}

#[test]
fn dns_interfaces_register_in_both_linkers_and_project_native_errors() {
    struct State {
        ctx: WasiCtx,
        table: ResourceTable,
    }
    impl WasiView for State {
        fn ctx(&mut self) -> WasiCtxView<'_> {
            WasiCtxView {
                ctx: &mut self.ctx,
                table: &mut self.table,
            }
        }
    }
    let engine = wasmtime::Engine::default();
    let mut sync = wasmtime::component::Linker::<State>::new(&engine);
    let mut asynchronous = wasmtime::component::Linker::<State>::new(&engine);
    crate::p2::add_to_linker_sync(&mut sync).unwrap();
    crate::p2::add_to_linker_async(&mut asynchronous).unwrap();
    let mut ctx = WasiCtx::new(1).unwrap();
    let mut table = ResourceTable::new();
    let native = table
        .push(wasmtime::Error::new(std::io::Error::from(
            std::io::ErrorKind::ConnectionRefused,
        )))
        .unwrap();
    assert_eq!(
        network::Host::network_error_code(
            &mut view(&mut ctx, &mut table),
            Resource::new_borrow(native.rep())
        )
        .unwrap(),
        Some(ErrorCode::ConnectionRefused)
    );
    table.delete(native).unwrap();
}

#[test]
fn dns_component_uses_versioned_resources_and_canonical_results() {
    struct State {
        ctx: WasiCtx,
        table: ResourceTable,
    }
    impl WasiView for State {
        fn ctx(&mut self) -> WasiCtxView<'_> {
            WasiCtxView {
                ctx: &mut self.ctx,
                table: &mut self.table,
            }
        }
    }
    // Both Store execution modes must execute the same 0.2.0 guest against
    // 0.2.12 providers, including Turnloop-backed blocking poll imports.
    for asynchronous in [false, true] {
        let engine = wasmtime::Engine::default();
        let component =
            wasmtime::component::Component::new(&engine, include_str!("dns.component.wat"))
                .unwrap();
        let mut linker = wasmtime::component::Linker::new(&engine);
        if asynchronous {
            crate::p2::add_to_linker_async(&mut linker).unwrap();
        } else {
            crate::p2::add_to_linker_sync(&mut linker).unwrap();
        }
        let mut store = wasmtime::Store::new(
            &engine,
            State {
                ctx: WasiCtx::new(4).unwrap(),
                table: ResourceTable::new(),
            },
        );
        let instance = if asynchronous {
            futures_lite::future::block_on(linker.instantiate_async(&mut store, &component))
                .unwrap()
        } else {
            linker.instantiate(&mut store, &component).unwrap()
        };
        let run = instance
            .get_typed_func::<(&str, u32), (u32,)>(&mut store, "run")
            .unwrap();
        for (allowed, name, expected_error) in [
            (false, "localhost", 20),
            (false, "127.0.0.1", 20),
            (true, "bad host", 3),
            (true, "127.0.0.1", 0),
            (true, "::1", 0),
            (true, "::ffff:127.0.0.1", 0),
            (true, "localhost", 0),
        ] {
            store.data_mut().ctx.sockets.allow_ip_name_lookup(allowed);
            let count = if asynchronous {
                futures_lite::future::block_on(run.call_async(&mut store, (name, expected_error)))
                    .unwrap()
                    .0
            } else {
                run.call(&mut store, (name, expected_error)).unwrap().0
            };
            assert_eq!(count > 0, expected_error == 0, "{name}");
            assert_eq!(store.data().ctx.outstanding_io(), 0);
            // Every guest-owned network, resolver and pollable was dropped.
            assert!(store.data().table.is_empty());
        }
    }
}
