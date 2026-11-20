- **fetch: `perry-stdlib` no longer depends on `reqwest` (tokio removal, group G).** The global `fetch()` and the `js_fetch_stream_start` SSE surface now run only on the turnloop client engine. The reqwest future that used to run when the engine declined a request is gone, and so is the `perry-stdlib → reqwest` edge (`scripts/tokio_inventory.json`: 17 → 16 edges). A request the engine refuses now rejects with Node 26.5.1's error for the same input, and `test_gap_fetch_refused_requests.ts` pins each one. Before this change, embedded URL credentials and `CONNECT`/`TRACE`/`TRACK` requests were actually *sent*.

  | input | rejection |
  |---|---|
  | `ftp://…` | `fetch failed`, cause `unknown scheme` |
  | `file://…` | `fetch failed`, cause `not implemented... yet...` |
  | a URL with embedded credentials | `TypeError: Request cannot be constructed from a URL that includes credentials: …` |
  | an unparseable URL | `TypeError: Failed to parse URL from …` (cause `Invalid URL`, `ERR_INVALID_URL`) |
  | a forbidden method (`CONNECT`/`TRACE`/`TRACK`) | `TypeError: '<method>' HTTP method is unsupported.` |
  | a malformed method | `TypeError: '<method>' is not a valid HTTP method.` |

  One capability is dropped: reqwest could speak TLS to an **`https://` proxy**. Such a proxy now rejects with `only HTTP proxies are supported` (`UND_ERR_NOT_SUPPORTED`). Carrying it needs `turnloop_http::client::ProxyEnvironment::proxy_for` to accept `https` proxies, plus TLS-in-TLS in the engine. `http://` proxies are unaffected.
  - `js_fetch_set_global_proxy` keeps reqwest's acceptance rules, and now stores the normalized URI, so a schemeless `host:port` proxy reaches the engine as `http://host:port`.
  - `fetch/abort_bridge.rs` drops its `tokio::sync::Notify` registry, because the engine owns cancellation.
