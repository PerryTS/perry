### Fixed

- Enforce `zlib.crc32` data types: string, Buffer, TypedArray, or DataView; reject bare ArrayBuffer.
- Validate the optional seed as uint32, with canonical errors checked data-first.
- Root both arguments and validate before copying input bytes.
- Preserve ArrayBuffer support in `gzipSync`.
