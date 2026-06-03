import { TextDecoderStream, TextEncoderStream } from "node:stream/web";

async function collectText(readable: ReadableStream<string>) {
  const reader = readable.getReader();
  let out = "";
  while (true) {
    const chunk = await reader.read();
    if (chunk.done) {
      return out;
    }
    out += chunk.value;
  }
}

async function collectBytes(readable: ReadableStream<Uint8Array>) {
  const reader = readable.getReader();
  const chunks: Uint8Array[] = [];
  let total = 0;
  while (true) {
    const chunk = await reader.read();
    if (chunk.done) {
      break;
    }
    chunks.push(chunk.value);
    total += chunk.value.byteLength;
  }
  const out = new Uint8Array(total);
  let offset = 0;
  for (const chunk of chunks) {
    out.set(chunk, offset);
    offset += chunk.byteLength;
  }
  return out;
}

const encoder = new TextEncoderStream();
console.log("encoder props:", encoder.encoding);
console.log(
  "encoder streams:",
  encoder.readable instanceof ReadableStream,
  encoder.writable instanceof WritableStream,
);
console.log(
  "encoder identity:",
  encoder.constructor === TextEncoderStream,
  encoder instanceof TextEncoderStream,
);

const encoded = collectBytes(encoder.readable);
const encoderWriter = encoder.writable.getWriter();
await encoderWriter.write("he");
await encoderWriter.write("llo");
await encoderWriter.close();
console.log("encoded text:", new TextDecoder().decode(await encoded));

const decoder = new TextDecoderStream("utf-8", { fatal: true, ignoreBOM: true });
console.log("decoder props:", decoder.encoding, decoder.fatal, decoder.ignoreBOM);
console.log(
  "decoder streams:",
  decoder.readable instanceof ReadableStream,
  decoder.writable instanceof WritableStream,
);
console.log(
  "decoder identity:",
  decoder.constructor === TextDecoderStream,
  decoder instanceof TextDecoderStream,
);

const decoded = collectText(decoder.readable);
const decoderWriter = decoder.writable.getWriter();
await decoderWriter.write(new Uint8Array([0x68, 0xc3]));
await decoderWriter.write(new Uint8Array([0xa9]));
await decoderWriter.close();
console.log("decoded text:", await decoded);

try {
  new TextDecoderStream("not-an-encoding");
  console.log("bad decoder label: ok");
} catch (err: any) {
  console.log("bad decoder label:", err.constructor.name, err.code);
}
