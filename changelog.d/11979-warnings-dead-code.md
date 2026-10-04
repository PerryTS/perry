Removed dead code that failed the `-D warnings` build: zlib's namespace
dispatch matched `createZstdCompress` and `createZstdDecompress` twice, and two
perry-codegen test helpers had no remaining use.
