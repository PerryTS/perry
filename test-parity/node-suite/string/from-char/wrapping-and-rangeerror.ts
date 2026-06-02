// #2788: String.fromCharCode applies ToUint16 (negative/out-of-range wrap
// modulo 65536); String.fromCodePoint throws RangeError for negative,
// non-integer, and > 0x10FFFF code points; zero-arg calls return "".
console.log("fcc(65,66,67):", String.fromCharCode(65, 66, 67));
console.log("fcc(0x1F600)->cp:", String.fromCharCode(0x1F600).codePointAt(0)); // 0xF600
console.log("fcc(-1)->cp:", String.fromCharCode(-1).codePointAt(0)); // 0xFFFF
console.log("fcc(65.9):", String.fromCharCode(65.9)); // "A"
console.log("fcc(65536)->cp:", String.fromCharCode(65536).codePointAt(0)); // 0 wraps -> "\0"
console.log("fcc():", JSON.stringify(String.fromCharCode())); // ""

console.log("fcp(0x1F600,65):", String.fromCodePoint(0x1F600, 65)); // "😀A"
console.log("fcp():", JSON.stringify(String.fromCodePoint())); // ""
for (const cp of [1114112, -1, 3.14]) {
  try {
    String.fromCodePoint(cp);
    console.log("fcp(" + cp + "): NO THROW");
  } catch (e) {
    console.log("fcp(" + cp + "):", (e as Error).name + ": " + (e as Error).message);
  }
}
