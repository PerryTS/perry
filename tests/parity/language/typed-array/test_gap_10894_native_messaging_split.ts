// #10894 — the program that found it: the TypeScript Native Messaging host
// from https://github.com/guest271314/NativeMessagingHosts (`nm_typescript.ts`).
//
// A host may send the browser at most 1 MiB per message, so its `sendMessage`
// splits a larger reply at a comma found with
// `message.indexOf(COMMA, searchStart)`. `message` is annotated
// `Uint8Array<ArrayBuffer>` — what `tsc` infers for `new Uint8Array(n)` since
// TypeScript 5.7. Perry lowered that spelling as an unrecognized generic, the
// Array fast path claimed the receiver, and `indexOf` answered -1: the reply
// was never split and a 1,048,581-byte message went out as ONE frame.
//
// This is `sendMessage` verbatim with `stdout.write` swapped for a collector,
// so it needs no stdin. Every frame must be <= 1 MiB and must itself be a
// JSON array; the element count across frames must equal the input's.

const FRAMES: number[] = [];
let ELEMENTS = 0;
let WELL_FORMED = true;

function emit(frame: Uint8Array<ArrayBuffer>): void {
  // frame = 4-byte little-endian length + payload
  const declared = frame[0] | (frame[1] << 8) | (frame[2] << 16) | (frame[3] << 24);
  const payload = frame.subarray(4);
  if (declared !== payload.length) WELL_FORMED = false;
  if (payload[0] !== 91 || payload[payload.length - 1] !== 93) WELL_FORMED = false;
  // `[null,null,…]`: elements = commas + 1
  let commas = 0;
  for (let i = 0; i < payload.length; i++) if (payload[i] === 44) commas++;
  ELEMENTS += commas + 1;
  FRAMES.push(payload.length);
}

function sendMessage(message: Uint8Array<ArrayBuffer>): void {
  const COMMA: number = 44;
  const OPEN_BRACKET: number = 91;
  const CLOSE_BRACKET: number = 93;
  const CHUNK_SIZE: number = 1024 * 1024;

  if (message.length <= CHUNK_SIZE) {
    const framed: Uint8Array<ArrayBuffer> = new Uint8Array(4 + message.length);
    framed.set(new Uint8Array(new Uint32Array([message.length]).buffer), 0);
    framed.set(message, 4);
    emit(framed);
    return;
  }

  let index: number = 0;

  while (index < message.length) {
    let splitIndex: number;
    let searchStart: number = index + CHUNK_SIZE - 8;

    if (searchStart >= message.length) {
      splitIndex = message.length;
    } else {
      splitIndex = message.indexOf(COMMA, searchStart);
      if (splitIndex === -1) {
        splitIndex = message.length;
      }
    }

    const rawChunk: Uint8Array<ArrayBuffer> = message.subarray(
      index,
      splitIndex,
    );
    const startByte: number = rawChunk[0];
    const endByte: number = rawChunk[rawChunk.length - 1];

    let prepend: number | null = null;
    let append: number | null = null;

    if (startByte === OPEN_BRACKET && endByte !== CLOSE_BRACKET) {
      append = CLOSE_BRACKET;
    } else if (startByte === COMMA) {
      prepend = OPEN_BRACKET;
      if (endByte !== CLOSE_BRACKET) {
        append = CLOSE_BRACKET;
      }
    }

    let bodyLength: number = rawChunk.length;
    let sourceOffset: number = 0;
    if (startByte === COMMA) {
      sourceOffset = 1;
      bodyLength -= 1;
    }

    const totalLength: number = 4 + (prepend !== null ? 1 : 0) + bodyLength +
      (append !== null ? 1 : 0);
    const output: Uint8Array<ArrayBuffer> = new Uint8Array(totalLength);

    const dataPayloadLen: number = totalLength - 4;
    output[0] = (dataPayloadLen >> 0) & 0xff;
    output[1] = (dataPayloadLen >> 8) & 0xff;
    output[2] = (dataPayloadLen >> 16) & 0xff;
    output[3] = (dataPayloadLen >> 24) & 0xff;

    let cursor: number = 4;
    if (prepend !== null) {
      output[cursor] = prepend;
      cursor++;
    } else if (startByte === COMMA) {
      output[cursor] = OPEN_BRACKET;
      cursor++;
    }

    output.set(rawChunk.subarray(sourceOffset), cursor);
    cursor += bodyLength;

    if (append !== null) {
      output[cursor] = append;
    }

    emit(output);
    index = splitIndex;
  }
}

/** The bytes of `JSON.stringify(new Array(n))` — `[null,null,…]`. */
function nulls(n: number): Uint8Array<ArrayBuffer> {
  const out: Uint8Array<ArrayBuffer> = new Uint8Array(n * 5 + 1);
  out[0] = 91;
  for (let i = 0; i < n; i++) {
    const at = 1 + i * 5;
    out[at] = 110; // n
    out[at + 1] = 117; // u
    out[at + 2] = 108; // l
    out[at + 3] = 108; // l
    out[at + 4] = 44; // ,
  }
  out[out.length - 1] = 93; // the last comma becomes `]`
  return out;
}

// 1 MiB exactly (unsplit), 1 MiB + 5 B (the smallest reply that must split),
// ~2 MiB, ~5 MiB.
for (const n of [209715, 209716, 419430, 1048576]) {
  FRAMES.length = 0;
  ELEMENTS = 0;
  WELL_FORMED = true;
  const message = nulls(n);
  sendMessage(message);
  let largest = 0;
  let total = 0;
  for (const f of FRAMES) {
    if (f > largest) largest = f;
    total += f;
  }
  console.log(
    `n=${n} bytes=${message.length} frames=${FRAMES.length} largest=${largest}` +
      ` within_1MiB=${largest <= 1024 * 1024} payload_total=${total}` +
      ` elements=${ELEMENTS} elements_ok=${ELEMENTS === n} well_formed=${WELL_FORMED}`,
  );
}
