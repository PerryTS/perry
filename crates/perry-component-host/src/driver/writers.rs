use super::{
    async_values::{call, field},
    requests::{Command, Target, WriterAction},
    Completion,
};
use crate::{asynchronous::origin, transport, StoreData};
use anyhow::{ensure, Result};
use serde_json::{json, Value};
use wasmtime::{
    component::{Accessor, Type, Val},
    AsContextMut,
};

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
    let mut closed_write = false;
    let result=origin::scoped(Some(command.origin),async {
        if reset {accessor.with(|mut store|{
            store.as_context_mut().set_fuel(fuel)?;store.as_context_mut().set_epoch_deadline(1);Ok::<_,wasmtime::Error>(())
        })?;}
        if let Target::Factory(ty)=&command.target {
            let (index,adapter)=accessor.with(|mut store|store.data_mut().async_values.reserve_factory(ty))?;
            command.started.set(true);started=true;
            let mut output=[Val::Bool(false)];
            call(accessor,adapter.writer.new,&[],&mut output,command.origin).await?;
            let (value,token,stream)=accessor.with(|mut store|store.data_mut().async_values.factory_result(index,&output[0]))?;
            command.state().register_writer(token,stream);
            return Ok(Some(value));
        }
        if let Target::WriterControl(value,action,intents)=&command.target {
            let valid=accessor.with(|mut store|store.data_mut().async_values.has_writer(value));
            if let Err(error)=valid {
                ensure!(intents.iter().any(|i|i.closed.get()),"{error:#}");
                if *action!=WriterAction::Abort {return Ok(None)}
            }
            if *action==WriterAction::Abort {
                command.started.set(true);started=true;
                anyhow::bail!("component producer aborted; Store retired");
            }
            let operations=intents.iter().filter_map(|i|i.operation.get()).collect::<Vec<_>>();
            let writer=accessor.with(|mut store|store.data_mut().async_values.control_writer(value,*action==WriterAction::Close,&operations))?;
            command.started.set(true);started=true;
            if let Some(writer)=writer {
                let token=crate::async_values::writers::token(value)?;
                command.state().remove_writer(token);
                call(accessor,writer.adapter.writer.close,&[Val::U32(writer.core)],&mut [],command.origin).await?;
            }
            return Ok(None);
        }
        let Target::Write(value,intent)=&command.target else {unreachable!()};
        let intent=intent.as_ref().expect("registered writer intent");
        let write=accessor.with(|mut store|store.data_mut().async_values.prepare_write(value))?;
        let count=u32::try_from(command.args.len())?;
        let (stream,payload)=match &write.adapter.ty {
            Type::Future(ty)=>(false,ty.ty().is_some()),
            Type::Stream(ty)=>(true,ty.ty().is_some()),
            _=>unreachable!(),
        };
        ensure!((1..=65536).contains(&count)&&(stream||count==1),"invalid async write count");
        ensure!(payload||command.args.iter().all(Value::is_null),"unit writes require null placeholders");
        if intent.cancelled.get() {
            intent.committed.set(true);
            if intent.close.get() {closed_write=true;return Ok(None)}
            return Ok(Some(json!({"written":0,"remaining":command.args,"cancelled":true,"closed":false})));
        }
        let values=if payload {Value::Array(command.args.clone())} else {json!([])};
        let lowered=accessor.with(|mut store|->Result<Val>{
            let mut lowering=transport::Lowering::new(store.as_context_mut());
            let values=lowering.value(&values,&write.adapter.writer.list)?;
            lowering.commit()?;
            Ok(values)
        })?;
        // Ownership is committed. Every later failure must retire the Store.
        command.started.set(true);started=true;
        accessor.with(|mut store|store.data_mut().async_values.commit_write(&write));
        intent.operation.set(Some(write.operation.id));
        ensure!(!command.cancelled.get(),"write observer cancelled during argument conversion");
        if intent.cancelled.get() {accessor.with(|mut store|store.data_mut().async_values.signal_write(&write))?;}
        let args=[Val::U32(write.core),lowered,Val::U32(count),Val::U32(write.operation.id)];
        let mut output=[Val::Bool(false)];
        call(accessor,write.adapter.writer.write,&args,&mut output,command.origin).await?;
        accessor.with(|mut store|crate::host_resources::drain(store.as_context_mut()))?;
        let Val::U32(status)=field(&output[0],"status")? else{anyhow::bail!("invalid canonical write status")};
        let Val::U32(written)=field(&output[0],"count")? else{anyhow::bail!("invalid canonical write count")};
        let Val::List(remaining)=field(&output[0],"remaining")? else{anyhow::bail!("invalid canonical write remainder")};
        ensure!(*status<=2&&*written<=count,"invalid canonical writer completion");
        ensure!(remaining.len()==if payload {(count-written) as usize}else{0},"invalid canonical writer remainder length");
        let outcome=accessor.with(|mut store|store.data_mut().async_values.finish_write(&write,*status,*written,intent.cancelled.get(),intent.close.get()))?;
        intent.committed.set(true);
        if outcome.removed {intent.closed.set(true);command.state().remove_writer(&write.token);}
        if let Some(writer)=outcome.close {
            call(accessor,writer.adapter.writer.close,&[Val::U32(writer.core)],&mut [],command.origin).await?;
        }
        if outcome.discard {
            if !remaining.is_empty() {
                call(accessor,write.adapter.writer.dispose,&[Val::List(remaining.clone())],&mut [],command.origin).await?;
            }
            closed_write=true;
            return Ok(None);
        }
        let remaining=if payload {
            accessor.with(|mut store|transport::lift(store.as_context_mut(),&Val::List(remaining.clone()),&write.adapter.writer.list))?
        }else{Value::Array(vec![Value::Null;(count-written) as usize])};
        Ok(Some(json!({"written":written,"remaining":remaining,"cancelled":outcome.cancelled,"closed":outcome.removed})))
    }).await;
    let (result, retire) = if closed_write {
        (
            Err(anyhow::anyhow!(
                "component async write cancelled by writer close"
            )),
            false,
        )
    } else {
        let retire = started && result.is_err();
        (result, retire)
    };
    (command, result, retire)
}
