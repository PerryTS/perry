// Adapted from Wasmtime 48.0.5 (Bytecode Alliance contributors).
// SPDX-License-Identifier: Apache-2.0 WITH LLVM-exception
// See PROVENANCE.md and LICENSE.

wasmtime::component::bindgen!({
    path: "wit",
    world: "wasi:io/imports",
    with: {
        "wasi:io/poll.pollable": crate::io::poll::DynPollable,
        "wasi:io/streams.input-stream": wasmtime_wasi_io::streams::DynInputStream,
        "wasi:io/streams.output-stream": wasmtime_wasi_io::streams::DynOutputStream,
        "wasi:io/error.error": wasmtime_wasi_io::streams::Error,
    },
    imports: {
        "wasi:io/poll.poll": async | trappable | tracing,
        "wasi:io/poll.[method]pollable.block": async | trappable | tracing,
        "wasi:io/poll.[method]pollable.ready": async | trappable | tracing,
        "wasi:io/streams.[method]input-stream.blocking-read": async | trappable | tracing,
        "wasi:io/streams.[method]input-stream.blocking-skip": async | trappable | tracing,
        "wasi:io/streams.[drop]input-stream": async | trappable | tracing,
        "wasi:io/streams.[method]output-stream.blocking-splice": async | trappable | tracing,
        "wasi:io/streams.[method]output-stream.blocking-flush": async | trappable | tracing,
        "wasi:io/streams.[method]output-stream.blocking-write-and-flush": async | trappable | tracing,
        "wasi:io/streams.[method]output-stream.blocking-write-zeroes-and-flush": async | trappable | tracing,
        "wasi:io/streams.[drop]output-stream": async | trappable,
        default: trappable | tracing,
    },
    trappable_error_type: {
        "wasi:io/streams.stream-error" => wasmtime_wasi_io::streams::StreamError,
    }
});
