// #12864 follow-up: the constructor applies the WHATWG path percent-encode
// set (it previously only encoded non-ASCII), so `pathname` and `href` escape
// space, `"`, `<`, `>`, `` ` ``, `{`, `}`, `^` and C0 controls the way Node
// does. `%` is not in the set, so existing escapes and stray `%` pass through,
// and tab/LF/CR are stripped from the input rather than encoded.
// Compared byte-for-byte against `node --experimental-strip-types`.

for (const input of [
  "https://x/a b<c",
  'https://x/a"b',
  "https://x/a`b{c}d",
  "https://x/a^b",
  "https://x/a|b",
  "https://x/a[b]",
  "https://x/a%20b",
  "https://x/a%zzb",
  "https://x/ä",
  "https://x/a\tb",
  "https://x/a\nb",
  "https://x/a\rb",
  "https://x/a\u0001b",
  "https://x/a\u007fb",
  "file:///a b/c d",
  "file:///a%C3%A9b",
  "foo://a/b c",
]) {
  const u = new URL(input);
  console.log(JSON.stringify([input, u.pathname, u.href]));
}

// An opaque URL has no segment list, so assigning `pathname` does nothing.
for (const start of ["mailto:a/../b", "data:text/plain,x", "urn:isbn:1", "about:blank"]) {
  const u = new URL(start);
  u.pathname = "/x/../y z";
  console.log(JSON.stringify([start, u.pathname, u.href]));
}

// A hierarchical one still takes the write, encoded and normalized.
const http = new URL("https://x/a");
http.pathname = "/p q^r";
console.log(JSON.stringify([http.pathname, http.href]));

const file = new URL("file:///a");
file.pathname = "/p q";
console.log(JSON.stringify([file.pathname, file.href]));

const nonSpecial = new URL("foo:/a");
nonSpecial.pathname = "/p q";
console.log(JSON.stringify([nonSpecial.pathname, nonSpecial.href]));
