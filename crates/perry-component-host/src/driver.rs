//! A persistent, owner-thread Store driver. Results can be published before
//! guest tasks exit, and subsequent commands share the same run_concurrent.
mod async_values;
mod requests;
#[cfg(test)]
mod tests;
mod writers;
use crate::{
    asynchronous::{dispatch, origin, Invocation},
    transport, Host,
};
use anyhow::{ensure, Context, Result};
use futures_channel::mpsc;
use futures_util::{stream::FuturesUnordered, StreamExt};
use requests::{Command, State};
pub use requests::{DriverCall, DriverClient};
use serde_json::Value;
use std::{
    future::Future,
    pin::Pin,
    rc::Rc,
    task::{Context as TaskContext, Poll},
};
use wasmtime::{
    component::{Accessor, Func, Val},
    AsContextMut,
};

type Run = Pin<Box<dyn Future<Output = Result<Host>>>>;

/// Poll this driver on the creating thread while its client submits calls.
/// Unlike Host::call_async, exported results do not stop background guest tasks.
/// Drop an unconsumed call after it starts to retire the component. Dropping
/// the last client returns the Host if quiescent and retires it otherwise.
/// Generic future/stream results use Store-local owned JSON descriptors.
/// Read or close them through DriverClient; transfer them as call arguments.
/// TypeScript wrappers and producer interfaces remain unfinished.
pub struct Driver {
    state: Rc<State>,
    run: Option<Run>,
}
impl Driver {
    /// Whether calls, commands or interesting guest tasks need an event loop.
    /// An idle driver does not by itself keep an embedding's event loop alive.
    pub fn is_active(&self) -> bool {
        self.state.assert_owner();
        self.state.active.get()
    }
}
impl Future for Driver {
    type Output = Result<Host>;
    fn poll(mut self: Pin<&mut Self>, cx: &mut TaskContext<'_>) -> Poll<Self::Output> {
        self.state.assert_owner();
        self.state.waker.register(cx.waker());
        if self.state.abort.get() {
            self.state.alive.set(false);
            self.run.take();
            self.state
                .fail_all("component driver call cancelled; Store retired");
            self.state.active.set(false);
            return Poll::Ready(Err(anyhow::anyhow!(
                "component driver call cancelled; Store retired"
            )));
        }
        let result = self
            .run
            .as_mut()
            .expect("component driver polled after completion")
            .as_mut()
            .poll(cx);
        if result.is_ready() {
            self.run.take();
            self.state.alive.set(false);
            self.state.active.set(false);
        }
        result
    }
}
impl Drop for Driver {
    fn drop(&mut self) {
        self.state.assert_owner();
        self.state.alive.set(false);
        self.run.take();
        self.state
            .fail_all("component driver dropped; Store retired");
        self.state.active.set(false);
    }
}

impl Host {
    /// Move the component into a persistent Rust driver. The caller polls Driver
    /// on this thread, submits through DriverClient and awaits individual calls.
    /// The driver continues imports after an exported result was published.
    /// Overlapping and post-return tasks share one fuel budget, replenished
    /// only after the Store becomes quiescent; new calls cannot refill it.
    pub fn into_driver(self) -> Result<(Driver, DriverClient)> {
        self.into_driver_with_retirement(|| {})
    }
    pub(crate) fn into_driver_with_retirement(
        self,
        before_retire: impl FnOnce() + 'static,
    ) -> Result<(Driver, DriverClient)> {
        ensure!(self.live.is_some(), "component is disposed");
        let (send, receive) = mpsc::unbounded();
        let state = Rc::new(State::new(
            self.live
                .as_ref()
                .unwrap()
                .store
                .data()
                .async_values
                .types(),
            self.live
                .as_ref()
                .unwrap()
                .store
                .data()
                .origins
                .registry
                .clone(),
        ));
        for (token, stream) in self
            .live
            .as_ref()
            .unwrap()
            .store
            .data()
            .async_values
            .writer_metadata()
        {
            state.register_writer(token, stream);
        }
        let driver = Driver {
            state: state.clone(),
            run: Some(Box::pin(run(
                self,
                receive,
                state.clone(),
                Box::new(before_retire),
            ))),
        };
        Ok((driver, DriverClient::new(send, state)))
    }
}

type Reply = Result<Option<Value>>;
type Completion = (Command, Reply, bool);

async fn execute(
    accessor: &Accessor<crate::StoreData>,
    command: Command,
    fuel: u64,
    reset_budget: bool,
) -> Completion {
    if !matches!(command.target, requests::Target::Export) {
        return async_values::execute(accessor, command, fuel, reset_budget).await;
    }
    let function = command
        .function
        .expect("export command has a validated function");
    if command.cancelled.get() {
        return (
            command,
            Err(anyhow::anyhow!("component call cancelled before execution")),
            false,
        );
    }
    let prepared = accessor.with(|mut store| -> Result<_> {
        let _scope = origin::Scope::enter(Some(command.origin));

        let ty = function.ty(&store);
        let params: Vec<_> = ty.params().map(|(_, ty)| ty).collect();
        ensure!(
            params.len() == command.args.len(),
            "component argument count mismatch"
        );
        let mut lowering = transport::Lowering::new(store.as_context_mut());
        let args = command
            .args
            .iter()
            .zip(&params)
            .map(|(v, t)| lowering.value(v, t))
            .collect::<Result<Vec<_>>>()?;
        lowering.commit()?;
        if reset_budget {
            store.as_context_mut().set_fuel(fuel)?;
            store.as_context_mut().set_epoch_deadline(1);
        }
        let result_count = ty.results().len();
        Ok((args, vec![Val::Bool(false); result_count]))
    });
    let (args, mut output) = match prepared {
        Ok(prepared) => prepared,
        Err(error) => return (command, Err(error), false),
    };
    if command.cancelled.get() {
        return (
            command,
            Err(anyhow::anyhow!(
                "component call cancelled during argument conversion"
            )),
            true,
        );
    }
    // No engine operation or user code runs between commit/start and this flag.
    command.started.set(true);
    let call = accessor.with(|mut store| {
        let _scope = origin::Scope::enter(Some(command.origin));
        let call = function.start_call_concurrent(store.as_context_mut(), &args, &mut output)?;
        store
            .data_mut()
            .origins
            .register(call.task(), command.origin);
        Ok::<_, wasmtime::Error>(call)
    });
    let call = match call {
        Ok(call) => call,
        Err(error) => return (command, Err(error.into()), true),
    };
    let result = origin::scoped(
        Some(command.origin),
        function.finish_call_concurrent(accessor, call),
    )
    .await;
    let result = result
        .map_err(anyhow::Error::from)
        .context("component driver call trapped")
        .and_then(|()| {
            accessor.with(|mut store| {
                let _scope = origin::Scope::enter(Some(command.origin));
                match output.as_slice() {
                    [] => Ok(None),
                    [value] => {
                        let ty = function.ty(&store).results().next().unwrap();
                        transport::lift(store.as_context_mut(), value, &ty).map(Some)
                    }
                    _ => anyhow::bail!("selected WIT function has multiple results"),
                }
            })
        });
    let retire = result.is_err();
    (command, result, retire)
}

async fn run(
    mut host: Host,
    mut receive: mpsc::UnboundedReceiver<requests::Request>,
    state: Rc<State>,
    before_retire: Box<dyn FnOnce()>,
) -> Result<Host> {
    let dispatcher = host.dispatcher.clone();
    let mut invocation = Invocation {
        live: host.live.take(),
        host: &mut host,
        before_retire: Some(before_retire),
    };
    let outcome = {
        let live = invocation.live.as_mut().unwrap();
        let exports = &live.exports;
        let fuel = live.fuel;
        dispatch::drive(
            dispatcher.as_ref(),
            live.store.run_concurrent(async |accessor| {
                let mut pending = FuturesUnordered::new();
                let mut fresh_budget = true;
                enum Event {
                    Request(Option<requests::Request>),
                    Complete(Completion),
                }
                loop {
                    let event = std::future::poll_fn(|cx| {
                        let idle =
                            pending.is_empty() && accessor.poll_no_interesting_tasks(cx).is_ready();
                        state.active.set(!idle);
                        if idle {
                            fresh_budget = true;
                            accessor.with(|mut store| store.data_mut().origins.clear());
                        }
                        if let Poll::Ready(Some(completed)) = pending.poll_next_unpin(cx) {
                            return Poll::Ready(Event::Complete(completed));
                        }
                        if let Poll::Ready(request) = receive.poll_next_unpin(cx) {
                            return Poll::Ready(Event::Request(request));
                        }
                        Poll::Pending
                    })
                    .await;
                    match event {
                        Event::Request(Some(request)) => {
                            if request.cancelled.get() {
                                request.reply.send(Err(anyhow::anyhow!(
                                    "component call cancelled before execution"
                                )));
                                continue;
                            }
                            if !matches!(request.target, requests::Target::Export) {
                                let reset = std::mem::replace(&mut fresh_budget, false);
                                pending.push(execute(
                                    accessor,
                                    Command::new(request, None),
                                    fuel,
                                    reset,
                                ));
                                continue;
                            }
                            let function: Option<Func> = exports
                                .get(&(request.interface.clone(), request.name.clone()))
                                .copied();
                            match function {
                                Some(function) => {
                                    let reset = std::mem::replace(&mut fresh_budget, false);
                                    pending.push(execute(
                                        accessor,
                                        Command::new(request, Some(function)),
                                        fuel,
                                        reset,
                                    ));
                                }
                                None => {
                                    request.reply.send(Err(anyhow::anyhow!(
                                        "function is outside the selected WIT world"
                                    )));
                                }
                            }
                        }
                        Event::Request(None) => {
                            ensure!(
                                pending.is_empty()
                                    && std::future::poll_fn(|cx| Poll::Ready(
                                        accessor.poll_no_interesting_tasks(cx).is_ready()
                                    ))
                                    .await,
                                "component driver client closed with active guest tasks"
                            );
                            return Ok::<_, anyhow::Error>(());
                        }
                        Event::Complete((command, result, retire)) => {
                            let _scope = origin::Scope::enter(Some(command.origin));
                            command.reply.send(result);
                            ensure!(!retire, "component driver call failed; Store retired");
                        }
                    }
                }
            }),
        )
        .await
    };
    match outcome {
        Ok(Ok(())) => {
            invocation
                .live
                .as_mut()
                .unwrap()
                .store
                .data_mut()
                .origins
                .clear();
            invocation.restore();
        }
        Ok(Err(error)) => {
            state.alive.set(false);
            let _ = invocation.retire();
            state.fail_all(&format!("component driver retired: {error:#}"));
            return Err(error);
        }
        Err(error) => {
            state.alive.set(false);
            let _ = invocation.retire();
            state.fail_all(&format!("component driver retired: {error:#}"));
            return Err(error.into());
        }
    }
    drop(invocation);
    Ok(host)
}
