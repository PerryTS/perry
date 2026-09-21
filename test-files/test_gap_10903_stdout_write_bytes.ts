// #10903: `process.stdout.write(chunk[, encoding])` / `process.stderr.write(...)`
// started from the chunk's DISPLAY TEXT and ignored `encoding`:
//   - a Buffer / Uint8Array was UTF-8 *decoded* and the text written, so every
//     byte that is not valid UTF-8 reached the fd as EF BF BD;
//   - any other TypedArray was written as its join(",") text, a DataView as
//     "[object DataView]";
//   - write("6869", "hex") wrote four characters instead of two bytes;
//   - on a non-blocking fd a large chunk was silently truncated at EAGAIN.
// Node writes a binary chunk byte for byte (exactly the view's window) and
// encodes a string chunk with `encoding`.
//
// Two modes:
//   * undriven (the parity sweep, which compares TEXT): every chunk below is
//     printable ASCII on the wire, so the wrong conversions still show up as
//     wrong text ("26984,2593" instead of "hi!", "6865780a" instead of "hex").
//   * PERRY_10903_CASE=<name>: raw bytes, compared fd by fd against what Node
//     writes by `crates/perry/tests/issue_10903_stdout_write_bytes.rs`.
const which: string = process.env.PERRY_10903_CASE ?? "";

function pattern(length: number, mul: number): Uint8Array {
  const bytes = new Uint8Array(length);
  for (let i = 0; i < length; i++) bytes[i] = (i * mul + (i >>> 8)) & 0xff;
  return bytes;
}

// Native Messaging framing: 4-byte little-endian length, then the payload.
function sendFrame(payload: Uint8Array): void {
  process.stdout.write(new Uint8Array(new Uint32Array([payload.length]).buffer));
  process.stdout.write(payload);
}

function chunkKinds(): void {
  const out = process.stdout;
  // The bytes that found the bug: a frame length of 1,048,567.
  process.stdout.write(new Uint8Array([0xf7, 0xff, 0x0f, 0x00, 0x0a]));
  process.stdout.write(Buffer.from([0xf7, 0xff, 0x44, 0x0a]));
  // Aliased and bound receivers reach the same method.
  out.write(new Uint8Array([0xc8, 0x00, 0x80, 0x0a]));
  const bound = process.stdout.write.bind(process.stdout);
  bound(new Uint8Array([0xb0, 0xb1, 0x0a]));
  // Views: only the window may be written.
  const backing = new Uint8Array(16);
  backing.set([0xfe, 0xfd, 0x80, 0x0a], 6);
  out.write(backing.subarray(6, 10));
  out.write(new Uint8Array(backing.buffer, 7, 3));
  out.write(Buffer.from([0x01, 0x91, 0x92, 0x0a, 0x05]).subarray(1, 4));
  const wide = new ArrayBuffer(16);
  new Uint8Array(wide).set([0, 0, 0xa1, 0xa2, 0xa3, 0x0a, 0, 0]);
  out.write(new Uint16Array(wide, 2, 2) as any);
  out.write(new DataView(wide, 3, 3) as any);
  // Wider element types go out as their stored bytes, not as numbers.
  out.write(new Uint16Array([0xfffe, 0x0a80]) as any);
  out.write(new Uint32Array([0x0a80fffe]) as any);
  out.write(new Int8Array([-1, -128, 10]) as any);
  out.write(new Uint8ClampedArray([300, 0x99, 10]) as any);
  out.write(new Float64Array([-2.5]) as any);
  out.write(new Uint8Array([0x0a]));
  out.write(new DataView(new Uint8Array([0x99, 0x98, 0x97, 0x0a]).buffer) as any);
  // A zero-length chunk writes nothing.
  out.write(new Uint8Array(0));
  out.write(Buffer.alloc(0));
  // Strings are unaffected.
  out.write("héllo\n");
  process.stderr.write(Buffer.from([0xf1, 0xf2, 0x0a]));
  const err = process.stderr;
  err.write(new Uint8Array([0x80, 0x0a]));
  err.write(new Uint16Array([0x0aff]) as any);
}

function encodings(): void {
  const out = process.stdout;
  out.write("hé\n", "latin1");
  out.write("hé\n", "binary");
  out.write("hé\n", "ascii");
  out.write("hé\n", "utf8");
  out.write("hé\n", "UTF-8");
  out.write("f7ff0f000a", "hex");
  out.write("9/8PAAo=", "base64");
  out.write("9_8PAAo", "base64url");
  out.write("hé\n", "ucs2");
  out.write("hé\n", "utf16le");
  // An encoding names how to encode a STRING; a binary chunk ignores it.
  out.write(Buffer.from([0xf7, 0x0a]), "hex");
  out.write(new Uint8Array([0xf8, 0x0a]), "latin1");
  process.stderr.write("ÿ\n", "latin1");
  process.stderr.write("800a", "hex");
}

function callbacks(): void {
  const mark = (text: string) => () => process.stderr.write(text);
  const a: boolean = process.stdout.write(new Uint8Array([0xe0, 0x0a]), mark("cb1\n"));
  const b: boolean = process.stdout.write("e10a", "hex", mark("cb2\n"));
  const c: boolean = process.stdout.write(Buffer.from([0xe2, 0x0a]), "utf8", mark("cb3\n"));
  process.stderr.write(`returned ${a} ${b} ${c}\n`);
}

function interleave(): void {
  console.log("one");
  process.stdout.write(new Uint8Array([0x80, 0x0a]));
  console.log("two");
  process.stdout.write("three");
  process.stdout.write(new Uint8Array([0x81, 0x0a]));
  console.log("four");
  process.stdout.write("f50a", "hex");
  console.log("five");
}

function framing(): void {
  sendFrame(pattern(200, 7)); // length C8 00 00 00
  sendFrame(pattern(1048567, 13)); // length F7 FF 0F 00
}

function large(): void {
  process.stdout.write(pattern(5 * 1024 * 1024 + 3, 31));
  process.stdout.write(Buffer.from(pattern(3 * 1024 * 1024 + 1, 17)));
  process.stdout.write("done\n");
}

function many(): void {
  const out = process.stdout;
  for (let i = 0; i < 20000; i++) {
    out.write(new Uint8Array([i & 0xff, (i >>> 8) & 0xff, 0x80, 0x0a]));
    if (i % 1000 === 0) console.log("k" + i);
  }
}

// One write() each of 16, 32 and 64 MiB: a chunk far larger than any pipe or
// socket buffer must still arrive whole and in order.
function huge(): void {
  for (const mib of [16, 32, 64]) {
    const bytes = new Uint8Array(mib * 1024 * 1024).fill(0x80 + mib);
    bytes[0] = mib;
    bytes[bytes.length - 1] = 0x0a;
    process.stdout.write(bytes);
  }
}

// Node re-wraps a non-Buffer view over its ArrayBuffer, which throws once the
// buffer has been transferred away; nothing may be written for that chunk.
function detached(): void {
  const names: string[] = [];
  const ab = new ArrayBuffer(4);
  const view = new Uint8Array(ab);
  view.set([0xf1, 0xf2, 0xf3, 0x0a]);
  const dataView = new DataView(ab, 1, 2);
  (ab as any).transfer();
  for (const chunk of [view, dataView]) {
    try {
      process.stdout.write(chunk as any);
      names.push("no throw");
    } catch (e: any) {
      names.push(e.name);
    }
  }
  process.stdout.write(names.join(",") + "\n");
}

function asciiParity(): void {
  const out = process.stdout;
  process.stdout.write(new Uint8Array([111, 107, 10])); // ok
  process.stdout.write(Buffer.from("buffer\n"));
  out.write(new Uint16Array([0x6968, 0x0a21]) as any); // hi!
  out.write(new Uint32Array([0x0a323375]) as any); // u32
  const backing = new Uint8Array([120, 119, 105, 110, 10, 120]);
  out.write(backing.subarray(1, 5)); // win
  out.write(new Uint8Array(backing.buffer, 2, 3)); // in
  out.write(new DataView(new Uint8Array([120, 100, 118, 10, 120]).buffer, 1, 3) as any); // dv
  out.write(Buffer.from("xslice\ny").subarray(1, 7)); // slice
  out.write(new Uint8Array(0));
  const bound = process.stdout.write.bind(process.stdout);
  bound(new Uint8Array([98, 111, 117, 110, 100, 10])); // bound
  out.write("6865780a", "hex"); // hex
  out.write("YjY0Cg==", "base64"); // b64
  out.write("YjY0dXJsCg", "base64url"); // b64url
  out.write("latin1\n", "latin1");
  out.write(new Uint8Array([105, 103, 110, 10]), "hex"); // ign: encoding ignored
  console.log("typeof:", typeof out.write(new Uint8Array([114, 101, 116, 10]))); // ret
  process.stderr.write(new Uint8Array([101, 114, 114, 10])); // err
  process.stderr.write(new Uint16Array([0x3265, 0x0a21]) as any); // e2!
  process.stderr.write("65330a", "hex"); // e3
}

switch (which) {
  case "chunks":
    chunkKinds();
    break;
  case "encodings":
    encodings();
    break;
  case "callbacks":
    callbacks();
    break;
  case "interleave":
    interleave();
    break;
  case "framing":
    framing();
    break;
  case "large":
    large();
    break;
  case "many":
    many();
    break;
  case "huge":
    huge();
    break;
  case "detached":
    detached();
    break;
  default:
    asciiParity();
}
