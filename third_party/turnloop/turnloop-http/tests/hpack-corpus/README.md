# HPACK interoperability corpus

Source: https://github.com/http2jp/hpack-test-case/tree/8a1406e7d14bfcb6c046021f13cc15cfb162726d

Vendored 72 complete stories / 3754 cases from nghttp2 (all stories), go-hpack and python-hpack (stories 00–19). MIT license included. No implementation code is vendored. The JSON wire bytes and ordered header lists were losslessly converted to gzip-compressed text: S starts a story/reset, C is hex wire, H is hex name and value, E ends a case. SHA256SUMS records the original JSON; every fetched file was verified against the Git blob SHA-1 in the commit tree. Compression mtime is zero.
