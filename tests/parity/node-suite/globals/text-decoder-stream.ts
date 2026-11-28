// `TextDecoder#decode(chunk, { stream: true })` keeps an incomplete sequence
// for the next call and consumes the BOM once per stream, for UTF-8, UTF-16LE
// and UTF-16BE; a call without `stream` ends the stream, flushing (or, with
// `fatal`, rejecting) what is still pending and starting the next one fresh.
const show = (s: string) => Array.from(s, (c) => c.codePointAt(0)!.toString(16)).join(",") || "-";
const run = (label: string, chunks: number[][], options: TextDecoderOptions = {}) => {
  const decoder = new TextDecoder(label, options);
  const out: string[] = [];
  chunks.forEach((chunk, i) => {
    try {
      out.push(show(decoder.decode(new Uint8Array(chunk), { stream: i < chunks.length - 1 })));
    } catch (error) {
      out.push((error as NodeJS.ErrnoException).code ?? String(error));
    }
  });
  console.log(label.padEnd(9), JSON.stringify(options).padEnd(18), out.join(" | "));
  return decoder;
};

// A character split at every possible point.
const euro = [0xe2, 0x82, 0xac];
const emoji = [0xf0, 0x9f, 0x98, 0x80];
run("utf-8", [euro.slice(0, 1), euro.slice(1)]);
run("utf-8", [euro.slice(0, 2), euro.slice(2)]);
run("utf-8", [emoji.slice(0, 1), emoji.slice(1, 2), emoji.slice(2, 3), emoji.slice(3), [0x41]]);
run("utf-16le", [[0x3d], [0xd8, 0x00], [0xde, 0x41], [0x00]]);
run("utf-16be", [[0xd8], [0x3d, 0xde], [0x00]]);
// BOM split across chunks, consumed once per stream.
run("utf-8", [[0xef], [0xbb], [0xbf, 0x48], [0xef, 0xbb, 0xbf]]);
run("utf-8", [[0xef, 0xbb], [0xbf, 0x48]], { ignoreBOM: true });
run("utf-16le", [[0xff], [0xfe, 0x48], [0x00]]);
run("utf-16be", [[0xfe], [0xff, 0x00], [0x48]]);
// Not a BOM after all, and an empty middle chunk.
run("utf-8", [[0xef], [], [0x41]]);
// Incomplete at the end of the stream: U+FFFD, or a TypeError with `fatal`.
run("utf-8", [euro.slice(0, 2), []]);
run("utf-8", [euro.slice(0, 2), []], { fatal: true });
run("utf-16le", [[0x41, 0x00, 0x3d], []]);
run("utf-16le", [[0x3d, 0xd8], []], { fatal: true });
// Invalid bytes mid-stream are still replaced (or rejected) right away.
run("utf-8", [[0xe0, 0x80], [0x41]]);
run("utf-8", [[0x41, 0xff], [0x42]], { fatal: true });
// The stream resets: the next stream consumes its own BOM.
const d = run("utf-8", [[0xef, 0xbb, 0xbf, 0x41], [0xe2]]);
console.log("next stream", show(d.decode(new Uint8Array([0xef, 0xbb, 0xbf, 0x42]))));
// A decode with no input ends the stream too.
const e = new TextDecoder();
console.log("flush", show(e.decode(new Uint8Array([0xe2, 0x82]), { stream: true })), show(e.decode()));
// Options read through a variable and through `any`.
const opts = { stream: true };
const f: any = new TextDecoder();
console.log("variable", show(f.decode(new Uint8Array([0xe2]), opts)) , show(f.decode(new Uint8Array([0x82, 0xac]))));
