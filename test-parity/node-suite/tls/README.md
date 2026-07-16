# `node:tls` granular parity coverage

This directory contains independent print-and-diff fixtures for the TLS API. The
Node oracle is Node.js 26.5.0. Network cases use only
`127.0.0.1`, bind port `0`, and use the fixed non-secret certificate material in
`fixtures/`. The localhost and SNI certificates share the committed test key and
cover only `localhost`/`127.0.0.1` and `api.local`; no case consults internet
endpoints or host trust stores.

## Upstream audit

The expansion was checked against these primary sources:

- Node.js 26.5.0 (`bebd1b8d92bf4cc917844d6335ed1ecf9c2a75fb`): the 241 TLS-named cases under [`test/parallel`](https://github.com/nodejs/node/tree/v26.5.0/test/parallel) and [`test/sequential`](https://github.com/nodejs/node/tree/v26.5.0/test/sequential), especially `test-tls-add-context.js`, `test-tls-alpn-server-client.js`, `test-tls-basic-validations.js`, `test-tls-ca-concat.js`, `test-tls-certificate-compression.js`, `test-tls-check-server-identity.js`, `test-tls-client-auth.js`, `test-tls-client-default-ciphers.js`, `test-tls-client-getephemeralkeyinfo.js`, `test-tls-connect-given-socket.js`, `test-tls-connect-simple.js`, `test-tls-connect-secure-context.js`, `test-tls-exportkeyingmaterial.js`, `test-tls-finished.js`, `test-tls-get-ca-certificates-default.js`, `test-tls-get-ca-certificates-error.js`, `test-tls-getcertificate-x509.js`, `test-tls-getcipher.js`, `test-tls-getprotocol.js`, `test-tls-ip-servername-forbidden.js`, `test-tls-multiple-cas-as-string.js`, `test-tls-no-cert-required.js`, `test-tls-options-boolean-check.js`, `test-tls-peer-certificate.js`, `test-tls-root-certificates.js`, `test-tls-secure-context-usage-order.js`, `test-tls-server-connection-server.js`, `test-tls-server-parent-constructor-options.js`, `test-tls-server-setkeycert.js`, `test-tls-set-default-ca-certificates-array-buffer.js`, `test-tls-set-default-ca-certificates-basic.js`, `test-tls-set-default-ca-certificates-error.js`, `test-tls-set-default-ca-certificates-mixed-types.js`, `test-tls-set-default-ca-certificates-recovery.js`, `test-tls-set-secure-context.js`, `test-tls-signal.js`, `test-tls-sni-case-insensitive.js`, `test-tls-sni-server-client.js`, `test-tls-snicallback-error.js`, `test-tls-socket-allow-half-open-option.js`, `test-tls-socket-constructor-alpn-options-parsing.js`, `test-tls-secure-session.js`, `test-tls-set-sigalgs.js`, and `test-tls-ticket-invalid-arg.js`.
- Deno `e6a0143641690dfb2723bab3ce97c394d5ea3ec3`: the current Node compatibility selections in [`tests/node_compat/config.jsonc`](https://github.com/denoland/deno/blob/e6a0143641690dfb2723bab3ce97c394d5ea3ec3/tests/node_compat/config.jsonc) and the implementation in [`ext/node/polyfills/tls_esm.ts`](https://github.com/denoland/deno/blob/e6a0143641690dfb2723bab3ce97c394d5ea3ec3/ext/node/polyfills/tls_esm.ts).
- Bun `4bbe0751a2e5436757768325c2cc7ed97dc8767c`: its current dedicated [`test/js/node/tls`](https://github.com/oven-sh/bun/tree/4bbe0751a2e5436757768325c2cc7ed97dc8767c/test/js/node/tls) cases and selected Node TLS tests under [`test/js/node/test/parallel`](https://github.com/oven-sh/bun/tree/4bbe0751a2e5436757768325c2cc7ed97dc8767c/test/js/node/test/parallel).

The local cases reduce those larger suites to deterministic semantic probes for
exports/classes/constants, ALPN wire conversion, root/default CA identity,
validation, mutation, connection application and explicit-CA precedence,
secure-context scalar/container material and algorithm validation, certificate
compression surface/validation, certificate and hostname identity including
custom acceptance/rejection callbacks, positional/context-backed/preconnected
loopback handshakes, required and optional client certificates, concatenated CA
inputs, missing-certificate and plaintext rejection, event cleanup, abort
signals, socket getters, mutable protocol/cipher defaults, `setKeyCert`, live
server-context rotation, shared signature algorithms and certificate views,
finished-message/exporter material, ALPN callback/protocol/key selection,
asynchronous and null-context SNI selection, case-insensitive wildcard SNI and
latest-`addContext` selection, TLS 1.2 session resumption, ticket keys, and
fragment-size controls.

## Build-mode diagnostic

A direct `node:tls` import currently exposes two separate build paths:

1. Auto-optimization attempts to rebuild `perry-stdlib` and fails at
   `crates/perry-stdlib/src/tls.rs` because `rustls::crypto::ring` is referenced
   while rustls's `ring` feature is not activated.
2. Without matched static fallback archives, compilation then reports missing
   `libperry_runtime.a`. With `PERRY_RUNTIME_DIR=target/perry-dev` after building
   `perry-runtime-static` and `perry-stdlib-static`, fixtures reach the behavioral
   lane. Some socket cases then expose a separate link failure for the undefined
   `_js_net_socket_once` symbol. Mixed `node:net` plus `node:tls` plaintext and
   preconnected-socket cases expose another fallback-composition failure where
   the TLS server symbols are absent from the selected archives.

These are feature/archive composition diagnostics, not changes made by this
suite. The fixtures intentionally remain present so later runtime/build work can
turn each category into behavioral results without rewriting coverage.

## Stopping boundary

The remaining upstream TLS cases were not copied when they depend on one or more
of the following:

- renegotiation, tracing/keylog output, PSK, OCSP, FIPS, engines/providers,
  OpenSSL/rustls-specific cipher/security-level behavior, or legacy protocol
  availability;
- system/extra CA environment flags, expired certificates, platform cipher
  inventories, native certificate dumps, or absolute fixture paths;
- internet DNS/endpoints, named pipes, fixed ports, process CLI flags, cluster or
  worker orchestration;
- large transfers, memory/GC pressure, concurrent stress, fault injection,
  socket/kernel races, handshake timeouts, or session-ticket timing;
- certificate-compression wire ratios, decompression limits, proxy mutation, or
  compression-bomb behavior, which require backend-aware transport harnesses;
- raw error strings, stack traces, cipher ordering, ephemeral port values, and
  other backend- or environment-specific output.

Those categories need separate targeted runtime/build work or a purpose-built
harness. Within the portable boundary, further upstream cases are duplicates of
the semantic contracts already represented here.
