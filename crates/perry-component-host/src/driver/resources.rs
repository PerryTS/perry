//! Dispose typed owned resources inside the persistent Store run, without
//! borrowing Store across an await or calling legacy Host operations.
use super::{
    async_values::call,
    requests::{Command, Target},
    Completion,
};
use crate::{asynchronous::origin, cleanup::CollectedResource, StoreData};
use anyhow::{ensure, Context, Result};
use wasmtime::{
    component::{Accessor, Val},
    AsContextMut,
};

pub(super) async fn execute(
    accessor: &Accessor<StoreData>,
    command: Command,
    fuel: u64,
    reset: bool,
) -> Completion {
    let mut started = false;
    let result = origin::scoped(Some(command.origin), async {
        let Target::Resource(request, explicit) = &command.target else {
            unreachable!()
        };
        let prepared = loop {
            let attempt = accessor.with(|mut store| -> Result<_> {
                let data = store.data_mut();
                let (token, host) = match request {
                    CollectedResource::Guest { token } => {
                        if !explicit && !data.resources.owns(token) {
                            return Ok(Ok(None));
                        }
                        (token.as_str(), false)
                    }
                    CollectedResource::Host { resource } => {
                        let entry = data.host_resources.entries.get(&resource.token);
                        let valid = entry.is_some_and(|entry| {
                            entry.reference == *resource && entry.handle.is_some()
                        });
                        if !valid {
                            ensure!(
                            !explicit,
                            "host resource is stale, transferred or belongs to another component"
                        );
                            return Ok(Ok(None));
                        }
                        (resource.token.as_str(), true)
                    }
                };
                let resource = if host {
                    data.host_resources.entries[token]
                        .handle
                        .context("host resource is owned by the guest")?
                } else {
                    data.resources.owned(token)?
                };
                let function = data
                    .disposers
                    .iter()
                    .find(|(ty, _)| *ty == resource.ty())
                    .map(|(_, f)| *f)
                    .context("resource disposal requires Host::new_for_driver")?;
                let key = format!("{}:{token}", if host { "host" } else { "guest" });
                if data.borrowed.contains(&key) {
                    ensure!(
                        !explicit,
                        "resource is borrowed by an active component call"
                    );
                    return Ok(Err((data.borrowed.clone(), key)));
                }
                // Complete every ownership/type check before committing the handle.
                // There is no user code between commit and the concurrent call.
                command.started.set(true);
                started = true;
                if host {
                    data.host_resources.entries.get_mut(token).unwrap().handle = None;
                } else {
                    data.resources.remove(token)?;
                }
                if reset {
                    store.as_context_mut().set_fuel(fuel)?;
                    store.as_context_mut().set_epoch_deadline(1);
                }
                Ok(Ok(Some((function, resource))))
            })?;
            match attempt {
                Ok(prepared) => break prepared,
                Err((registry, key)) => registry.wait(key, command.origin).await,
            }
        };
        let Some((function, resource)) = prepared else {
            return Ok(None);
        };
        call(
            accessor,
            function,
            &[Val::Resource(resource)],
            &mut [],
            command.origin,
        )
        .await?;
        Ok(None)
    })
    .await;
    let retire = started && result.is_err();
    (command, result, retire)
}
