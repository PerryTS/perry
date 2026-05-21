import { parse } from "node:querystring";

// `maxKeys` and `decodeURIComponent` must coexist on the same options
// object. The limit applies BEFORE decoding — Node counts pairs, not
// decoded keys.
const tag = (s: string): string => `<${s}>`;
console.log(JSON.stringify(parse("a=1&b=2&c=3&d=4", undefined, undefined, { maxKeys: 2, decodeURIComponent: tag })));
console.log(JSON.stringify(parse("a=1&b=2&c=3", undefined, undefined, { maxKeys: 0, decodeURIComponent: tag })));

// Custom separators + custom decoder.
console.log(JSON.stringify(parse("a:1;b:2", ";", ":", { decodeURIComponent: tag })));
