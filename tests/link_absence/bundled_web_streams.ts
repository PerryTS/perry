// HWS hook: js_stdlib_install_bundled_streams
// HWS prefixes: perry_stdlib::streams::|brotli::|brotli_decompressor::|Brotli|js_readable_stream_|js_writable_stream_|js_transform_stream_|js_compression_stream_|js_decompression_stream_|js_stream_
import { open } from "node:fs/promises";
async function main() {
  const file = await open("value.json", "r");
  const stream = file.readableWebStream();
  (stream as any).label = "ok";
  console.log((stream as any).label);
  const reader = stream.getReader();
  console.log((await reader.read()).done === false);
  await reader.cancel();
  await file.close();

}
main();
