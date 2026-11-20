// node:punycode (deprecated) — top-level conversion surface (#2513).
import punycode from "node:punycode";

console.log("version:", punycode.version);
console.log("encode:", punycode.encode("münchen"));
console.log("decode:", punycode.decode("mnchen-3ya"));
console.log("encode bücher:", punycode.encode("bücher"));
console.log("decode bücher:", punycode.decode("bcher-kva"));
console.log("toASCII:", punycode.toASCII("münchen.de"));
console.log("toUnicode:", punycode.toUnicode("xn--mnchen-3ya.de"));
console.log("toASCII multi:", punycode.toASCII("faß.bücher.de"));
console.log("toUnicode multi:", punycode.toUnicode("xn--fa-hia.xn--bcher-kva.de"));
console.log("ascii passthrough:", punycode.toASCII("example.com"));
console.log("unicode passthrough:", punycode.toUnicode("example.com"));
console.log("short encode:", punycode.encode("ä"));
console.log("short decode:", punycode.decode("4ca"));
console.log("empty encode:", JSON.stringify(punycode.encode("")));
