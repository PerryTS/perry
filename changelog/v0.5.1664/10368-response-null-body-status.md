Fixed `Response.json(value, init)` skipping the `ResponseInit` validation that
`new Response(body, init)` applies (#10360). Both now share one check, in
Node's order: status range (`RangeError`), then `statusText` (`TypeError`), then
the body/null-body-status conflict. So `Response.json({a: 1}, {status: 204})`
throws Node's `TypeError: Response constructor: Invalid response status code
204` instead of returning a 204, and `Response.json({}, {status: 600})` throws
a `RangeError` instead of returning a 600. The fix covers both perry-stdlib and
perry-ext-fetch.

Programs compiled with `--platform bun` follow Bun instead: a body with a
null-body status (204/205/304) is accepted by both constructors, so
`new Response("", {status: 204})` works. The compiler seeds
`__perry_runtime.setBunPlatform()` into every module's init next to the #9599
`globalThis.Bun` install, which sets a runtime flag
(`perry-runtime/src/bun_compat/platform.rs`, `js_set_bun_platform` /
`js_bun_platform_enabled`) before any dependency's top-level code runs.

Tests: `test-files/test_gap_response_null_body_status_10360.ts` (Node parity)
and `crates/perry/tests/issue_10360_bun_platform_response_null_body.rs` (Bun
1.3.14 output under `--platform bun`, plus a node-platform control).
