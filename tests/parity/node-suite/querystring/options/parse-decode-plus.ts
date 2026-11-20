import { parse } from "node:querystring";

// With a custom decoder, Node rewrites `+` to `%20` before invoking the
// callback. Identity/custom decoders therefore see percent-encoded spaces,
// not raw plus signs.
const tag = (s: string): string => `<${s}>`;
console.log(JSON.stringify(parse("a+b=c+d", undefined, undefined, { decodeURIComponent: tag })));
console.log(JSON.stringify(parse("a+b=c+d&a%20b=c%20d", undefined, undefined, { decodeURIComponent: tag })));
