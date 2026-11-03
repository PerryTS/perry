// HWS companion: bundled_web_streams
// HWS hook: js_stdlib_install_bundled_streams
// HWS prefixes: brotli::|brotli_decompressor::|Brotli
async function main() {
  const compressed = new CompressionStream("brotli");
  const drain = async () => {
    const reader = compressed.readable.getReader();
    let size = 0;
    while (true) {
      const next = await reader.read();
      if (next.done) break;
      size += next.value.byteLength;
    }
    return size;
  };
  const result = drain();
  const writer = compressed.writable.getWriter();
  await writer.write(new TextEncoder().encode("hello"));
  await writer.close();
  console.log((await result) > 0);
}
main();
