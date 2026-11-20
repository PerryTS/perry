**HTTP/2 custom SETTINGS decoding matches Node.js.**

`http2.getUnpackedSettings()` now preserves unknown SETTINGS identifiers in
`customSettings` instead of discarding them. Repeated identifiers keep their
last value, and known settings retain their existing decoding behavior.
