//! WASI I/O resources with fallible native readiness. A driver failure is a
//! host trap, never a successful timer expiry or stream-readiness notification.
pub mod bindings;
mod impls;
pub mod poll;
mod sync_poll;
mod sync_streams;

pub mod sync {
    wasmtime::component::bindgen!({
        path: "wit",
        world: "wasi:io/imports",
        require_store_data_send: true,
        imports: { default: trappable },
        with: {
            "wasi:io/error": crate::io::bindings::wasi::io::error,
            "wasi:io/poll.pollable": crate::io::poll::DynPollable,
            "wasi:io/streams.input-stream": wasmtime_wasi_io::streams::DynInputStream,
            "wasi:io/streams.output-stream": wasmtime_wasi_io::streams::DynOutputStream,
        },
        trappable_error_type: {
            "wasi:io/streams.stream-error" => wasmtime_wasi_io::streams::StreamError,
        },
    });
}
