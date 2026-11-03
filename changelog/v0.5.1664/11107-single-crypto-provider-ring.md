**Binaries ship one rustls crypto provider (ring) instead of two.** reqwest, mongodb, turnloop-tls and perry-ext-http all require ring; aws-lc-rs (+ the aws-lc-sys C/asm build) arrived only through the *default features* of the workspace `rustls`/`tokio-rustls` deps. Those are now `default-features = false` with `ring`, the 14 `crypto::aws_lc_rs::default_provider()` call sites in perry-ext-net and perry-stdlib (net, tls, tls_verifier, ws) use `crypto::ring`, and `deny.toml` bans `aws-lc-rs`/`aws-lc-sys` so a bare `rustls = "0.23"` cannot bring it back.

Measured on a standalone rustls 0.23.45 probe (arm64 macOS, stripped, thin LTO): **−1.3 MB binary (2.7 → 1.4 MB), −1 MB peak RSS**; TLS 1.3 handshakes 1–9 % faster than the aws-lc + X25519MLKEM768 path previously negotiated; bulk AES-256-GCM ~15 % slower (2.25 → 1.9 GB/s single core).

Also removes a call-order dependence: perry-ext-http installed ring as the process-default `CryptoProvider` while stdlib net/tls/ws installed aws-lc, so `ServerConfig::builder()` and `CryptoProvider::get_default()` resolved to whichever subsystem ran first.

Trade-offs: no post-quantum key exchange (falls back to X25519) and no P-521 ECDSA certificates in TLS; `node:crypto`/WebCrypto P-521 is unaffected (it does not go through rustls).
