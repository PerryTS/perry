// node:punycode import/default shape for the deprecated core module (#3816).
import punycodeDefault, {
  decode,
  encode,
  toASCII,
  toUnicode,
  ucs2,
  version,
} from "node:punycode";
import * as ns from "node:punycode";
import prefixlessDefault from "punycode";
import * as prefixlessNs from "punycode";

console.log("default object:", typeof punycodeDefault, punycodeDefault !== null);
console.log("namespace default identity:", ns.default === punycodeDefault);
console.log("prefixless default identity:", prefixlessDefault === punycodeDefault);
console.log(
  "prefixless namespace default identity:",
  prefixlessNs.default === prefixlessDefault,
);
console.log(
  "named function identity:",
  decode === punycodeDefault.decode,
  encode === ns.encode,
  toASCII === punycodeDefault.toASCII,
  toUnicode === ns.toUnicode,
);
console.log(
  "ucs2 identity:",
  ucs2 === punycodeDefault.ucs2,
  ucs2 === ns.ucs2,
  prefixlessDefault.ucs2 === ucs2,
);
console.log("version identity:", version === punycodeDefault.version, version);
console.log(
  "conversion sample:",
  encode("mañana"),
  decode("maana-pta"),
  toASCII("mañana.com"),
  toUnicode("xn--maana-pta.com"),
  ucs2.encode([97, 128512, 98]),
);
