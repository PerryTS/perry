//! P3 HTTP keeps its Store running through native response transmission.
use crate::{
    asynchronous::{dispatch, Invocation},
    Host, StoreData,
};
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
use std::cell::RefCell;
use wasmtime::component::{Accessor, Resource, TypedFunc};

#[cfg(test)]
mod tests;

type Handler =
    TypedFunc<(Resource<Request>,), (std::result::Result<Resource<Response>, ErrorCode>,)>;

impl Host {
    /// Invoke the pinned `wasi:http/handler@0.3.0` export and drive its body
    /// streams until the embedding-owned transport completes. Receiving a
    /// request grants no outbound network authority. No Store enters I/O threads.
    ///
    /// The outer error reports validation or a guest trap; the inner result is
    /// native transport completion, including an explicit guest HTTP error.
    /// Successful exchanges and explicit guest errors preserve the Store.
    /// Traps, transport failures and cancellation retire it before releasing
    /// provider state. Completion means transport flush, not peer consumption.
    pub async fn handle_http_async(
        &mut self,
        interface: &str,
        request: server::Request,
        completion: server::Completion,
    ) -> Result<std::result::Result<(), Error>> {
        self.handle_http_async_with_retirement(interface, request, completion, || {})
            .await
    }

    /// The C bridge installs a fresh cleanup frame on every poll. Load its
    /// current token only when retiring, never retain a poll's borrowed frame.
    pub(crate) async fn handle_http_async_with_retirement(
        &mut self,
        interface: &str,
        mut request: server::Request,
        completion: server::Completion,
        before_retire: impl FnOnce(),
    ) -> Result<std::result::Result<(), Error>> {
        let preflight = (|| -> Result<Handler> {
            let live = self.live.as_mut().context("component is disposed")?;
            ensure!(
                live.store.data().wasi.is_some(),
                "HTTP handling requires explicit WASI options"
            );
            ensure!(
                interface == "wasi:http/handler@0.3.0",
                "unsupported P3 HTTP handler interface"
            );
            let function = *live
                .exports
                .get(&(interface.to_owned(), "handle".to_owned()))
                .context("missing selected P3 HTTP handler export")?;
            ensure!(
                function.ty(&live.store).async_(),
                "P3 HTTP handler must use the canonical async calling convention"
            );
            function
                .typed(&live.store)
                .map_err(anyhow::Error::from)
                .context("invalid P3 HTTP handler signature")
        })();
        let typed = match preflight {
            Ok(typed) => typed,
            Err(error) => {
                // Release the response sender before waiting for its 500 flush.
                drop(request);
                let _ = completion.finish().await;
                return Err(error);
            }
        };
        request.request.headers.set_immutable();
        let dispatcher = self.dispatcher.clone();
        let mut invocation = Invocation {
            live: self.live.take(),
            host: self,
            before_retire: Some(Box::new(before_retire)),
        };
        let completion = completion.finish().boxed().shared();
        let finished = completion.clone();
        let guest_error = RefCell::new(None);
        let outcome = {
            let live = invocation.live.as_mut().unwrap();
            live.store.set_fuel(live.fuel)?;
            live.store.set_epoch_deadline(1);
            let running = dispatch::drive(
                dispatcher.as_ref(),
                live.store.run_concurrent(async |accessor| {
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
                    let (reply,) = typed.call_concurrent(accessor, (resource,)).await?;
                    let reply = match reply {
                        Ok(response) => http.with(|mut store| {
                            let response = store.get().table.delete(response)?;
                            match response.into_native(&mut store, completion.boxed()) {
                                Ok(response) => Ok(Ok(response)),
                                Err(error) => error.downcast().map(|error| Err(error.into())),
                            }
                        }),
                        Err(error) => {
                            let error: Error = error.into();
                            *guest_error.borrow_mut() = Some(error.clone());
                            Ok(Err(error))
                        }
                    };
                    let reply = reply?;
                    if send_response.send(reply).is_err() {
                        return Ok(Err(Error::ConnectionTerminated));
                    }
                    let result = finished.clone().await;
                    let expected_error = guest_error.borrow().as_ref() == result.as_ref().err();
                    if result.is_ok() || expected_error {
                        // A handler may publish its response and keep a task
                        // alive to observe transmission completion. Let that
                        // task exit before returning its Store to the host.
                        std::future::poll_fn(|cx| accessor.poll_no_interesting_tasks(cx)).await;
                    }
                    wasmtime::error::Ok(result)
                }),
            );
            let completed = finished.clone();
            futures_util::pin_mut!(running, completed);
            match select(running, completed).await {
                Either::Left((result, _)) => result,
                Either::Right((result, pending)) => {
                    let expected_error = guest_error.borrow().as_ref() == result.as_ref().err();
                    if result.is_ok() || expected_error {
                        pending.await
                    } else {
                        // The pinned driver drops at the end of this block,
                        // before Invocation retires the suspended Store.
                        Ok(Ok(result))
                    }
                }
            }
            .map_err(anyhow::Error::from)
            .and_then(|result| result.map_err(anyhow::Error::from))
            .context("P3 HTTP handler trapped")
        };
        if matches!(&outcome, Ok(Ok(())))
            || matches!((&outcome, &*guest_error.borrow()), (Ok(Err(error)), Some(rejection)) if error == rejection)
        {
            invocation.restore();
        } else {
            let _ = invocation.retire();
        }
        if outcome.is_err() {
            // Guest failure closes its response/body channels. Keep the native
            // exchange alive to finish its error response after Store teardown.
            let _ = finished.await;
        }
        outcome
    }
}
