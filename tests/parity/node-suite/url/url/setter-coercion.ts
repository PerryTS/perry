function show(label: string, fn: () => string) {
  try {
    console.log(label + ":", fn());
  } catch (err: any) {
    console.log(label + " err:", err?.name, err?.code || "no-code", err?.message);
  }
}

show("pathname bool", () => {
  const u = new URL("https://example.com/base?x=1#old");
  u.pathname = false as any;
  return u.href;
});
show("search null", () => {
  const u = new URL("https://example.com/base?x=1#old");
  u.search = null as any;
  return u.href + " params=" + u.searchParams.toString();
});
show("hash undefined", () => {
  const u = new URL("https://example.com/base?x=1#old");
  u.hash = undefined as any;
  return u.href;
});
show("username object", () => {
  const u = new URL("https://example.com/base");
  u.username = { toString() { return "u ser"; } } as any;
  return u.href;
});
show("href symbol", () => {
  const u = new URL("https://example.com/base");
  u.href = Symbol("x") as any;
  return u.href;
});
