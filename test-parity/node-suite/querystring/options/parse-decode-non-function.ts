import { parse } from "node:querystring";

// Non-callable `decodeURIComponent` slots must fall back to the built-in
// percent decoder. Node coerces silently; Perry's runtime path skips the
// closure call when `is_closure_ptr` returns false.
console.log(JSON.stringify(parse("a=hi%20there", undefined, undefined, { decodeURIComponent: "not-a-fn" as unknown as (s: string) => string })));
console.log(JSON.stringify(parse("a=hi%20there", undefined, undefined, { decodeURIComponent: null as unknown as (s: string) => string })));
console.log(JSON.stringify(parse("a=hi%20there", undefined, undefined, { decodeURIComponent: undefined })));
console.log(JSON.stringify(parse("a=hi%20there", undefined, undefined, {})));
