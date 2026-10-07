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
pub(super) fn body_bytes(body: Vec<u8>, brand: perry_runtime::buffer::bytes::Brand) -> f64 {
    let len = body.len();
    let (value, pin) = perry_runtime::buffer::bytes::new_bytes(
        brand,
        len,
        perry_runtime::buffer::bytes::Init::AdoptVec(body),
    );
    drop(pin);
    value
}
fn array_buffer(body: Vec<u8>, _: String) -> Result<f64, f64> {
    Ok(body_bytes(
        body,
        perry_runtime::buffer::bytes::Brand::ArrayBuffer,
    ))
}
fn bytes(body: Vec<u8>, _: String) -> Result<f64, f64> {
    Ok(body_bytes(
        body,
        perry_runtime::buffer::bytes::Brand::Uint8Array,
    ))
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

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn array_buffer_adopts_the_body_allocation() {
        let mut body = Vec::with_capacity(1024 * 1024);
        body.resize(1024 * 1024, 0);
        body[..3].copy_from_slice(&[37, 91, 7]);
        let original = body.as_ptr();
        let value = array_buffer(body, String::new()).unwrap();
        let pin = perry_runtime::buffer::bytes::pin(value).unwrap();
        assert_eq!(pin.as_ptr(), original, "Response body must be adopted");
        assert_eq!(pin.len(), 1024 * 1024);
        let ptr = JSValue::from_bits(value.to_bits()).as_pointer::<u8>();
        assert!(perry_runtime::buffer::is_array_buffer(ptr as usize));
        perry_runtime::gc::js_gc_collect();
        assert_eq!(unsafe { *pin.as_ptr().add(1) }, 91);
    }
}
