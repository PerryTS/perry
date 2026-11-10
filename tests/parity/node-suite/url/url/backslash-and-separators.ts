// #12864 follow-up: the WHATWG shape of a *special* URL besides dot segments.
// For `http:`/`https:`/`ws:`/`wss:`/`ftp:`/`file:`, `\` is a path separator and
// an authority terminator, the `//` is optional, and a run of separators after
// the scheme is skipped. `file:` is the special one: extra separators are path,
// its host may be empty, and a lone Windows drive letter keeps its `:` (and
// accepts the `|` spelling). A non-special URL keeps backslashes literally, and
// a backslash in a non-special *host* makes the URL invalid.
//
// Compared byte-for-byte against `node --experimental-strip-types`.

function show(label: string, u: URL) {
  console.log(JSON.stringify([label, u.host, u.pathname, u.search, u.hash, u.href]));
}

function attempt(input: string) {
  try {
    show(input, new URL(input));
  } catch (e: any) {
    console.log(JSON.stringify([input, "THROW", e.code]));
  }
}

// `\` as the authority terminator and path separator, plus the skipped run.
for (const input of [
  "http://x\\a\\b",
  "http://x/a\\b\\..\\c",
  "http://x/a\\..\\b",
  "http://x\\a",
  "http://\\y",
  "http://\\\\y",
  "https:///x",
  "https:////x",
  "ws:////x",
  "http://x//y",
  "http://x\\",
  "ftp:x/y",
]) {
  attempt(input);
}

// The query and fragment encode sets do not encode `\`, so those keep theirs
// while the path folds.
attempt("https://x\\y\\..\\z?q=\\a#f\\b");
attempt("https://x/?q=\\a");
attempt("https://x/#f\\b");

// A special scheme is hierarchical even without the `//`; a non-special one is
// an opaque path. Empty hosts are only allowed for `file:`.
for (const input of ["https:x", "https:/x", "https:\\x", "https:", "http:/", "http:\\\\", "file:"]) {
  attempt(input);
}
attempt("mailto:");
attempt("data:text/plain,a/../b");

// `file:` separators: `file:\x` is a hostless path, `file:\\x` carries a
// host, and extra separators belong to the path. Its host is an ordinary
// WHATWG host, so it canonicalizes like any other special scheme.
for (const input of [
  "file:\\x",
  "file:\\\\x",
  "file:\\\\\\\\x",
  "file:/x",
  "file:x",
  "file://",
  "file:////x",
  "file://server\\share\\x",
  "file://SERVER/x",
  "file://123/x",
  "file://LOCALHOST/x",
  "file:///a\\b\\..\\c",
]) {
  attempt(input);
}

// The Windows drive letter: `|` is accepted for `:`, only in the first path
// segment, only for `file:`, and that lone drive is never shortened.
for (const input of [
  "file://C|/x",
  "file:///C|/x",
  "file:///z|/x",
  "file:///C|",
  "file:///C|/..",
  "file:///C|/x/../y",
  "file:///ab|/x",
]) {
  attempt(input);
}

// Non-special schemes: the path keeps backslashes, the host may not have one.
for (const input of ["foo://a/b\\c", "foo://a\\b"]) {
  attempt(input);
}

// Validation follows the same shape.
for (const input of [
  "https:x",
  "https:/x",
  "http://\\y",
  "http://\\\\",
  "foo://a\\b",
  "foo://a/b\\c",
  "file:\\x",
  "file:////x",
  "mailto:",
  "https:",
]) {
  console.log(JSON.stringify(["canParse", input, URL.canParse(input)]));
}

// Assigning `pathname` re-runs the same path states.
const https = new URL("https://x/a");
for (const value of ["/b\\c", "\\a\\b", "b\\..\\c", "//a/../b"]) {
  https.pathname = value;
  show(`set https ${value}`, https);
}

const file = new URL("file:///a/b");
for (const value of ["/C|/x", "/x\\..\\y", "abc", ""]) {
  file.pathname = value;
  show(`set file ${value}`, file);
}

const nonSpecial = new URL("foo://x/a");
for (const value of ["/b\\c"]) {
  nonSpecial.pathname = value;
  show(`set foo ${value}`, nonSpecial);
}
