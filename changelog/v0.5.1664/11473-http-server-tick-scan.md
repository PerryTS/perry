**perf(http): the `node:http` server pump no longer walks every live handle on every tick (#11473).** `js_node_http_server_has_active` and `js_node_http_server_process_pending` found the server handles by walking the whole perry-ffi handle registry four times per event-loop tick. #11328 added one `req.socket` handle per keep-alive connection, which made that walk O(open connections). That was the −9% req/s at c=256 in the tokio-removal report (#11451): on the M1 mini, #11451's binaries give 108.4k vs 100.4k req/s at c=256, with +7% user-space instructions per request and equal kernel time.

The new `perry_ffi::index_handle_type::<T>()` keeps a side list of a type's live ids. It is maintained by `register_handle` and `remove_payload` while the id is still pending or retiring, and it backfills handles registered before the declaration. `iter_handles_of`, `iter_handles_of_mut` and `iter_handle_ids_of` use the list for indexed types. perry-ext-http indexes `HttpServer`, `HttpsServer` and `Http2SecureServer`.

Retired instructions per request on Linux (`perf stat`, 5 interleaved rounds, main `b6a85cdad` → this change):

| c | before | after |
|---:|---:|---:|
| 1 | 184,233 | 92,041 (−50.0%) |
| 64 | 73,805 | 71,019 (−3.8%) |
| 256 | 75,687 | 71,317 (−5.8%) |
| 1024 | 85,492 | 72,311 (−15.4%) |

Instructions per request are now flat in the connection count. Registrations of non-indexed types pay one `TypeId` compare per declared type.
