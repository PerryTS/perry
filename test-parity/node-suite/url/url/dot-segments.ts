// #12864: WHATWG removes `.` and `..` path segments, so `pathname` (and the
// `href` derived from it) never carries them. Every case below is compared
// byte-for-byte against `node --experimental-strip-types`.
//
// The asymmetry is the interesting part: a dot segment is dropped, but when it
// is the LAST thing in the path the parser appends an empty segment instead of
// nothing, so `/a/..` is `/` while `/a/b/..` is `/a/`.

// The issue's own reproduction, verbatim.
const i = 0;
const u = new URL("https://user:pw@example.com:8443/a/b/../c?q=1#f");
console.log(i, u.pathname, new URL("https://x/a/./b/../../c").pathname);

// Special-scheme paths: plain, trailing, double-separator and contained-dot.
for (const input of [
  "https://x/a/b/../c",
  "https://x/a/..",
  "https://x/a/.",
  "https://x/..",
  "https://x/./",
  "https://x/a/../",
  "https://x/a/./",
  "https://x/a//b",
  "https://x//a/../b",
  "https://x/a/.../b",
  "https://x/a/b/../../..",
  "https://x/a/b/c/../../d/",
]) {
  const parsed = new URL(input);
  console.log(JSON.stringify([input, parsed.pathname, parsed.href]));
}

// The dot segments are recognized in their percent-encoded spellings too
// (ASCII case-insensitive); `%2e%2e%2e` is not a double-dot segment.
for (const input of [
  "https://x/a/%2e%2e/b",
  "https://x/a/%2E./b",
  "https://x/a/my%2e/b",
  "https://x/a/./%2e/b",
  "https://x/a/b/%2e%2e/%2e%2e/%2e%2e/c",
  "https://x/a/%2e%2e%2e/b",
]) {
  const parsed = new URL(input);
  console.log(JSON.stringify([input, parsed.pathname, parsed.href]));
}

// Non-special hierarchical schemes share the rule; an opaque path is not a
// segment list and keeps its text.
for (const input of [
  "foo://a/b/../c",
  "foo://a/b/./c",
  "mailto:a/../b",
  "data:text/plain,a/../b",
]) {
  const parsed = new URL(input);
  console.log(JSON.stringify([input, parsed.pathname]));
}

// `file:` URLs remove dot segments, except that a path which is only a Windows
// drive letter is never shortened.
for (const input of [
  "file:///a/b/../c",
  "file:///a/./b/",
  "file:///a/b/../../../c",
  "file:///C:/..",
  "file:///C:/a/../..",
  "file:///C:/./a",
]) {
  const parsed = new URL(input);
  console.log(JSON.stringify([input, parsed.pathname, parsed.href]));
}

// Resolution against a base goes through the same path state.
const base = new URL("https://x/a/b/");
for (const relative of ["../c", "/a/../c", "./d", "../../e"]) {
  const resolved = new URL(relative, base);
  console.log(JSON.stringify([relative, resolved.pathname, resolved.href]));
}

// Assigning `pathname` runs the same rules as construction.
const assigned = new URL("https://x/a/b");
assigned.pathname = "/a/../z";
console.log(JSON.stringify(["assigned", assigned.pathname, assigned.href]));
assigned.pathname = "q/./r/../s";
console.log(JSON.stringify(["assigned-relative", assigned.pathname, assigned.href]));
assigned.pathname = "/a/%2e%2e/t";
console.log(JSON.stringify(["assigned-encoded", assigned.pathname, assigned.href]));

// `URL.parse` is the same constructor path.
const parsedStatic = URL.parse("https://x/a/../b");
console.log(JSON.stringify(["URL.parse", parsedStatic!.pathname, parsedStatic!.href]));
