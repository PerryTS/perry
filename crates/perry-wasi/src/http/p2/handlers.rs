// Adapted from Wasmtime 48.0.5; Apache-2.0 WITH LLVM-exception. See PROVENANCE.md.
use super::{
    bindings::http::types::{self, Method, Scheme, StatusCode, Trailers},
    *,
};
use crate::http::{
    body::{
        FutureTrailers as HostFutureTrailers, IncomingBody as HostIncomingBody,
        OutgoingBody as HostOutgoingBody, StreamContext,
    },
    FieldMap,
};
use crate::io::poll::DynPollable;
use http::HeaderName;
use std::str::FromStr;
use wasmtime::component::Resource;
use wasmtime::error::Context as _;
use wasmtime_wasi_io::streams::{DynInputStream, DynOutputStream};
impl types::Host for WasiHttpCtxView<'_> {
    fn convert_error_code(&mut self, err: HttpError) -> wasmtime::Result<types::ErrorCode> {
        err.downcast()
    }

    fn convert_header_error(&mut self, err: HeaderError) -> wasmtime::Result<types::HeaderError> {
        err.downcast()
    }

    fn http_error_code(
        &mut self,
        err: wasmtime::component::Resource<types::IoError>,
    ) -> wasmtime::Result<Option<types::ErrorCode>> {
        let e = self.table.get(&err)?;
        Ok(e.downcast_ref::<crate::http::Error>()
            .cloned()
            .map(Into::into)
            .or_else(|| e.downcast_ref::<types::ErrorCode>().cloned()))
    }
}

impl types::HostFields for WasiHttpCtxView<'_> {
    fn new(&mut self) -> wasmtime::Result<Resource<FieldMap>> {
        let limit = self.ctx.field_size_limit;
        let id = self
            .table
            .push(FieldMap::new_mutable(limit))
            .context("[new_fields] pushing fields")?;

        Ok(id)
    }

    fn from_list(&mut self, entries: Vec<(String, Vec<u8>)>) -> HeaderResult<Resource<FieldMap>> {
        let mut fields = FieldMap::new_mutable(self.ctx.field_size_limit);

        for (header, value) in entries {
            fields.append(header, value)?;
        }

        Ok(self.table.push(fields)?)
    }

    fn drop(&mut self, fields: Resource<FieldMap>) -> wasmtime::Result<()> {
        self.table
            .delete(fields)
            .context("[drop_fields] deleting fields")?;
        Ok(())
    }

    fn get(&mut self, fields: Resource<FieldMap>, name: String) -> wasmtime::Result<Vec<Vec<u8>>> {
        let fields = self.table.get(&fields)?;

        let header = match HeaderName::from_bytes(name.as_bytes()) {
            Ok(header) => header,
            Err(_) => return Ok(vec![]),
        };

        if !fields.contains_key(&header) {
            return Ok(vec![]);
        }

        let res = fields
            .get_all(&header)
            .into_iter()
            .map(|val| val.as_bytes().to_owned())
            .collect();
        Ok(res)
    }

    fn has(&mut self, fields: Resource<FieldMap>, name: String) -> wasmtime::Result<bool> {
        let fields = self.table.get(&fields)?;

        match HeaderName::from_bytes(name.as_bytes()) {
            Ok(header) => Ok(fields.contains_key(&header)),
            Err(_) => Ok(false),
        }
    }

    fn set(
        &mut self,
        fields: Resource<FieldMap>,
        name: String,
        values: Vec<Vec<u8>>,
    ) -> HeaderResult<()> {
        let fields = self.table.get_mut(&fields)?;
        fields.set(name, values)?;
        Ok(())
    }

    fn delete(&mut self, fields: Resource<FieldMap>, name: String) -> HeaderResult<()> {
        let fields = self.table.get_mut(&fields)?;
        fields.remove_all(name)?;
        Ok(())
    }

    fn append(
        &mut self,
        fields: Resource<FieldMap>,
        name: String,
        value: Vec<u8>,
    ) -> HeaderResult<()> {
        let fields = self.table.get_mut(&fields)?;
        fields.append(name, value)?;
        Ok(())
    }

    fn entries(&mut self, fields: Resource<FieldMap>) -> wasmtime::Result<Vec<(String, Vec<u8>)>> {
        Ok(self
            .table
            .get(&fields)?
            .iter()
            .map(|(name, value)| (name.as_str().to_owned(), value.as_bytes().to_owned()))
            .collect())
    }

    fn clone(&mut self, fields: Resource<FieldMap>) -> wasmtime::Result<Resource<FieldMap>> {
        let mut fields = self.table.get(&fields)?.clone();
        fields.set_mutable(self.ctx.field_size_limit);
        let id = self.table.push(fields)?;
        Ok(id)
    }
}

impl types::HostIncomingRequest for WasiHttpCtxView<'_> {
    fn method(&mut self, id: Resource<HostIncomingRequest>) -> wasmtime::Result<Method> {
        let method = self.table.get(&id)?.method.clone();
        Ok(method.into())
    }
    fn path_with_query(
        &mut self,
        id: Resource<HostIncomingRequest>,
    ) -> wasmtime::Result<Option<String>> {
        let req = self.table.get(&id)?;
        Ok(req
            .uri
            .path_and_query()
            .map(|path_and_query| path_and_query.as_str().to_owned()))
    }
    fn scheme(&mut self, id: Resource<HostIncomingRequest>) -> wasmtime::Result<Option<Scheme>> {
        let req = self.table.get(&id)?;
        Ok(Some(req.scheme.clone()))
    }
    fn authority(&mut self, id: Resource<HostIncomingRequest>) -> wasmtime::Result<Option<String>> {
        let req = self.table.get(&id)?;
        Ok(Some(req.authority.clone()))
    }

    fn headers(
        &mut self,
        id: Resource<HostIncomingRequest>,
    ) -> wasmtime::Result<Resource<FieldMap>> {
        let req = self.table.get(&id)?;
        let mut headers = req.headers.clone();
        headers.set_immutable();
        Ok(self.table.push_child(headers, &id)?)
    }

    fn consume(
        &mut self,
        id: Resource<HostIncomingRequest>,
    ) -> wasmtime::Result<Result<Resource<HostIncomingBody>, ()>> {
        let req = self.table.get_mut(&id)?;
        match req.body.take() {
            Some(body) => {
                let id = self.table.push(body)?;
                Ok(Ok(id))
            }

            None => Ok(Err(())),
        }
    }

    fn drop(&mut self, id: Resource<HostIncomingRequest>) -> wasmtime::Result<()> {
        let _ = self.table.delete(id)?;
        Ok(())
    }
}

impl types::HostOutgoingRequest for WasiHttpCtxView<'_> {
    fn new(
        &mut self,
        headers: Resource<FieldMap>,
    ) -> wasmtime::Result<Resource<HostOutgoingRequest>> {
        let mut headers = self.table.delete(headers)?;
        headers.set_immutable();

        self.table
            .push(HostOutgoingRequest {
                path_with_query: None,
                authority: None,
                method: types::Method::Get,
                headers,
                scheme: None,
                body: None,
            })
            .context("[new_outgoing_request] pushing request")
    }

    fn body(
        &mut self,
        request: Resource<HostOutgoingRequest>,
    ) -> wasmtime::Result<Result<Resource<HostOutgoingBody>, ()>> {
        let req = self
            .table
            .get_mut(&request)
            .context("[outgoing_request_write] getting request")?;

        if req.body.is_some() {
            return Ok(Err(()));
        }

        let size = match get_content_length(&req.headers) {
            Ok(size) => size,
            Err(..) => return Ok(Err(())),
        };

        let (host_body, hyper_body) = HostOutgoingBody::new(StreamContext::Request, size);

        req.body = Some(hyper_body);

        // The output stream will necessarily outlive the request, because we could be still
        // writing to the stream after `outgoing-handler.handle` is called.
        let outgoing_body = self.table.push(host_body)?;

        Ok(Ok(outgoing_body))
    }

    fn drop(&mut self, request: Resource<HostOutgoingRequest>) -> wasmtime::Result<()> {
        let _ = self.table.delete(request)?;
        Ok(())
    }

    fn method(
        &mut self,
        request: wasmtime::component::Resource<types::OutgoingRequest>,
    ) -> wasmtime::Result<Method> {
        Ok(self.table.get(&request)?.method.clone())
    }

    fn set_method(
        &mut self,
        request: wasmtime::component::Resource<types::OutgoingRequest>,
        method: Method,
    ) -> wasmtime::Result<Result<(), ()>> {
        let req = self.table.get_mut(&request)?;

        if let Method::Other(s) = &method {
            if http::Method::from_str(s).is_err() {
                return Ok(Err(()));
            }
        }

        req.method = method;

        Ok(Ok(()))
    }

    fn path_with_query(
        &mut self,
        request: wasmtime::component::Resource<types::OutgoingRequest>,
    ) -> wasmtime::Result<Option<String>> {
        Ok(self.table.get(&request)?.path_with_query.clone())
    }

    fn set_path_with_query(
        &mut self,
        request: wasmtime::component::Resource<types::OutgoingRequest>,
        path_with_query: Option<String>,
    ) -> wasmtime::Result<Result<(), ()>> {
        let req = self.table.get_mut(&request)?;

        if let Some(s) = path_with_query.as_ref() {
            if http::uri::PathAndQuery::from_str(s).is_err() {
                return Ok(Err(()));
            }
        }

        req.path_with_query = path_with_query;

        Ok(Ok(()))
    }

    fn scheme(
        &mut self,
        request: wasmtime::component::Resource<types::OutgoingRequest>,
    ) -> wasmtime::Result<Option<Scheme>> {
        Ok(self.table.get(&request)?.scheme.clone())
    }

    fn set_scheme(
        &mut self,
        request: wasmtime::component::Resource<types::OutgoingRequest>,
        scheme: Option<Scheme>,
    ) -> wasmtime::Result<Result<(), ()>> {
        let req = self.table.get_mut(&request)?;

        if let Some(types::Scheme::Other(s)) = scheme.as_ref() {
            if http::uri::Scheme::from_str(s.as_str()).is_err() {
                return Ok(Err(()));
            }
        }

        req.scheme = scheme;

        Ok(Ok(()))
    }

    fn authority(
        &mut self,
        request: wasmtime::component::Resource<types::OutgoingRequest>,
    ) -> wasmtime::Result<Option<String>> {
        Ok(self.table.get(&request)?.authority.clone())
    }

    fn set_authority(
        &mut self,
        request: wasmtime::component::Resource<types::OutgoingRequest>,
        authority: Option<String>,
    ) -> wasmtime::Result<Result<(), ()>> {
        let req = self.table.get_mut(&request)?;

        if let Some(s) = authority.as_ref() {
            if http::uri::Authority::from_str(s.as_str()).is_err() {
                return Ok(Err(()));
            }
        }

        req.authority = authority;

        Ok(Ok(()))
    }

    fn headers(
        &mut self,
        request: wasmtime::component::Resource<types::OutgoingRequest>,
    ) -> wasmtime::Result<wasmtime::component::Resource<FieldMap>> {
        let req = self.table.get(&request)?;
        let mut headers = req.headers.clone();
        headers.set_immutable();
        let id = self.table.push_child(headers, &request)?;
        Ok(id)
    }
}

impl types::HostResponseOutparam for WasiHttpCtxView<'_> {
    fn drop(&mut self, id: Resource<HostResponseOutparam>) -> wasmtime::Result<()> {
        let _ = self.table.delete(id)?;
        Ok(())
    }
    fn set(
        &mut self,
        id: Resource<HostResponseOutparam>,
        resp: Result<Resource<HostOutgoingResponse>, types::ErrorCode>,
    ) -> wasmtime::Result<()> {
        self.table.get(&id)?;
        let val = match resp {
            Ok(resp) => Ok(self.table.delete(resp)?),
            Err(e) => Err(e.into()),
        };

        let resp = self.table.delete(id)?;
        resp.status.set();
        let _ = resp.send.send(val);
        Ok(())
    }

    fn send_informational(
        &mut self,
        _id: Resource<HostResponseOutparam>,
        _status: u16,
        _headers: Resource<FieldMap>,
    ) -> HttpResult<()> {
        Err(HttpError::trap(wasmtime::format_err!(
            "experimental informational-outbound-responses is disabled"
        )))
    }
}

impl types::HostIncomingResponse for WasiHttpCtxView<'_> {
    fn drop(&mut self, response: Resource<HostIncomingResponse>) -> wasmtime::Result<()> {
        let _ = self
            .table
            .delete(response)
            .context("[drop_incoming_response] deleting response")?;
        Ok(())
    }

    fn status(&mut self, response: Resource<HostIncomingResponse>) -> wasmtime::Result<StatusCode> {
        let r = self
            .table
            .get(&response)
            .context("[incoming_response_status] getting response")?;
        Ok(r.status)
    }

    fn headers(
        &mut self,
        response: Resource<HostIncomingResponse>,
    ) -> wasmtime::Result<Resource<FieldMap>> {
        let resp = self.table.get(&response)?;
        let mut headers = resp.headers.clone();
        headers.set_immutable();
        let id = self.table.push_child(headers, &response)?;
        Ok(id)
    }

    fn consume(
        &mut self,
        response: Resource<HostIncomingResponse>,
    ) -> wasmtime::Result<Result<Resource<HostIncomingBody>, ()>> {
        let r = self
            .table
            .get_mut(&response)
            .context("[incoming_response_consume] getting response")?;

        match r.body.take() {
            Some(body) => {
                let id = self.table.push(body)?;
                Ok(Ok(id))
            }

            None => Ok(Err(())),
        }
    }
}

impl types::HostFutureTrailers for WasiHttpCtxView<'_> {
    fn drop(&mut self, id: Resource<HostFutureTrailers>) -> wasmtime::Result<()> {
        let _ = self
            .table
            .delete(id)
            .context("[drop future-trailers] deleting future-trailers")?;
        Ok(())
    }

    fn subscribe(
        &mut self,
        index: Resource<HostFutureTrailers>,
    ) -> wasmtime::Result<Resource<DynPollable>> {
        crate::io::poll::subscribe(self.table, index)
    }

    fn get(
        &mut self,
        id: Resource<HostFutureTrailers>,
    ) -> wasmtime::Result<Option<Result<Result<Option<Resource<Trailers>>, types::ErrorCode>, ()>>>
    {
        let result = self.table.get_mut(&id)?.get();
        Ok(match result {
            None => None,
            Some(Err(())) => Some(Err(())),
            Some(Ok(Err(error))) => Some(Ok(Err(error.into()))),
            Some(Ok(Ok(fields))) => Some(Ok(Ok(fields.map(|f| self.table.push(f)).transpose()?))),
        })
    }
}

impl types::HostIncomingBody for WasiHttpCtxView<'_> {
    fn stream(
        &mut self,
        id: Resource<HostIncomingBody>,
    ) -> wasmtime::Result<Result<Resource<DynInputStream>, ()>> {
        let body = self.table.get_mut(&id)?;

        if let Some(stream) = body.take_stream() {
            let stream = self.table.push_child(stream, &id)?;
            return Ok(Ok(stream));
        }

        Ok(Err(()))
    }

    fn finish(
        &mut self,
        id: Resource<HostIncomingBody>,
    ) -> wasmtime::Result<Resource<HostFutureTrailers>> {
        let body = self.table.delete(id)?;
        let trailers = self.table.push(body.finish()?)?;
        Ok(trailers)
    }

    fn drop(&mut self, id: Resource<HostIncomingBody>) -> wasmtime::Result<()> {
        let _ = self.table.delete(id)?;
        Ok(())
    }
}

impl types::HostOutgoingResponse for WasiHttpCtxView<'_> {
    fn new(
        &mut self,
        headers: Resource<FieldMap>,
    ) -> wasmtime::Result<Resource<HostOutgoingResponse>> {
        let mut fields = self.table.delete(headers)?;
        fields.set_immutable();

        let id = self.table.push(HostOutgoingResponse {
            status: http::StatusCode::OK,
            headers: fields,
            body: None,
        })?;

        Ok(id)
    }

    fn body(
        &mut self,
        id: Resource<HostOutgoingResponse>,
    ) -> wasmtime::Result<Result<Resource<HostOutgoingBody>, ()>> {
        let resp = self.table.get_mut(&id)?;

        if resp.body.is_some() {
            return Ok(Err(()));
        }

        let size = match get_content_length(&resp.headers) {
            Ok(size) => size,
            Err(..) => return Ok(Err(())),
        };

        let (host, body) = HostOutgoingBody::new(StreamContext::Response, size);

        resp.body.replace(body);

        let id = self.table.push(host)?;

        Ok(Ok(id))
    }

    fn status_code(
        &mut self,
        id: Resource<HostOutgoingResponse>,
    ) -> wasmtime::Result<types::StatusCode> {
        Ok(self.table.get(&id)?.status.into())
    }

    fn set_status_code(
        &mut self,
        id: Resource<HostOutgoingResponse>,
        status: types::StatusCode,
    ) -> wasmtime::Result<Result<(), ()>> {
        let resp = self.table.get_mut(&id)?;

        match http::StatusCode::from_u16(status) {
            Ok(status) => resp.status = status,
            Err(_) => return Ok(Err(())),
        };

        Ok(Ok(()))
    }

    fn headers(
        &mut self,
        id: Resource<HostOutgoingResponse>,
    ) -> wasmtime::Result<Resource<FieldMap>> {
        let resp = self.table.get(&id)?;
        let mut headers = resp.headers.clone();
        headers.set_immutable();
        Ok(self.table.push_child(headers, &id)?)
    }

    fn drop(&mut self, id: Resource<HostOutgoingResponse>) -> wasmtime::Result<()> {
        let _ = self.table.delete(id)?;
        Ok(())
    }
}

impl types::HostFutureIncomingResponse for WasiHttpCtxView<'_> {
    fn drop(&mut self, id: Resource<HostFutureIncomingResponse>) -> wasmtime::Result<()> {
        let _ = self.table.delete(id)?;
        Ok(())
    }

    fn get(
        &mut self,
        id: Resource<HostFutureIncomingResponse>,
    ) -> wasmtime::Result<
        Option<Result<Result<Resource<HostIncomingResponse>, types::ErrorCode>, ()>>,
    > {
        let result = self.table.get_mut(&id)?.get();
        Ok(match result {
            None => None,
            Some(Err(())) => Some(Err(())),
            Some(Ok(Err(error))) => Some(Ok(Err(error.into()))),
            Some(Ok(Ok(response))) => Some(Ok(Ok(self.table.push(HostIncomingResponse {
                status: response.status,
                headers: response.headers,
                body: Some(response.body),
            })?))),
        })
    }

    fn subscribe(
        &mut self,
        id: Resource<HostFutureIncomingResponse>,
    ) -> wasmtime::Result<Resource<DynPollable>> {
        crate::io::poll::subscribe(self.table, id)
    }
}

impl types::HostOutgoingBody for WasiHttpCtxView<'_> {
    fn write(
        &mut self,
        id: Resource<HostOutgoingBody>,
    ) -> wasmtime::Result<Result<Resource<DynOutputStream>, ()>> {
        let body = self.table.get_mut(&id)?;
        if let Some(stream) = body.take_stream() {
            let id = self.table.push_child(stream, &id)?;
            Ok(Ok(id))
        } else {
            Ok(Err(()))
        }
    }

    fn finish(
        &mut self,
        id: Resource<HostOutgoingBody>,
        ts: Option<Resource<Trailers>>,
    ) -> HttpResult<()> {
        let body = self.table.delete(id)?;

        let ts = if let Some(ts) = ts {
            Some(self.table.delete(ts)?)
        } else {
            None
        };

        body.finish(ts)
            .map_err(HttpError::trap)?
            .map_err(|e| HttpError::from(types::ErrorCode::from(e)))?;
        Ok(())
    }

    fn drop(&mut self, id: Resource<HostOutgoingBody>) -> wasmtime::Result<()> {
        self.table.delete(id)?;
        Ok(())
    }
}

impl types::HostRequestOptions for WasiHttpCtxView<'_> {
    fn new(&mut self) -> wasmtime::Result<Resource<types::RequestOptions>> {
        let id = self.table.push(types::RequestOptions::default())?;
        Ok(id)
    }

    fn connect_timeout(
        &mut self,
        opts: Resource<types::RequestOptions>,
    ) -> wasmtime::Result<Option<types::Duration>> {
        let nanos = self.table.get(&opts)?.connect_timeout.map(|d| d.as_nanos());

        if let Some(nanos) = nanos {
            Ok(Some(nanos.try_into()?))
        } else {
            Ok(None)
        }
    }

    fn set_connect_timeout(
        &mut self,
        opts: Resource<types::RequestOptions>,
        duration: Option<types::Duration>,
    ) -> wasmtime::Result<Result<(), ()>> {
        self.table.get_mut(&opts)?.connect_timeout = duration.map(std::time::Duration::from_nanos);
        Ok(Ok(()))
    }

    fn first_byte_timeout(
        &mut self,
        opts: Resource<types::RequestOptions>,
    ) -> wasmtime::Result<Option<types::Duration>> {
        let nanos = self
            .table
            .get(&opts)?
            .first_byte_timeout
            .map(|d| d.as_nanos());

        if let Some(nanos) = nanos {
            Ok(Some(nanos.try_into()?))
        } else {
            Ok(None)
        }
    }

    fn set_first_byte_timeout(
        &mut self,
        opts: Resource<types::RequestOptions>,
        duration: Option<types::Duration>,
    ) -> wasmtime::Result<Result<(), ()>> {
        self.table.get_mut(&opts)?.first_byte_timeout =
            duration.map(std::time::Duration::from_nanos);
        Ok(Ok(()))
    }

    fn between_bytes_timeout(
        &mut self,
        opts: Resource<types::RequestOptions>,
    ) -> wasmtime::Result<Option<types::Duration>> {
        let nanos = self
            .table
            .get(&opts)?
            .between_bytes_timeout
            .map(|d| d.as_nanos());

        if let Some(nanos) = nanos {
            Ok(Some(nanos.try_into()?))
        } else {
            Ok(None)
        }
    }

    fn set_between_bytes_timeout(
        &mut self,
        opts: Resource<types::RequestOptions>,
        duration: Option<types::Duration>,
    ) -> wasmtime::Result<Result<(), ()>> {
        self.table.get_mut(&opts)?.between_bytes_timeout =
            duration.map(std::time::Duration::from_nanos);
        Ok(Ok(()))
    }

    fn drop(&mut self, rep: Resource<types::RequestOptions>) -> wasmtime::Result<()> {
        let _ = self.table.delete(rep)?;
        Ok(())
    }
}
