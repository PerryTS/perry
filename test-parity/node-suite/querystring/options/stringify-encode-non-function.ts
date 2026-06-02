import { stringify } from "node:querystring";

// Mirror of parse-decode-non-function.ts: non-callable `encodeURIComponent`
// slots fall back to Node's percent encoder.
console.log(stringify({ a: "hi there" }, undefined, undefined, { encodeURIComponent: "not-a-fn" as unknown as (s: string) => string }));
console.log(stringify({ a: "hi there" }, undefined, undefined, { encodeURIComponent: null as unknown as (s: string) => string }));
console.log(stringify({ a: "hi there" }, undefined, undefined, { encodeURIComponent: undefined }));
console.log(stringify({ a: "hi there" }, undefined, undefined, {}));
