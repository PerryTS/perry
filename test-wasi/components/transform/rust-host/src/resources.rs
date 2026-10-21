//! Independent Rust resource provider and caller for a Perry-authored guest.
use super::State;
use anyhow::{ensure, Context, Result};
use wasmtime::component::{
    ComponentType, Instance, Lift, Linker, Lower, Resource, ResourceType, Val,
};
use wasmtime::{Store, StoreContextMut};

// Forced evacuation plus verification walks the guest heap on every collect.
// Keep a finite per-call budget, with headroom for this instrumented profile.
const RESOURCE_FUEL: u64 = 1_000_000_000;

struct Token(u32);
#[derive(ComponentType, Lift, Lower)]
#[component(record)]
struct Pair {
    first: Resource<Token>,
    second: Resource<Token>,
}

pub(super) fn link(linker: &mut Linker<State>) -> Result<()> {
    let mut foreign = linker.instance("perry:guest-resources/foreign@1.0.0")?;
    foreign.resource("token", ResourceType::host::<Token>(), |mut store, rep| {
        let token = store
            .data_mut()
            .table
            .delete(Resource::<Token>::new_own(rep))?;
        store.data_mut().logs.push(format!("foreign:{}", token.0));
        Ok(())
    })?;
    foreign.func_wrap(
        "[constructor]token",
        |mut store: StoreContextMut<'_, State>, (value,): (u32,)| {
            Ok((store.data_mut().table.push(Token(value))?,))
        },
    )?;
    foreign.func_wrap(
        "[method]token.read",
        |store: StoreContextMut<'_, State>, (token,): (Resource<Token>,)| {
            Ok((store.data().table.get(&token)?.0,))
        },
    )?;
    foreign.func_wrap(
        "consume",
        |mut store: StoreContextMut<'_, State>, (token,): (Resource<Token>,)| {
            let value = store.data_mut().table.delete(token)?.0;
            store.data_mut().logs.push(format!("foreign:{value}"));
            Ok((value,))
        },
    )?;
    foreign.func_wrap(
        "consume-pair",
        |mut store: StoreContextMut<'_, State>, (pair,): (Pair,)| {
            let a = store.data_mut().table.delete(pair.first)?.0;
            let b = store.data_mut().table.delete(pair.second)?.0;
            store.data_mut().logs.push(format!("foreign:{a}"));
            store.data_mut().logs.push(format!("foreign:{b}"));
            Ok((a + b,))
        },
    )?;
    linker
        .instance("perry:guest-resources/observe@1.0.0")?
        .func_wrap(
            "dropped",
            |mut store: StoreContextMut<'_, State>, (value,): (u32,)| {
                store.data_mut().logs.push(format!("guest:{value}"));
                wasmtime::ensure!(value != 99, "host destructor failure");
                Ok(())
            },
        )?;
    Ok(())
}

fn call_interface(
    store: &mut Store<State>,
    instance: &Instance,
    interface: &str,
    name: &str,
    params: &[Val],
    count: usize,
) -> Result<Vec<Val>> {
    store.set_fuel(RESOURCE_FUEL)?;
    let namespace = instance
        .get_export_index(&mut *store, None, interface)
        .context("missing resource interface")?;
    let index = instance
        .get_export_index(&mut *store, Some(&namespace), name)
        .context("missing counter function")?;
    let function = instance
        .get_func(&mut *store, &index)
        .context("not a counter function")?;
    let mut results = vec![Val::Bool(false); count];
    function.call(&mut *store, params, &mut results)?;
    Ok(results)
}

fn call(
    store: &mut Store<State>,
    instance: &Instance,
    name: &str,
    params: &[Val],
    count: usize,
) -> Result<Vec<Val>> {
    call_interface(
        store,
        instance,
        "perry:guest-resources/counters@1.0.0",
        name,
        params,
        count,
    )
}

pub(super) fn run_dual(store: &mut Store<State>, instance: &Instance) -> Result<()> {
    let api = "perry:guest-resources/foreign@1.0.0";
    let a = call_interface(
        store,
        instance,
        api,
        "[constructor]token",
        &[Val::U32(30)],
        1,
    )?;
    let b = call_interface(
        store,
        instance,
        api,
        "[constructor]token",
        &[Val::U32(40)],
        1,
    )?;
    ensure!(
        call_interface(store, instance, api, "[method]token.read", &a, 1)? == [Val::U32(30)],
        "exported resource lost nominal identity"
    );
    let pair = Val::Record(vec![
        ("first".into(), a[0].clone()),
        ("second".into(), b[0].clone()),
    ]);
    ensure!(
        call_interface(store, instance, api, "consume-pair", &[pair], 1)? == [Val::U32(70)],
        "nested exported resources changed"
    );
    ensure!(
        store.data().logs == ["foreign:31", "foreign:41", "guest:30", "guest:40"],
        "resource directions or destructors were confused: {:?}",
        store.data().logs
    );
    println!("PASS dual resource interface: distinct import/export identities, cross-type rejection, nested transfer and destruction");
    Ok(())
}

pub(super) fn run(store: &mut Store<State>, instance: &Instance) -> Result<()> {
    store.set_fuel(RESOURCE_FUEL)?;
    let host_token = store.data_mut().table.push(Token(77))?;
    let namespace = instance
        .get_export_index(&mut *store, None, "perry:guest-resources/counters@1.0.0")
        .context("missing counters")?;
    let index = instance
        .get_export_index(&mut *store, Some(&namespace), "read-imported")
        .context("missing resource alias probe")?;
    let read = instance
        .get_func(&mut *store, &index)
        .context("missing alias function")?
        .typed::<(Resource<Token>,), (u32,)>(&*store)?;
    ensure!(
        read.call(&mut *store, (Resource::new_borrow(host_token.rep()),))? == (77,),
        "borrowed imported resource alias changed identity"
    );
    ensure!(
        store.data_mut().table.delete(host_token)?.0 == 77,
        "borrow consumed host ownership"
    );
    ensure!(
        call(store, instance, "exercise-imports", &[], 1)? == [Val::U32(4)],
        "import exercise did not finish"
    );
    ensure!(
        store.data().logs == ["foreign:17", "foreign:23", "foreign:5", "foreign:6"],
        "imported ownership/drop counts: {:?}",
        store.data().logs
    );
    for value in 100..150 {
        let results = call(
            store,
            instance,
            "[constructor]counter",
            &[Val::U32(value)],
            1,
        )?;
        let Val::Resource(resource) = results[0] else {
            anyhow::bail!("constructor did not return a resource");
        };
        let borrowed = Val::Resource(resource);
        ensure!(
            call(
                store,
                instance,
                "[method]counter.read",
                &[borrowed.clone()],
                1
            )? == [Val::U32(value)],
            "borrowed payload changed after guest GC"
        );
        call(store, instance, "keep-borrow", &[borrowed], 0)?;
        ensure!(
            call(store, instance, "check-expired", &[], 1)? == [Val::Bool(true)],
            "escaped borrow remained live"
        );
        resource.resource_drop(&mut *store)?;
        ensure!(
            store.data().logs.last() == Some(&format!("guest:{value}")),
            "missing destructor"
        );
    }
    let results = call(store, instance, "[constructor]counter", &[Val::U32(7)], 1)?;
    call(store, instance, "consume", &results, 0)?;
    ensure!(
        store.data().logs.len() == 55 && store.data().logs.last().unwrap() == "guest:7",
        "duplicate or missing destructor"
    );
    let results = call(store, instance, "[constructor]counter", &[Val::U32(13)], 1)?;
    let Val::Resource(resource) = results[0] else {
        anyhow::bail!("missing trap resource");
    };
    ensure!(
        resource.resource_drop(&mut *store).is_err(),
        "destructor trap was swallowed"
    );
    ensure!(
        store.data().logs.len() == 55,
        "trapping destructor unexpectedly notified host"
    );
    println!("PASS Perry guest resources: imported ownership, rollback, nested transfers, exported payload GC, expired borrows, 51 destructions, trap");
    Ok(())
}
