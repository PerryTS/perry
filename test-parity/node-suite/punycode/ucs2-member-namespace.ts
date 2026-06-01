// #3925: `node:punycode.ucs2` is NOT an importable module, but `ucs2` is a
// property of node:punycode exposing decode/encode.
import punycode from "node:punycode";
console.log(JSON.stringify(punycode.ucs2.decode("xy")));
console.log(punycode.ucs2.encode([120, 121]));
