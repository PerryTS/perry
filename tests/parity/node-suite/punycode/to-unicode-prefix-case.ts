(process as any).noDeprecation = true;
const punycode = (process as any).getBuiltinModule("punycode");

console.log(
  "lower prefix lower payload:",
  punycode.toUnicode("xn--bcher-kva.example"),
);
console.log(
  "lower prefix upper payload:",
  punycode.toUnicode("xn--BCHER-KVA.example"),
);
console.log(
  "upper prefix preserved:",
  punycode.toUnicode("XN--BCHER-KVA.example"),
);
console.log(
  "mixed prefix preserved 1:",
  punycode.toUnicode("Xn--bcher-kva.example"),
);
console.log(
  "mixed prefix preserved 2:",
  punycode.toUnicode("xN--bcher-kva.example"),
);
console.log(
  "multi labels:",
  punycode.toUnicode("XN--BCHER-KVA.xn--BCHER-KVA"),
);
