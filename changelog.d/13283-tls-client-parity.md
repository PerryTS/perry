Fix `tls.TLSSocket.authorizationError` to report `null` after a successful authorized handshake, matching Node.js.

Preserve the Node.js client error code when TLS peers have no common ALPN protocol, without changing handshake rejection.

Fix TLS server identity checks for equivalent compressed and expanded IPv6 addresses, preserving IP-only subject alternative name matching and existing DNS validation.

Keep the IPv6 identity regression test valid in debug builds by extracting NaN-boxed string keys through the string-aware runtime helper.
