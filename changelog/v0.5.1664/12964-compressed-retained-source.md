Store permanent function reflection source in independent 4 KiB raw-DEFLATE blocks. Each function info directly owns its first block, decoded offset and exact byte length; `Function.prototype.toString` decodes into a transient result without retaining decoded blocks or adding a lookup index. Legacy class, raw-method and unloadable-image source ownership remains unchanged. Common function infos keep their existing size; name and constructor-tail readers also handle the optional versioned-clone source layout.

Add exact nested, Unicode, class, arrow, async and generator reflection coverage, plus an opt-in `PERRY_SOURCE_READ_COUNT=1` runtime census. The pure Rust decoder reuses miniz_oxide already present in the workspace and introduces no native codec dependency.

The opt-in source-read census also reports before an explicit `process.exit()`, which intentionally skips ordinary cleanup handlers.

Stop the final frame at the requested source range end, avoiding unused suffix decoding while preserving exact property-key coercion. Cover partial backreferences, frame-end ranges, implicit function property keys and Unicode class identifiers.

A function whose own source is at most 512 bytes keeps a raw copy that its info names directly, so short functions used as property keys or tested by shape cost no decoding. Longer sources stay in the frames.
