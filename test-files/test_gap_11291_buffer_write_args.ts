// #11291: Buffer.prototype.write(string, offset, length, encoding) takes an
// explicit `undefined` wherever an argument may be omitted, with Node's full
// argument handling.
//
// bson's `encodeUTF8Into` calls `buf.write(source, byteOffset, undefined,
// 'utf8')` for any key or string over 25 chars or containing non-ASCII.
// Perry threw `ERR_INVALID_ARG_TYPE: Invalid Buffer length`, so
// `BSON.serialize` (and mongodb compiled from source) failed.
function show(label: string, f: () => unknown): void {
  try {
    console.log(label, JSON.stringify(f()));
  } catch (e: any) {
    console.log(label, "THREW", e?.name, e?.code, e?.message);
  }
}
function run(label: string, ...args: any[]): void {
  const b = Buffer.alloc(8, 0x2e);
  show(label, () => {
    const n = (b.write as any)(...args);
    return [n, b.toString("latin1")];
  });
}

run("(s)", "hello");
run("(s, undefined)", "hello", undefined);
run("(s, undefined, undefined)", "hello", undefined, undefined);
run("(s, undefined, undefined, enc)", "6869", undefined, undefined, "hex");
run("(s, undefined, 3, hex) ignores all", "6869", undefined, 3, "hex");
run("(s, off)", "hello", 2);
run("(s, off, undefined)", "hello", 2, undefined);
run("(s, off, undefined, enc)", "hello", 2, undefined, "utf8");
run("(s, off, undefined, latin1)", "hé", 1, undefined, "latin1");
run("(s, off, len)", "hello", 2, 3);
run("(s, off, len, enc)", "68656c6c6f", 1, 2, "hex");
run("(s, off, len > remaining)", "hello", 6, 5);
run("(s, off, enc)", "6869", 1, "hex");
run("(s, enc)", "6869", "hex");
run("(s, enc, undefined)", "6869", "hex", undefined);
run("(s, off, len, null enc)", "hello", 0, 2, null);
run("(s, off, len, empty enc)", "hello", 0, 2, "");
run("(s, off, len, undefined enc)", "hello", 0, 2, undefined);
run("utf8 multibyte", "héllo wörld", 0, undefined, "utf8");
run("utf8 long", "x".repeat(30), 1, undefined, "utf8");
run("off == length", "hello", 8);
run("off == length, len 0", "hello", 8, 0);

run("offset string with length", "x", "hex", 5);
run("offset null", "x", null);
run("offset out of range", "x", 9);
run("offset negative", "x", -1);
run("offset fraction", "x", 1.5);
run("offset NaN", "x", NaN);
run("length null", "x", 2, null);
run("length out of range", "x", 2, 9);
run("length negative", "x", 2, -1);
run("length fraction", "x", 2, 1.5);
run("unknown encoding", "x", 2, 3, "bogus");
run("numeric encoding", "x", 2, 3, 5);
run("non-string", 5);

// The bson shape: key encoded at a running offset into a larger buffer.
const out = Buffer.alloc(64);
let index = 4;
for (const key of ["a", "abcdefghijklmnopqrstuvwxyz0123", "héllo"]) {
  const n = out.write(key, index, undefined as any, "utf8");
  index += n;
  out[index++] = 0;
}
console.log("bson-style", index, out.subarray(0, index).toString("hex"));
