// node:punycode ucs2 UTF-16 surrogate code-unit behavior (#3002).
(process as any).noDeprecation = true;
const punycode = (process as any).getBuiltinModule("punycode");

function codeUnits(value: string) {
  const out: number[] = [];
  for (let i = 0; i < value.length; i++) {
    out.push(value.charCodeAt(i));
  }
  return JSON.stringify(out);
}

console.log("decode lone high:", JSON.stringify(punycode.ucs2.decode("\uD800")));
console.log("decode lone low:", JSON.stringify(punycode.ucs2.decode("A\uDC00")));
console.log("decode split pair:", JSON.stringify(punycode.ucs2.decode("\uD83DA\uDE00")));
console.log("decode valid pair:", JSON.stringify(punycode.ucs2.decode("\uD83D\uDE00")));
console.log("encode high unit:", codeUnits(punycode.ucs2.encode([0xD800])));
console.log("encode low unit:", codeUnits(punycode.ucs2.encode([0xDC00])));
console.log(
  "encode split units:",
  codeUnits(punycode.ucs2.encode([0xD83D, 65, 0xDE00])),
);
console.log("encode valid pair:", codeUnits(punycode.ucs2.encode([0x1F600])));
