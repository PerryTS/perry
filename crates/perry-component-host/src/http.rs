//! Streaming `wasi:http/incoming-handler` invocation on the caller's Store.
//! The embedding supplies owned native request/response channels. The guest
//! cannot acquire a listener or outbound authority from receiving a request.
mod p3;
#[cfg(test)]
mod tests;

use crate::Host;
use anyhow::{ensure, Context, Result};
use futures_channel::oneshot;
use perry_wasi::http::{
    p2::{HostIncomingRequest, HostOutgoingResponse, HostResponseOutparam, WasiHttpCtxView},
    Error,
};
use wasmtime::component::Resource;

impl Host {
    /// Call the selected incoming-handler export. Request reads and response
    /// writes are driven concurrently by the embedding's owned native transport.
    ///
    /// No guest callback or Store enters that driver. Fuel and epoch limits apply
    /// as for ordinary exports. A trap or return without setting the response
    /// retires this instance: partially transferred canonical resources cannot
    /// safely be reused. Explicit guest set(err) is a valid handler return; the
    /// transport's completion reports the response error separately.
    pub fn handle_http(
        &mut self,
        interface: &str,
        request: HostIncomingRequest,
        response: oneshot::Sender<Result<HostOutgoingResponse, Error>>,
    ) -> Result<()> {
        self.handle_http_with_retirement(interface, request, response, || {})
    }

    /// The C bridge separates primary-call exceptions from cleanup exceptions.
    /// Switch callback context only after invocation has failed, before retiring.
    pub(crate) fn handle_http_with_retirement(
        &mut self,
        interface: &str,
        mut request: HostIncomingRequest,
        response: oneshot::Sender<Result<HostOutgoingResponse, Error>>,
        before_retire: impl FnOnce(),
    ) -> Result<()> {
        let live = self.live.as_mut().context("component is disposed")?;
        ensure!(
            live.store.data().wasi.is_some(),
            "HTTP handling requires explicit WASI options"
        );
        let func = *live
            .exports
            .get(&(interface.to_owned(), "handle".to_owned()))
            .context("missing selected HTTP incoming-handler export")?;
        // Check the actual canonical resource identities before publishing any
        // handles. A structurally similar custom resource is not a WASI request.
        let typed = func
            .typed::<(
                Resource<HostIncomingRequest>,
                Resource<HostResponseOutparam>,
            ), ()>(&live.store)
            .map_err(anyhow::Error::from)
            .context("invalid HTTP incoming-handler signature")?;
        live.store.set_fuel(live.fuel)?;
        live.store.set_epoch_deadline(1);
        request.headers.set_immutable();
        let (request, response, observer) = {
            let state = live.store.data_mut().wasi.as_mut().unwrap();
            let view = state.view();
            let mut http = WasiHttpCtxView {
                ctx: &mut view.ctx.http,
                table: view.table,
            };
            let req = http.table.push(request).map_err(anyhow::Error::from)?;
            let out = match http.new_response_outparam(response) {
                Ok(out) => out,
                Err(error) => {
                    let _ = http.table.delete(req);
                    return Err(error.into());
                }
            };
            let observer = http
                .table
                .get(&out)
                .map_err(anyhow::Error::from)?
                .observer();
            (req, out, observer)
        };
        let result = typed
            .call(&mut live.store, (request, response))
            .map_err(anyhow::Error::from)
            .context("HTTP incoming-handler trapped")
            .and_then(|()| {
                ensure!(
                    observer.is_set(),
                    "HTTP incoming-handler returned without setting response-outparam"
                );
                Ok(())
            });
        if result.is_err() {
            // Also release custom host resources through their normal destructor
            // path; preserve the original handler failure if cleanup also fails.
            before_retire();
            let _ = self.dispose();
        }
        result
    }
}
