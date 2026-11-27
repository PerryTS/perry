// `Buffer#indexOf` / `#includes` / `#lastIndexOf` read a string `byteOffset`
// as the encoding (Node's `bidirectionalIndexOf`), and an unknown encoding
// throws `ERR_UNKNOWN_ENCODING` when the needle is a string. A numeric or
// Buffer needle ignores the encoding, and a `Uint8Array` takes
// ToIntegerOrInfinity for the same argument.
const r = (f: () => unknown) => {
  try {
    return f();
  } catch (error) {
    return (error as NodeJS.ErrnoException).code ?? String(error);
  }
};
const bytes = Buffer.from([7, 8, 7]);
const text = Buffer.from("abcabc");
const u8 = new Uint8Array([7, 8, 7]);
const anyText: any = text;
for (const offset of ["1", "utf8", "hex", "LATIN1", "bogus"]) {
  const o = offset as any;
  console.log(
    JSON.stringify(offset).padEnd(9),
    "indexOf",
    r(() => bytes.indexOf(7, o)),
    r(() => text.indexOf("abc", o)),
    r(() => anyText.indexOf("616263", o)),
    r(() => text.indexOf(Buffer.from("c"), o)),
    "includes",
    r(() => bytes.includes(8, o)),
    r(() => text.includes("c", o)),
    "lastIndexOf",
    r(() => text.lastIndexOf("abc", o)),
    "Uint8Array",
    r(() => u8.indexOf(7, o)),
    r(() => u8.includes(7, o)),
  );
}
console.log("third argument", r(() => text.indexOf("abc", 1, "bogus" as any)), r(() => text.includes("abc", 0, 5 as any)), r(() => text.indexOf(97, 0, "bogus" as any)), r(() => text.indexOf("abc", 1, undefined)));
console.log("numeric offsets", text.indexOf("abc", NaN), text.indexOf("abc", undefined), text.indexOf("abc", {} as any), text.indexOf("abc", 2), text.includes("abc", 4));
