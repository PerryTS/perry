Fixed optimized `node:net`, `node:http`, `node:https`, and `node:http2` builds
after the TLS parity split. These external wrappers now retain Perry's shared
TLS server and SNI/ALPN preflight provider without also linking the bundled
network implementation, eliminating the `js_tls_client_preflight` undefined
symbol that blocked HTTP gap and GC-stress fixtures at compile time. External
`tls.connect` overload arguments also remain rooted when a user-replaced
`createSecureContext` callback triggers a moving collection.
