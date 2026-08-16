### Testing

- `tests/test_next_app_route_dylib.sh` (the #8161 Next App Route dylib gate) no
  longer aborts on its first request (#8205). The Rust `provider-host.rs` exported
  rustc's System-allocator shim, and `provider-linker.sh` linked the stdlib
  provider image with `-flat_namespace`, so the image's `__rust_dealloc` import
  bound to the host's shim while the runtime image's shim is mimalloc — the first
  cross-image `Vec<i64>` drop in `js_node_http_server_process_pending` freed a
  mimalloc pointer with libsystem `free()`. The host is now C
  (`tests/fixtures/next-app-route/provider-host.c`; same load order, flags, probe
  check and event loop, no Rust shim in the executable) and the stdlib image is
  linked two-level so its runtime imports bind to `libperry_runtime.dylib`. The
  gate's ABI check, forbidden-diagnostic grep and bypass guard are unchanged; the
  100-batch loop can still surface #8163's default-GC `TypeError` until that
  lands.
