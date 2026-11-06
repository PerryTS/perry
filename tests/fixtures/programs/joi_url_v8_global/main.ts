// V8-fallback jsruntime now defines `URL` and `URLSearchParams` as globals
// so packages routed through the V8 fallback (anything in node_modules not
// listed in `perry.compilePackages`) can read `URL.prototype` at module-init
// time. This was the `import Joi from "joi"` crash path: joi's transitive
// dep `@hapi/hoek/lib/types.js` reads `URL.prototype` as a top-level
// expression and v8 (without `deno_url`) threw `ReferenceError`.
import * as pkg from "url-init-pkg";

console.log("urlProto type:", typeof pkg.urlProto);
const parsed = pkg.parseHref("https://example.com:8443/api/v1?x=1&y=2");
console.log("protocol:", parsed.protocol);
console.log("hostname:", parsed.hostname);
console.log("pathname:", parsed.pathname);
console.log("search:", parsed.search);
console.log("search-stringify:", pkg.searchToString({ a: "1", b: "2" }));
