Update AES-ECB support to `ecb` 0.2 and its current RustCrypto cipher traits.
ECB now shares the AES 0.9 implementation already used by CBC while the GCM,
CTR, and key-wrap paths retain their existing AES dependency generation.
