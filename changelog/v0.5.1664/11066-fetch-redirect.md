### Fixed

- Global `fetch` now honors `redirect: "follow"`, `"manual"`, and `"error"`.
  Followed responses expose their final URL and set `response.redirected`; manual
  mode returns the redirect response with its `Location` header; error mode
  fails the request. `fetch(Request)` also inherits the Request's redirect mode,
  and a mode that is not follow/error/manual rejects with a `TypeError`.

  `RequestInit.redirect` is carried from HIR (`Expr::FetchWithOptions`) to the
  transport through the runtime's pending-option stash — the same trick
  `init.signal` uses — so `js_fetch_with_options`' 4-argument ABI is unchanged.

  Ported from #11066, whose original home (`crates/perry-ext-fetch/`) was
  removed by the turnloop migration (#10354); the fix now lives in
  `crates/perry-stdlib/src/fetch/`. The transport is the turnloop client engine
  (`fetch/turnloop_bridge.rs`), which hardcoded `RedirectMode::Follow`. It
  already implements all three modes and already reports `final_url` /
  `redirected`, so `FetchDispatch.redirect` plus
  `turnloop_bridge::engine_redirect()` is the whole transport-side change.
