// TextDecoder consumes one leading byte-order mark for UTF-8, UTF-16LE and
// UTF-16BE unless `ignoreBOM` is true, whether the options are a literal, a
// variable, an object with a getter, or reach a constructor value; single-byte
// encodings keep the bytes, and a BOM after the start is content.
const show = (s: string) => Array.from(s, (c) => c.codePointAt(0)!.toString(16)).join(",") || "(empty)";
const cases: [string, number[], number[]][] = [
  ["utf-8", [0xef, 0xbb, 0xbf], [0x48, 0x69]],
  ["utf-16le", [0xff, 0xfe], [0x48, 0x00, 0x69, 0x00]],
  ["utf-16be", [0xfe, 0xff], [0x00, 0x48, 0x00, 0x69]],
  ["windows-1252", [0xef, 0xbb, 0xbf], [0x48]],
];
const keep = { ignoreBOM: true };
const strip = { ignoreBOM: false, fatal: false };
const empty = {};
let reads = 0;
const getter = {
  get ignoreBOM() {
    reads++;
    return true;
  },
};
const Ctor: any = TextDecoder;
for (const [label, bom, body] of cases) {
  const once = new Uint8Array([...bom, ...body]);
  const twice = new Uint8Array([...bom, ...bom, ...body]);
  const middle = new Uint8Array([...body, ...bom]);
  console.log(label);
  console.log("  default      ", show(new TextDecoder(label).decode(once)));
  console.log("  literal true ", show(new TextDecoder(label, { ignoreBOM: true }).decode(once)));
  console.log("  literal false", show(new TextDecoder(label, { ignoreBOM: false }).decode(once)));
  console.log("  var keep     ", show(new TextDecoder(label, keep).decode(once)), new TextDecoder(label, keep).ignoreBOM);
  console.log("  var strip    ", show(new TextDecoder(label, strip).decode(once)));
  console.log("  var empty    ", show(new TextDecoder(label, empty).decode(once)));
  console.log("  getter       ", show(new TextDecoder(label, getter).decode(once)));
  console.log("  constructor  ", show(new Ctor(label).decode(once)), show(new Ctor(label, keep).decode(once)));
  const reused = new TextDecoder(label);
  console.log("  reused       ", show(reused.decode(once)), show(reused.decode(once)));
  console.log("  two BOMs     ", show(new TextDecoder(label).decode(twice)));
  console.log("  BOM only     ", show(new TextDecoder(label).decode(new Uint8Array(bom))));
  console.log("  BOM later    ", show(new TextDecoder(label).decode(middle)));
}
console.log("getter read", reads > 0);
// `globalThis.TextDecoder` is the global even where a parameter shadows the
// name, with non-literal options too.
function viaGlobal(TextDecoder: unknown, options: TextDecoderOptions) {
  return new globalThis.TextDecoder("utf-8", options);
}
const shadowed = viaGlobal(class {}, { fatal: true });
console.log("globalThis shadowed", shadowed.constructor.name, shadowed.fatal);
