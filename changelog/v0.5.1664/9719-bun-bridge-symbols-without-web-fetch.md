**`node:http` links again from a cold auto-optimize cache.** Any program
importing `node:http` failed to link with
`Undefined symbols: _js_bun_http_response_snapshot_json` unless a warm
`target/perry-auto-*` cache happened to hide it.

`perry-ext-http`'s `server/bun_server.rs` declares that symbol and
`js_bun_http_request_from_json` in an unconditional `extern "C"` block — only
its `#[cfg(test)]` arm stubs them — but their definitions live in
`fetch::bun_server_bridge`, which `perry-stdlib` compiles only under
`web-fetch`. The auto-optimize feature set for a `node:http` program
(`async-runtime`, `external-http-{client,server}-pump`, `external-net-tls`,
`external-tls-server`, `external-ws-pump`) does not include `web-fetch`, so
definition and reference disagreed about the feature gating them.

The definitions genuinely need Fetch machinery (`FETCH_RESPONSES`,
`consume_response_body`), so they cannot move out of the gated module; and
making the http features depend on `web-fetch` would link `reqwest` — an HTTP
*client* — into every `node:http` *server* build. Instead `perry-stdlib` now
defines both symbols unconditionally: the real bridge under `web-fetch`, and
under `cfg(not(web-fetch))` a pair that answers "nothing to bridge" (`null` /
`undefined`). That is the correct answer rather than a placeholder — with no
`fetch` module there are no `Response` objects to snapshot, and it matches what
the real bridge already returns for an unknown handle.

CI could not see this: plain `cargo build` / `cargo test` never link the
auto-optimize ext-http path. It surfaced only in a full gap sweep run from a
cold cache, where it accounted for eight `compile_fail` results
(`test_gap_3527_http_ctor_prototype`,
`test_gap_gc_http2_pending_event_callback_rooting`,
`test_gap_http_client_no_redirect_follow`, `test_gap_http_overloads_3226plus`,
`test_gap_http_req_async_iterator`,
`test_gap_http_res_socket_writable_onfinished`, `test_gap_http2_settings`,
`test_gap_regex_replace_dyn_regex_with_http`). All eight compile, link and pass
against the pinned node oracle with this change; those eight are the
regression coverage, since they exercise the real cold link that no unit test
reaches.
