### Fixed

- Fixed HTTP upgrade requests that use a custom `createConnection` socket.
  Perry now preserves `Connection: Upgrade`, emits the request's `upgrade`
  event for a `101` response, and hands the same live socket back to callers.
  This allows source-compiled `ws` clients to complete their handshake instead
  of receiving `Unexpected server response: 426` (#10888).
