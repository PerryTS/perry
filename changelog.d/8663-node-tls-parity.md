Completed `node:tls` compatibility across the full node-suite inventory. TLS
servers and sockets now support real loopback handshakes, ALPN and SNI
selection, certificate and secure-context rotation, custom trust stores,
client certificates, identity callbacks, negotiated state, orderly shutdown,
and Node-compatible validation and error shapes in both bundled and optimized
external-net builds. The current TLS inventory passes 100/100 fixtures.
