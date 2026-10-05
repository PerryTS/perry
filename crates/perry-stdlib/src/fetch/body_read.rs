//! Response whole-body methods share the same asynchronous stream consumer.
use super::*;

pub(super) enum Kind {
    Text,
    Json,
    ArrayBuffer,
    Bytes,
    Blob,
    FormData,
}

pub(super) unsafe fn read(handle: f64, kind: Kind) -> *mut perry_runtime::Promise {
    let _roots = lifecycle::pin_handles(&[handle]);
    let promise = perry_runtime::js_promise_new_cross_thread();
    let id = handle_id(handle);
    let state = FETCH_RESPONSES
        .lock()
        .unwrap()
        .get(&id)
        .map(|r| (r.body_present, r.body_used, r.body_stream_id));
    let Some((present, used, stream)) = state else {
        reject_fetch_type_error(promise, "Invalid response handle");
        return promise;
    };
    if used
        || (present
            && stream.is_some_and(|id| {
                let (locked, disturbed) = crate::streams::readable_body_state(id);
                locked || disturbed
            }))
    {
        reject_fetch_type_error(promise, BODY_ALREADY_USED_MESSAGE);
        return promise;
    }
    let metadata = if matches!(kind, Kind::Blob | Kind::FormData) {
        body_metadata::response_content_type(handle)
    } else {
        String::new()
    };
    let finish = match kind {
        Kind::Text => text,
        Kind::Json => json,
        Kind::ArrayBuffer => array_buffer,
        Kind::Bytes => bytes,
        Kind::Blob => blob,
        Kind::FormData => form_data,
    };
    if present {
        if let Some(stream) = stream {
            FETCH_RESPONSES
                .lock()
                .unwrap()
                .get_mut(&id)
                .unwrap()
                .body_used = true;
            crate::streams::collect_body(stream, promise, metadata, finish);
            return promise;
        }
    }
    let body = consume_response_body(handle).unwrap_or_default();
    match finish(body, metadata) {
        Ok(value) => perry_runtime::js_promise_resolve(promise, value),
        Err(error) => perry_runtime::js_promise_reject(promise, error),
    }
    promise
}

fn text(body: Vec<u8>, _: String) -> Result<f64, f64> {
    let text = String::from_utf8_lossy(&body);
    let text = text.strip_prefix('\u{feff}').unwrap_or(&text);
    let ptr = js_string_from_bytes(text.as_ptr(), text.len() as u32);
    Ok(f64::from_bits(JSValue::string_ptr(ptr).bits()))
}
fn json(body: Vec<u8>, _: String) -> Result<f64, f64> {
    unsafe { parse_json_body(&body).map(|v| f64::from_bits(v.bits())) }
}
fn array_buffer(body: Vec<u8>, _: String) -> Result<f64, f64> {
    unsafe {
        let buf = perry_runtime::buffer::buffer_alloc(body.len() as u32);
        (*buf).length = body.len() as u32;
        std::ptr::copy_nonoverlapping(
            body.as_ptr(),
            perry_runtime::buffer::buffer_data_mut(buf),
            body.len(),
        );
        Ok(f64::from_bits(JSValue::object_ptr(buf as *mut u8).bits()))
    }
}
fn bytes(body: Vec<u8>, _: String) -> Result<f64, f64> {
    unsafe {
        Ok(f64::from_bits(crate::streams::alloc_uint8array_from_bytes(
            &body,
        )))
    }
}
fn blob(body: Vec<u8>, content_type: String) -> Result<f64, f64> {
    Ok(handle_to_f64(alloc_blob(BlobData::blob(
        body,
        content_type,
    ))))
}

fn form_data(body: Vec<u8>, content_type: String) -> Result<f64, f64> {
    body_metadata::form_data_from_body(&body, &content_type)
        .map(|form| handle_to_f64(body_metadata::alloc_form_data(form)))
        .map_err(|message| unsafe { f64::from_bits(fetch_type_error_bits(message)) })
}
