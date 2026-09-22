### Fixed

- Global `fetch` now honors `redirect: "follow"`, `"manual"`, and `"error"`.
  Followed responses expose their final URL and set `response.redirected`; manual
  mode returns the redirect response with its `Location` header; error mode
  rejects with a `TypeError`. `fetch(Request)` also inherits the Request's
  redirect mode.
