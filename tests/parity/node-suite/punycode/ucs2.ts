// node:punycode ucs2 code-point helpers (#2607).
import punycode from "node:punycode";

console.log("decode abc:", JSON.stringify(punycode.ucs2.decode("abc")));
console.log("encode abc:", punycode.ucs2.encode([97, 98, 99]));
console.log("decode astral:", JSON.stringify(punycode.ucs2.decode("a\u{1F600}b")));
console.log("encode astral:", punycode.ucs2.encode([97, 128512, 98]));
console.log("decode accented:", JSON.stringify(punycode.ucs2.decode("héllo")));
console.log("roundtrip:", punycode.ucs2.encode(punycode.ucs2.decode("héllo😀")));
console.log("empty decode:", JSON.stringify(punycode.ucs2.decode("")));

import * as p from "node:punycode";
console.log("ns decode:", JSON.stringify(p.ucs2.decode("xy")));
console.log("ns encode:", p.ucs2.encode([120, 121]));
