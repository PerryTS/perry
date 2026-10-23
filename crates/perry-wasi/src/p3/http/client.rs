use super::{
    bindings::http::{
        client::{Host, HostWithStore},
        types::{ErrorCode, Request, Response},
    },
    body::Body,
    HttpError, HttpResult,
};
use crate::http::{
    p2::{get_content_length, WasiHttp, WasiHttpCtxView},
    transport, Error,
};
use futures_channel::oneshot;
use futures_util::future::BoxFuture;
use wasmtime::component::{Accessor, Resource};

impl<T: 'static> HostWithStore<T> for WasiHttp {
    async fn send(
        store: &Accessor<T, Self>,
        request: Resource<Request>,
    ) -> HttpResult<Resource<Response>> {
        let pending = store.with(|mut store| -> HttpResult<transport::PendingResponse> {
            let request = store.get().table.delete(request)?;
            let spec = (|| -> Result<_, Error> {
                let expected = get_content_length(&request.headers)?;
                let uri = http::Uri::builder()
                    .scheme(request.scheme.clone().unwrap_or(http::uri::Scheme::HTTPS))
                    .authority(
                        request
                            .authority
                            .clone()
                            .ok_or(Error::HttpRequestUriInvalid)?,
                    )
                    .path_and_query(
                        request
                            .path_with_query
                            .clone()
                            .ok_or(Error::HttpRequestUriInvalid)?,
                    )
                    .build()
                    .map_err(|_| Error::HttpRequestUriInvalid)?;
                Ok((
                    expected,
                    transport::RequestSpec {
                        method: request.method.to_string(),
                        uri,
                        headers: request.headers.clone(),
                    },
                ))
            })();
            let (expected, spec) = match spec {
                Ok(spec) => spec,
                Err(error) => {
                    request
                        .body
                        .fail(&mut store, error.clone())
                        .map_err(HttpError::trap)?;
                    return Err(error.into());
                }
            };
            let mut options = store.get().ctx.options.clone();
            if let Some(request_options) = request.options {
                if let Some(value) = request_options.connect_timeout {
                    options.connect_timeout = value;
                }
                if let Some(value) = request_options.first_byte_timeout {
                    options.first_byte_timeout = value;
                }
                if let Some(value) = request_options.between_bytes_timeout {
                    options.between_bytes_timeout = value;
                }
            }
            let (completed, completion) =
                oneshot::channel::<BoxFuture<'static, Result<(), Error>>>();
            let processing = Box::pin(async move {
                match completion.await {
                    Ok(result) => result.await,
                    Err(_) => Err(Error::ConnectionTerminated),
                }
            });
            let body = request
                .body
                .into_native(
                    &mut store,
                    processing,
                    expected,
                    ErrorCode::HttpRequestBodySize,
                )
                .map_err(HttpError::trap)?;
            let view = store.get();
            match transport::send(
                &view.ctx.submitter,
                view.ctx.policy.clone(),
                options,
                spec,
                body,
            ) {
                Ok(pending) => {
                    let _ = completed.send(pending.processing());
                    Ok(pending)
                }
                Err(error) => {
                    let failure = error.clone();
                    let _ = completed.send(Box::pin(async move { Err(failure) }));
                    Err(error.into())
                }
            }
        })?;
        let response = pending.receive().await?;
        store.with(|mut store| {
            let (body, _processing) = Body::native(response.body);
            let status = http::StatusCode::from_u16(response.status).map_err(HttpError::trap)?;
            Ok(store.get().table.push(Response {
                status,
                headers: response.headers,
                body,
            })?)
        })
    }
}
impl Host for WasiHttpCtxView<'_> {}
