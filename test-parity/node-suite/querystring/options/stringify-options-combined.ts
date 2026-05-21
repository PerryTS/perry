import { stringify } from "node:querystring";

// Custom separators + custom encoder on the same call. Encoder must be
// applied to keys and values (including array elements), while `sep`/`eq`
// stay literal.
const tag = (s: string): string => `<${s}>`;
console.log(stringify({ a: "1", b: "2" }, ";", ":", { encodeURIComponent: tag }));
console.log(stringify({ x: "1", y: ["2", "3"] }, ";", ":", { encodeURIComponent: tag }));

// Encoder receives the raw key/value verbatim — verify it sees space, not `+`.
const reveal = (s: string): string => `[${s.length}:${s}]`;
console.log(stringify({ "a b": "c d" }, undefined, undefined, { encodeURIComponent: reveal }));
