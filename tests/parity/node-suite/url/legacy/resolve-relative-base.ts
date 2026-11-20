// `url.resolve` is the legacy resolver: `from` need not be an absolute URL, and
// Perry answered it through the WHATWG helper, which models absolute URLs only.
// `url.resolve("/a/b", "../c")` came back as `///c` (a scheme-less base grew a
// `//host` prefix it never had), and a relative base lost the climb above its own
// root. The resolver now walks segments like Node's `resolveObject`.
//
// Compared byte-for-byte against `node --experimental-strip-types`.

import url from "node:url";

const pairs: [string, string][] = [
  // The reported shape: a path-only base.
  ["/a/b", "../c"],
  ["/a/b", "/c"],
  ["/a/b", "c"],
  ["/a/b", "?q=1"],
  ["/a/b", "#h"],
  ["/a/b", ""],
  ["/a/b/", ".."],
  ["/a/b/", "./c"],
  ["/a/b", "//x/y"],
  ["/a/b", "../../../c"],
  ["/a/b", "?q#h"],
  // A relative base keeps its own prefix, and may climb above its root.
  ["a/b", "c"],
  ["a/b", "../c"],
  ["a/b", "../../c"],
  ["x/a/b", "../../c"],
  ["a/b/c", "../d"],
  ["a", "../b"],
  ["./a/b", "c"],
  ["./a/b", "../c"],
  ["a/b/", "../c"],
  ["a/b", "."],
  ["a/b", "./"],
  ["a/b", "/c"],
  ["", "c"],
  // An absolute base keeps its scheme and authority.
  ["https://x/a/b", "../c"],
  ["https://x/a/b/", "./c"],
  ["https://x/a/b", "/c"],
  ["https://x/a/b?x=1#h", "?q=2"],
  ["https://x/a/b?x=1#h", "#h2"],
  ["https://x/a/b?x=1#h", "c"],
  ["https://x/a/b", "//y/z"],
  ["https://x/a/b", "https://z/q"],
  ["https://x/a/b", "./"],
  ["file:///a/b", "../c"],
  ["file:///a/b", "c"],
  // A hostless or opaque scheme keeps its own shape.
  ["foo:/a/b", "../c"],
  ["foo:/a/b", "c"],
  ["foo:a/b", "c"],
  ["mailto:a/b", "c"],
];

for (const [from, to] of pairs) {
  console.log(JSON.stringify([from, to, url.resolve(from, to)]));
}
