Stop linking the Brotli encoder into every program that uses `fetch`.

`web-fetch` implies `bundled-streams` (for `response.body`), and `bundled-streams` carried the `brotli` crate for the WHATWG `CompressionStream` / `DecompressionStream` codec. That codec dispatches every format through one runtime `match`, and a runtime-selected arm cannot be dead-stripped while its function is reachable — so the whole Brotli encoder (~820 KB of surviving symbols, measured from the linker map) was live in fetch programs that never compress anything.

The codec is now its own feature, `streams-brotli = ["bundled-streams", "dep:brotli"]`, and `bundled-streams` carries only `flate2`. `full` includes `streams-brotli`, so full builds are unchanged.

Auto-optimize adds `streams-brotli` when the HIR references `CompressionStream` or `DecompressionStream`, or when the program has deferred dynamic code. It deliberately does NOT key on the literal `"brotli"` the way the `node:zlib` codec gate does: the format is a runtime argument, and `new CompressionStream(process.argv[2])` leaves no `"brotli"` anywhere in the HIR, so a literal match would ship a false negative that fails at runtime. Direct construction lowers to `New { class_name: "CompressionStream", .. }` and an alias (`const C = CompressionStream`) to `PropertyGet { property: "CompressionStream", .. }`, so the constructor name survives both. `DecompressionStream` is checked separately because its `c` is lowercase.

If Brotli is ever requested without the feature compiled in — only possible through a fully computed global name the detector cannot see — construction throws `ERR_FEATURE_UNAVAILABLE_ON_PLATFORM` naming the missing feature, rather than producing wrong output.

The detector tests use HIR strings taken verbatim from `--print-hir` dumps, including the runtime-format case that a `contains("rotli")` detector would miss.
