//! Typed native HTTP handoff. Requests never pass through application JSON.
use super::{
    requests::{Command, Target},
    Completion, DriverCall, DriverClient,
};
use crate::{asynchronous::origin, InvocationId, StoreData};
use anyhow::{ensure, Context, Result};
use futures_util::{
    future::{select, Either},
    FutureExt,
};
use perry_wasi::{
    http::{
        p2::{WasiHttp, WasiHttpCtxView},
        server, Error,
    },
    p3::http::{bindings::http::types::ErrorCode, Request, Response},
};
use std::{
    cell::RefCell,
    future::Future,
    pin::Pin,
    rc::Rc,
    task::{Context as TaskContext, Poll},
};
use wasmtime::{
    component::{Accessor, Resource, TypedFunc},
    AsContextMut,
};

const INTERFACE: &str = "wasi:http/handler@0.3.0";
type Handler =
    TypedFunc<(Resource<Request>,), (std::result::Result<Resource<Response>, ErrorCode>,)>;
type TransportResult = Rc<RefCell<Option<std::result::Result<(), Error>>>>;
type NativeCompletion = futures_util::future::Shared<
    futures_util::future::BoxFuture<'static, std::result::Result<(), Error>>,
>;
pub(super) struct Exchange {
    request: server::Request,
    completion: NativeCompletion,
    result: TransportResult,
}

/// Completes after transport flush and this handler's validated task exit.
/// Dropping a started call retires its Store, as for ordinary driver calls.
pub struct DriverHttpCall {
    call: DriverCall,
    result: TransportResult,
    completion: NativeCompletion,
    error: Option<anyhow::Error>,
}
impl DriverHttpCall {
    pub fn id(&self) -> InvocationId {
        self.call.id()
    }
}
impl Future for DriverHttpCall {
    type Output = Result<std::result::Result<(), Error>>;
    fn poll(mut self: Pin<&mut Self>, cx: &mut TaskContext<'_>) -> Poll<Self::Output> {
        if self.error.is_none() {
            match Pin::new(&mut self.call).poll(cx) {
                Poll::Pending => return Poll::Pending,
                Poll::Ready(Ok(_)) => {
                    return Poll::Ready(
                        self.result
                            .borrow_mut()
                            .take()
                            .context("HTTP driver omitted transport completion"),
                    )
                }
                Poll::Ready(Err(error)) => self.error = Some(error),
            }
        }
        match Pin::new(&mut self.completion).poll(cx) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(_) => Poll::Ready(Err(self.error.take().unwrap())),
        }
    }
}
impl DriverClient {
    /// Queue a native P3 exchange on the persistent Store. The selected handler
    /// must have a locally observable callback ABI; nested/imported handlers
    /// remain on the serialized Host API until they can be tracked exactly.
    pub fn handle_http_async(
        &self,
        interface: &str,
        request: server::Request,
        completion: server::Completion,
    ) -> Result<DriverHttpCall> {
        self.state.assert_owner();
        ensure!(
            interface == INTERFACE,
            "unsupported P3 HTTP handler interface"
        );
        ensure!(
            self.state
                .tracked_exports
                .contains(&(interface.to_owned(), "handle".into())),
            "HTTP handler task exit is not observable; use the serialized Host API"
        );
        let result = TransportResult::default();
        let completion = completion.finish().boxed().shared();
        let exchange = Exchange {
            request,
            completion: completion.clone(),
            result: result.clone(),
        };
        let call = self.submit(
            interface,
            "handle",
            &[],
            Target::Http(Rc::new(RefCell::new(Some(exchange)))),
        )?;
        Ok(DriverHttpCall {
            call,
            result,
            completion,
            error: None,
        })
    }
}

pub(super) async fn execute(
    accessor: &Accessor<StoreData>,
    command: Command,
    fuel: u64,
    reset: bool,
) -> Completion {
    let Target::Http(exchange) = &command.target else {
        unreachable!()
    };
    let Exchange {
        mut request,
        completion,
        result: published,
    } = exchange
        .borrow_mut()
        .take()
        .expect("HTTP exchange executed once");
    let preflight = accessor.with(|mut store| -> Result<Handler> {
        ensure!(
            store.data_mut().wasi.is_some(),
            "HTTP handling requires explicit WASI options"
        );
        let function = command
            .function
            .context("missing selected HTTP handler export")?;
        ensure!(
            function.ty(&store).async_(),
            "P3 HTTP handler must use the canonical async calling convention"
        );
        let typed = function
            .typed(&store)
            .map_err(anyhow::Error::from)
            .context("invalid P3 HTTP handler signature")?;
        if reset {
            store.as_context_mut().set_fuel(fuel)?;
            store.as_context_mut().set_epoch_deadline(1);
        }
        Ok(typed)
    });
    let typed = match preflight {
        Ok(typed) => typed,
        Err(error) => {
            drop(request);
            let _ = completion.await;
            return (command, Err(error), false);
        }
    };
    request.request.headers.set_immutable();
    let guest_error = RefCell::new(None);
    let outcome = origin::scoped(Some(command.origin), async {
        let http: Accessor<StoreData, WasiHttp> = accessor.with_getter(|state| {
            let view = state.wasi.as_mut().unwrap().view();
            WasiHttpCtxView {
                ctx: &mut view.ctx.http,
                table: view.table,
            }
        });
        let server::Request {
            request,
            response: send_response,
        } = request;
        let (request, _processed) = Request::from_native(request);
        let resource = http.with(|mut store| store.get().table.push(request))?;
        command.started.set(true);
        let (call, exited) = accessor.with(|mut store| {
            let call = typed.start_call_concurrent(store.as_context_mut(), (resource,))?;
            store
                .data_mut()
                .origins
                .register(call.task(), command.origin);
            let exited = store.data_mut().task_exits.wait(command.origin);
            Ok::<_, wasmtime::Error>((call, exited))
        })?;
        let running = async {
            let (reply,) = typed.finish_call_concurrent(accessor, call).await?;
            let reply = match reply {
                Ok(response) => http.with(|mut store| {
                    let response = store.get().table.delete(response)?;
                    match response.into_native(&mut store, completion.clone().boxed()) {
                        Ok(response) => Ok(Ok(response)),
                        Err(error) => error.downcast().map(|error| Err(error.into())),
                    }
                }),
                Err(error) => {
                    let error: Error = error.into();
                    *guest_error.borrow_mut() = Some(error.clone());
                    Ok(Err(error))
                }
            }?;
            if send_response.send(reply).is_err() {
                return Ok(Err(Error::ConnectionTerminated));
            }
            let result = completion.clone().await;
            let expected = guest_error.borrow().as_ref() == result.as_ref().err();
            if result.is_ok() || expected {
                exited
                    .await
                    .context("HTTP handler retired before task exit")
                    .map_err(wasmtime::Error::from_anyhow)?;
            }
            wasmtime::error::Ok(result)
        };
        let finished = completion.clone();
        futures_util::pin_mut!(running, finished);
        let result = match select(running, finished).await {
            Either::Left((result, _)) => result,
            Either::Right((result, pending)) => {
                let expected = guest_error.borrow().as_ref() == result.as_ref().err();
                if result.is_ok() || expected {
                    pending.await
                } else {
                    Ok(result)
                }
            }
        };
        result
            .map_err(anyhow::Error::from)
            .context("P3 HTTP driver handler trapped")
    })
    .await;
    let retire = !matches!(&outcome, Ok(Ok(())))
        && !matches!((&outcome, &*guest_error.borrow()), (Ok(Err(error)), Some(expected)) if error == expected);
    let reply = outcome.map(|result| {
        *published.borrow_mut() = Some(result);
        None
    });
    (command, reply, retire)
}
