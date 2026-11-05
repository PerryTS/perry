// node:url legacy `url.parse` / `url.format` — perry's implementation was a
// hand-rolled approximation that computed the wrong thing structurally: a plain
// relative path had its first segment stolen as a hostname, there were no
// hostless/slashed protocol tables, IPv6 brackets were never stripped from
// `hostname`, `pathname` was invented as "/" for a bare query/hash, and `href`
// was just the input string rather than the result of format(). See #6375.
//
// Validated byte-for-byte against `node --experimental-strip-types`.

import url from "node:url";

const P = (u: string, q?: boolean, s?: boolean) =>
  console.log(u.padEnd(38), JSON.stringify(url.parse(u, q as any, s as any)));

// a relative path is NOT an authority
P("a/b/c");
P("//some_path");
// bare query / hash have no pathname
P("?a=1");
P("#h");
// hostless vs slashed protocols
P("mailto:a@b.com");
P("file:///a/b");
// IPv6: host keeps the brackets, hostname does not
P("http://[::1]:8080/x");
P("http://[2001:db8::1]/");
// href comes from format(), so the root path appears
P("http://example.com");
P("http://example.com?");
// auth, ports, the lot
P("http://u:p@host:8080/p/q?x=1#frag");
// hostPattern makes this an authority even without slashesDenoteHost
P("//user:pass@example.com:8000/foo/bar?baz=quux#frag");
// the last `@` decides the auth boundary; format escapes the inner one
P("http://a@b@c/");
// parseQueryString
P("http://example.com/p?a=1&b=2", true);

// url.format(string) parses first, so it normalizes
console.log("format(str)", JSON.stringify(url.format("http://example.com?")));

// domainToASCII / domainToUnicode
for (const d of ["a@b", "a/b", "a.b", "xn--fsq.com"]) {
  console.log(
    "domain",
    JSON.stringify(d),
    JSON.stringify(url.domainToASCII(d)),
    JSON.stringify(url.domainToUnicode(d)),
  );
}
