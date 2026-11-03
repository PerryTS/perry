// #12864 follow-up: a *default* or namespace import of `node:url` reaches its
// helpers through the imported object, so no `module: "url"` marker reaches the
// HIR. Programs using only these helpers therefore linked no `url-engine`, and
// `domainToASCII`/`domainToUnicode` silently fell back to plain IDNA — while the
// same call in a program that also constructs a `URL` was correct.
//
// This file deliberately constructs NO URL: that is the trigger. Compared
// byte-for-byte against `node --experimental-strip-types`.

import url from "node:url";

console.log("domainToASCII a/b:", JSON.stringify(url.domainToASCII("a/b")));
console.log("domainToASCII EXAMPLE.COM:", JSON.stringify(url.domainToASCII("EXAMPLE.COM")));
console.log("domainToUnicode xn--fsq.com:", JSON.stringify(url.domainToUnicode("xn--fsq.com")));
console.log("fileURLToPath:", JSON.stringify(url.fileURLToPath("file:///a%20b")));
console.log(
  "format:",
  JSON.stringify(url.format({ protocol: "https:", host: "x", pathname: "/a b" })),
);
