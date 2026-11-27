// `new URL` failures throw Node's TypeError("Invalid URL") with own `code`
// (ERR_INVALID_URL), `input`, and `base` when a base was given; and a URL
// without a `//` authority has no host: host, hostname and port read "".

function fail(fn: () => unknown): void {
  try {
    fn();
    console.log("no throw");
  } catch (err) {
    const e = err as TypeError & { code?: string; input?: string; base?: string };
    console.log(e instanceof TypeError, e.name, JSON.stringify(e.message), e.code, e.input, e.base, Object.keys(e).join(","));
  }
}

fail(() => new URL("x"));
fail(() => new URL("/a", "not a url"));
fail(() => new URL("http://a b/"));
fail(() => new URL("x", "mailto:y"));
fail(() => {
  const u = new URL("http://h/");
  u.href = "nope";
});
console.log(URL.canParse("x"), URL.parse("x"));

for (const input of ["web+demo:/.//not-a-host/", "foo:/path", "mailto:a@b", "foo://host:8/p", "file:///etc/hosts"]) {
  const u = new URL(input);
  console.log(JSON.stringify([u.href, u.host, u.hostname, u.port, u.pathname, u.origin]));
}
