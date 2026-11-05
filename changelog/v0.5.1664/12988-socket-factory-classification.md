### Fixed

- Preserve Socket classification for late-assigned `net.connect()`, `net.createConnection()`, and `tls.connect()` results, including ordinary own getter/data-property reads. Register the classification before lowering right-hand-side callbacks; declarations reuse the same classifier, and non-Socket fallbacks remain unchanged.
- Add focused public NET/TLS regression coverage for factory-result classification and property access.
