//! Independent Rust resource provider and caller for a Perry-authored guest.
use super::State;
use anyhow::{ensure, Context, Result};
use wasmtime::component::{
    ComponentType, Instance, Lift, Linker, Lower, Resource, ResourceType, Val,
};
use wasmtime::{Store, StoreContextMut};

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

fn call(
    store: &mut Store<State>,
    instance: &Instance,
    name: &str,
    params: &[Val],
    count: usize,
) -> Result<Vec<Val>> {
    store.set_fuel(100_000_000)?;
    let namespace = instance
        .get_export_index(&mut *store, None, "perry:guest-resources/counters@1.0.0")
        .context("missing counters")?;
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

pub(super) fn run(store: &mut Store<State>, instance: &Instance) -> Result<()> {
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
