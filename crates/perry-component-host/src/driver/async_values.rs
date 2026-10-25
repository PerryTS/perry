use super::{
    requests::{Command, Target},
    Completion,
};
use crate::{async_values::Begin, asynchronous::origin, transport, StoreData};
use anyhow::{ensure, Context, Result};
use serde_json::{json, Value};
use wasmtime::{
    component::{Accessor, Func, Val},
    AsContextMut,
};

async fn call(
    accessor: &Accessor<StoreData>,
    function: Func,
    args: &[Val],
    results: &mut [Val],
    origin: crate::InvocationId,
) -> Result<()> {
    let call = accessor.with(|mut store| {
        let call = function.start_call_concurrent(store.as_context_mut(), args, results)?;
        store.data_mut().origins.register(call.task(), origin);
        Ok::<_, wasmtime::Error>(call)
    })?;
    origin::scoped(
        Some(origin),
        function.finish_call_concurrent(accessor, call),
    )
    .await?;
    Ok(())
}
fn field<'a>(packet: &'a Val, name: &str) -> Result<&'a Val> {
    let Val::Record(fields) = packet else {
        anyhow::bail!("invalid canonical read packet")
    };
    Ok(&fields
        .iter()
        .find(|(key, _)| key == name)
        .context("missing canonical read field")?
        .1)
}
pub(super) async fn execute(
    accessor: &Accessor<StoreData>,
    command: Command,
    fuel: u64,
    reset: bool,
) -> Completion {
    if command.cancelled.get() {
        return (
            command,
            Err(anyhow::anyhow!(
                "async operation cancelled before execution"
            )),
            false,
        );
    }
    let mut started = false;
    let mut expected_cancel = false;
    let result = origin::scoped(Some(command.origin), async {
        if reset {
            accessor.with(|mut store| {
                store.as_context_mut().set_fuel(fuel)?;
                store.as_context_mut().set_epoch_deadline(1);
                Ok::<_, wasmtime::Error>(())
            })?;
        }
        if let Target::Close(value, intents) = &command.target {
            let reader = accessor
                .with(|mut store| store.data_mut().async_values.close(value))
                .or_else(|error| {
                    if intents.iter().any(|intent| intent.closed.get()) {
                        Ok(None)
                    } else {
                        Err(error)
                    }
                })?;
            command.started.set(true);
            started = true;
            if let Some(mut reader) = reader {
                accessor.with(|mut store| match &mut reader {
                    Val::Future(reader) => reader.close(store.as_context_mut()),
                    Val::Stream(reader) => reader.close(store.as_context_mut()),
                    _ => unreachable!(),
                })?;
            }
            return Ok(None);
        }
        let Target::Read(value, capacity, intent) = &command.target else {
            unreachable!()
        };
        let intent = intent.as_ref().expect("registered read intent");
        if intent.cancelled.get() {
            expected_cancel = true;
            return Ok(None);
        }
        let begin =
            accessor.with(|mut store| store.data_mut().async_values.begin(value, *capacity))?;
        let Begin::Read(read) = begin else {
            return Ok(Some(json!({"done":true})));
        };
        command.started.set(true);
        started = true;
        let args = [
            read.reader.clone(),
            Val::U32(*capacity),
            Val::U32(read.operation.id),
        ];
        let mut output = [Val::Bool(false)];
        call(
            accessor,
            read.adapter.read,
            &args,
            &mut output,
            command.origin,
        )
        .await?;
        // Finalizers can queue host-resource cleanup while I/O is pending.
        // Drain it under this read's origin before committing its result; a
        // callback may submit a close even after the helper has returned.
        accessor.with(|mut store| crate::host_resources::drain(store.as_context_mut()))?;
        let Val::U32(status) = field(&output[0], "status")? else {
            anyhow::bail!("invalid canonical read status")
        };
        let Val::U32(count) = field(&output[0], "count")? else {
            anyhow::bail!("invalid canonical read count")
        };
        ensure!(
            *status <= 2 && count <= capacity,
            "invalid canonical read completion"
        );
        let Val::List(values) = field(&output[0], "values")? else {
            anyhow::bail!("invalid canonical read values")
        };
        let Val::Option(reader) = field(&output[0], "reader")? else {
            anyhow::bail!("invalid canonical continuation")
        };
        // Commit the read only after observing close submission, independent
        // of the mailbox's completion/command polling order.
        if intent.cancelled.get() {
            accessor.with(|mut store| store.data_mut().async_values.request_cancel(&read))?;
        }
        let cancelled = accessor.with(|mut store| {
            store
                .data_mut()
                .async_values
                .finish(&read, *status, *count, reader.as_deref().cloned())
        })?;
        intent.committed.set(true);
        if cancelled {
            intent.closed.set(true);
            if let Some(mut continuation) = reader.as_deref().cloned() {
                accessor.with(|mut store| match &mut continuation {
                    Val::Future(reader) => reader.close(store.as_context_mut()),
                    Val::Stream(reader) => reader.close(store.as_context_mut()),
                    _ => unreachable!(),
                })?;
            }
            if !values.is_empty() {
                call(
                    accessor,
                    read.adapter.dispose,
                    &[Val::List(values.clone())],
                    &mut [],
                    command.origin,
                )
                .await?;
            }
            // Cancellation is an expected value-level outcome, not Store failure.
            expected_cancel = true;
            return Ok(None);
        }
        let lifted = accessor.with(|mut store| {
            transport::lift(
                store.as_context_mut(),
                &Val::List(values.clone()),
                &read.adapter.list,
            )
        })?;
        let payload = match &read.adapter.ty {
            wasmtime::component::Type::Future(ty) => ty.ty().is_some(),
            wasmtime::component::Type::Stream(ty) => ty.ty().is_some(),
            _ => unreachable!(),
        };
        if matches!(read.adapter.ty, wasmtime::component::Type::Future(_)) {
            ensure!(*count == 1, "future closed without a value");
            Ok(if payload {
                Some(
                    lifted
                        .as_array()
                        .context("canonical future list")?
                        .first()
                        .context("missing future value")?
                        .clone(),
                )
            } else {
                None
            })
        } else {
            if *count == 0 && *status == 1 {
                return Ok(Some(json!({"done":true})));
            }
            let chunk = if payload {
                lifted
            } else {
                Value::Array(vec![Value::Null; *count as usize])
            };
            Ok(Some(json!({"done":false,"value":chunk})))
        }
    })
    .await;
    let (result, retire) = if expected_cancel {
        (
            Err(anyhow::anyhow!("component async read cancelled")),
            false,
        )
    } else {
        let retire = started && result.is_err();
        (result, retire)
    };
    (command, result, retire)
}
