//! Pinned WASI 0.3 service imports and asynchronous handler exports.
wasmtime::component::bindgen!({
    path: "wit-p3",
    world: "wasi:http/service",
    require_store_data_send: true,
    imports: {
        "wasi:http/client.send": store | trappable,
        "wasi:http/types.[drop]request": store | trappable,
        "wasi:http/types.[drop]response": store | trappable,
        "wasi:http/types.[static]request.consume-body": store | trappable,
        "wasi:http/types.[static]request.new": store | trappable,
        "wasi:http/types.[static]response.consume-body": store | trappable,
        "wasi:http/types.[static]response.new": store | trappable,
        default: trappable,
    },
    exports: { default: async | store },
    with: {
        "wasi:cli": crate::p3::bindings::cli,
        "wasi:clocks": crate::p3::bindings::clocks,
        "wasi:random": crate::p3::bindings::random,
        "wasi:http/types.fields": crate::http::FieldMap,
        "wasi:http/types.request": crate::p3::http::Request,
        "wasi:http/types.response": crate::p3::http::Response,
        "wasi:http/types.request-options": crate::p3::http::RequestOptions,
    },
    trappable_error_type: {
        "wasi:http/types.error-code" => crate::p3::http::HttpError,
        "wasi:http/types.header-error" => crate::p3::http::HeaderError,
        "wasi:http/types.request-options-error" => crate::p3::http::RequestOptionsError,
    },
});
pub use wasi::*;
