use super::{
    bindings::http::{outgoing_handler, types},
    *,
};
use crate::http::body::{OutgoingBody, StreamContext};
use wasmtime::component::Resource;

impl outgoing_handler::Host for WasiHttpCtxView<'_> {
    fn handle(
        &mut self,
        request: Resource<HostOutgoingRequest>,
        options: Option<Resource<RequestOptions>>,
    ) -> HttpResult<Resource<HostFutureIncomingResponse>> {
        // A bad options handle is a trap, never silently treated as defaults.
        let options = options.map(|id| self.table.delete(id)).transpose()?;
        let request = self.table.delete(request)?;
        let mut settings = self.ctx.options.clone();
        if let Some(options) = options {
            if let Some(d) = options.connect_timeout {
                settings.connect_timeout = d;
            }
            if let Some(d) = options.first_byte_timeout {
                settings.first_byte_timeout = d;
            }
            if let Some(d) = options.between_bytes_timeout {
                settings.between_bytes_timeout = d;
            }
        }
        let method = http::Method::try_from(request.method)
            .map_err(|_| types::ErrorCode::HttpRequestMethodInvalid)?;
        let scheme = match request.scheme {
            Some(types::Scheme::Http) => "http",
            Some(types::Scheme::Https) | None => "https",
            Some(types::Scheme::Other(_)) => {
                return Err(types::ErrorCode::HttpRequestUriInvalid.into())
            }
        };
        let uri = http::Uri::builder()
            .scheme(scheme)
            .authority(request.authority.unwrap_or_default())
            .path_and_query(request.path_with_query.unwrap_or_else(|| "/".into()))
            .build()
            .map_err(|_| types::ErrorCode::HttpRequestUriInvalid)?;
        let body = match request.body {
            Some(body) => body,
            None => {
                let size = get_content_length(&request.headers).map_err(types::ErrorCode::from)?;
                let (body, receive) = OutgoingBody::new(StreamContext::Request, size);
                body.finish(None)
                    .map_err(HttpError::trap)?
                    .map_err(types::ErrorCode::from)?;
                receive
            }
        };
        let pending = transport::send(
            &self.ctx.submitter,
            self.ctx.policy.clone(),
            settings,
            transport::RequestSpec {
                method: method.to_string(),
                uri,
                headers: request.headers,
            },
            body,
        )
        .map_err(types::ErrorCode::from)?;
        let future = HostFutureIncomingResponse::Pending(Box::pin(pending.receive()));
        Ok(self.table.push(future)?)
    }
}
