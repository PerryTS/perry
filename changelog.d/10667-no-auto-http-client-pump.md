Fixed `node:http`/`node:https` client dynamic dispatch (`res.pipe()`, `req.setHeader()`, `req.setTimeout()`, and
the rest of the client `IncomingMessage`/`ClientRequest` fallback surface) being silently absent under
`PERRY_NO_AUTO_OPTIMIZE=1`: the prebuilt stdlib archive is built with the default `full` feature set, which
deliberately excludes `external-http-client-pump` (folding it into `full` would force every no-auto program to
carry `libperry_ext_http.a`). When the program imports `http`/`https`, the no-auto path now rebuilds
`perry-stdlib-static` with that feature on top of `full`, in the same cargo invocation as `perry-ext-http` itself
(two archives built in separate invocations can carry different tokio compilations even off an identical
`Cargo.lock`, which the existing link-time guard in `runtime_compat.rs` refuses to link). Mirrors the on-demand
`wasm-host` rebuild `build_optional_runtime` already does for `WebAssembly.*` support (#10466).
