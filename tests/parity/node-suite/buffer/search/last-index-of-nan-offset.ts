// `Buffer#lastIndexOf` coerces its offset with `+byteOffset`: a NaN result
// (NaN, undefined, a non-numeric string or object) searches the whole buffer,
// and a string offset is the encoding. A `Uint8Array` takes
// ToIntegerOrInfinity, so the same offsets search from index 0.
const offsets: [string, unknown][] = [
  ["NaN", NaN],
  ["undefined", undefined],
  ["null", null],
  ["\"x\"", "x"],
  ["\"1\"", "1"],
  ["{}", {}],
  ["[]", []],
  ["1.5", 1.5],
  ["true", true],
  ["Infinity", Infinity],
];
const buf = Buffer.from([7, 8, 7]);
const u8 = new Uint8Array([7, 8, 7]);
const anyBuf: any = buf;
const anyU8: any = u8;
for (const [name, offset] of offsets) {
  const o = offset as number;
  console.log(
    name.padEnd(10),
    "Buffer",
    buf.lastIndexOf(7, o),
    anyBuf.lastIndexOf(7, o),
    buf.lastIndexOf(Buffer.from([7]), o),
    "Uint8Array",
    u8.lastIndexOf(7, o),
    anyU8.lastIndexOf(7, o),
  );
}
const text = Buffer.from("abcabc");
console.log("string needle", text.lastIndexOf("abc", NaN), text.lastIndexOf("abc", undefined));
console.log("encoding offset", text.lastIndexOf("abc", "utf8"), text.lastIndexOf("616263", "hex"));
console.log("valueOf", buf.lastIndexOf(7, { valueOf: () => 1 } as any), buf.lastIndexOf(7, { valueOf: () => NaN } as any));
console.log("empty needle", text.lastIndexOf("", NaN), text.lastIndexOf(""), text.lastIndexOf(Buffer.alloc(0), undefined), text.lastIndexOf("", "utf8"));
