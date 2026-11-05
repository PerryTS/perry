// req() of stream/promises, stream/consumers and stream/web: every export must
// run the real implementation, however it is read or called.
import { createRequire } from "node:module";
const req = createRequire(import.meta.url);
"use strict";
const stream = req("stream");
const sp = req("stream/promises");
const spPrefixed = req("node:stream/promises");
const { finished, pipeline } = req("stream/promises");
const sc = req("stream/consumers");
const scPrefixed = req("node:stream/consumers");
const { text, json } = req("stream/consumers");
const sw = req("stream/web");
const swPrefixed = req("node:stream/web");
const { ReadableStream, TransformStream } = req("stream/web");
const show = async (label, fn) => {
  try { console.log(label, JSON.stringify(await fn())); }
  catch (e) { console.log(label, "threw", e && e.name); }
};
const source = (...chunks) => stream.Readable.from(chunks);
(async () => {
  for (const n of ["pipeline", "finished"]) {
    console.log("stream/promises", n, typeof sp[n], typeof spPrefixed[n], sp[n] === stream.promises[n]);
  }
  console.log("stream/promises namespace", sp === stream.promises, spPrefixed === sp);
  await show("finished member", async () => {
    const r = source("x");
    r.resume();
    return (await sp.finished(r)) === undefined;
  });
  await show("finished destructured", async () => {
    const r = source("x");
    r.resume();
    return (await finished(r)) === undefined;
  });
  await show("pipeline comma", async () => {
    const chunks = [];
    const sink = new stream.Writable({ write(c, e, cb) { chunks.push(String(c)); cb(); } });
    await (0, pipeline)(source("c"), sink);
    return chunks;
  });

  for (const n of ["text", "json", "buffer", "arrayBuffer", "blob"]) {
    console.log("stream/consumers", n, typeof sc[n], typeof scPrefixed[n]);
  }
  await show("text member", () => sc.text(source("ab", "cd")));
  await show("text prefixed", () => scPrefixed.text(source("pre", "fixed")));
  await show("text comma", () => (0, sc.text)(source("com", "ma")));
  await show("text destructured", () => text(source("dest")));
  await show("json destructured", () => json(source('{"a":', "1}")));
  await show("json member", () => sc.json(source('[1,', '2]')));
  await show("buffer", async () => (await sc.buffer(source(Buffer.from("buf")))).toString());
  await show("arrayBuffer", async () => (await sc.arrayBuffer(source(Buffer.from("abcd")))).byteLength);
  await show("blob", async () => (await sc.blob(source(Buffer.from("blob")))).size);

  const classes = ["ReadableStream", "WritableStream", "TransformStream", "TextEncoderStream",
    "TextDecoderStream", "CompressionStream", "DecompressionStream", "ByteLengthQueuingStrategy",
    "CountQueuingStrategy", "ReadableStreamDefaultReader", "WritableStreamDefaultWriter"];
  for (const n of classes) {
    // Only the classes Perry also defines on globalThis are compared with it; the
    // reader/writer and queuing-strategy classes are exports of stream/web alone.
    const global = ["ReadableStream", "WritableStream", "TransformStream", "TextEncoderStream",
      "TextDecoderStream", "CompressionStream", "DecompressionStream"].includes(n);
    console.log("stream/web", n, typeof sw[n], typeof swPrefixed[n], global ? sw[n] === globalThis[n] : sw[n] === swPrefixed[n]);
  }
  console.log("stream/web destructured", ReadableStream === globalThis.ReadableStream, TransformStream === globalThis.TransformStream);
  process.exit(0);
})();
