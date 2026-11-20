// Gap test: fs.readSync argument validation. offset and length are checked in
// Node's order: offset (integer >= 0), `length |= 0` with 0 returning 0 at
// once, an empty buffer, length >= 0, offset + length <= byteLength, then the
// position. A length past the buffer throws ERR_OUT_OF_RANGE instead of being
// clamped.
import * as fs from "node:fs";

const path = "/tmp/perry_gap_fs_readsync_range_validation.txt";
fs.writeFileSync(path, "0123456789abcdefghij");
const fd = fs.openSync(path, "r");
const buf = Buffer.alloc(16);

function show(label: string, f: () => unknown): void {
  try {
    console.log(label, "ok", f());
  } catch (e: any) {
    console.log(label, e.name, e.code, e.message);
  }
}

const cases: [string, any, any, any][] = [
  ["length past buffer", 0, 32, 0],
  ["length one past", 0, 17, 0],
  ["offset+length past", 8, 16, 0],
  ["exact fit", 0, 16, 0],
  ["offset at end, length 0", 16, 0, 0],
  ["offset past end, length 0", 17, 0, 0],
  ["offset past end, length 4", 17, 4, 0],
  ["negative offset", -1, 4, 0],
  ["negative length", 0, -1, 0],
  ["fractional offset", 1.5, 4, 0],
  ["fractional length", 0, 1.5, 0],
  ["NaN offset", NaN, 4, 0],
  ["NaN length", 0, NaN, 0],
  ["Infinity offset", Infinity, 4, 0],
  ["Infinity length", 0, Infinity, 0],
  ["2**53 offset", 2 ** 53, 4, 0],
  ["2**31 length", 0, 2 ** 31, 0],
  ["2**40 length", 0, 2 ** 40, 0],
  ["string offset", "a", 4, 0],
  ["position -1", 0, 4, -1],
  ["position fractional", 0, 4, 1.5],
  ["position string", 0, 4, "x"],
  ["position 2**53", 0, 4, 2 ** 53],
  ["position null", 0, 4, null],
  ["position undefined", 0, 4, undefined],
  ["position bigint", 0, 4, 2n],
  ["length 0 skips position check", 0, 0, "x"],
];
for (const [label, o, l, p] of cases) {
  show(label, () => fs.readSync(fd, buf, o, l, p));
}
show("undefined offset", () => fs.readSync(fd, buf, undefined as any, 4, 0));
show("null offset reads whole buffer", () => fs.readSync(fd, buf, null as any, 4, 0));
show("empty buffer", () => fs.readSync(fd, new Uint8Array(0), 0, 1, 0));
show("empty buffer, length 0", () => fs.readSync(fd, new Uint8Array(0), 0, 0, 0));
show("options length past", () => fs.readSync(fd, buf, { length: 20 }));
show("options offset past", () => fs.readSync(fd, buf, { offset: 20 }));
show("options offset+length past", () => fs.readSync(fd, buf, { offset: 2, length: 15 }));
show("options ok", () => fs.readSync(fd, buf, { offset: 2, length: 4, position: 0 }));
show("content intact", () => buf.subarray(2, 6).toString());
fs.closeSync(fd);
fs.unlinkSync(path);
