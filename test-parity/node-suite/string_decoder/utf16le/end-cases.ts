import { StringDecoder } from "node:string_decoder";

// Mirrors Node's test/parallel/test-string-decoder-end.js utf16le table —
// covers odd-byte carry, lone surrogates, mid-pair end() flushes, and the
// full astral round-trip. Issue #1182.
const cases: [string, string][] = [
  ["3D", "6100"],
  ["3D", "D84DDC"],
  ["3DD8", ""],
  ["3DD8", "6100"],
  ["3DD8", "4DDC"],
  ["3DD84D", ""],
  ["3DD84D", "6100"],
  ["3DD84D", "DC"],
  ["3DD84DDC", "6100"],
];
for (const [first, next] of cases) {
  const dec = new StringDecoder("utf16le");
  const output = dec.write(Buffer.from(first, "hex")) + dec.end() + dec.write(Buffer.from(next, "hex")) + dec.end();
  console.log(first + "/" + next + ":", JSON.stringify(output));
}
