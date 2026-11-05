WASI 0.2: `node:dgram` works under `--target wasi`. A program that imported it
used to fail to link (`undefined symbol: js_dgram_create_socket`), because UDP
rode turnloop, which wasm32 does not build. The JS-facing dgram layer is now the
same on every target; on WASI the socket is a non-blocking wasi:sockets UDP
socket polled by the event-loop pump, as WASI TCP is (bounded 1 ms park while a
socket is open). `reuseAddr` is accepted and has no effect there (wasi:sockets
has no SO_REUSEADDR), `ipv6Only: true` fails the bind with ENOTSUP, and the
multicast membership/interface setters throw ENOTSUP. Run with
`wasmtime run -S inherit-network=y`. New acceptance case
`scripts/wasi_acceptance/dgram`: two loopback sockets echo 1000 datagrams.
