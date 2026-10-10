### Use CPU feature dispatch for wide SHA-2 compression

SHA-384/512 Hash and HMAC now share one fixed OpenSSL SHA-512 context, using the bundled provider’s CPU dispatch. This removes the Intel-only AVX restriction inherited from ring on AMD hosts, without a second API, runtime cache or package rule. SHA-1/256 keep their existing SHA-NI implementations.

Partial-block copies and long-key HMACs have independent backend and Node parity coverage. The unpackcpu benchmark includes a CPU-qualified instruction-budget witness and records its baseline negative control.
