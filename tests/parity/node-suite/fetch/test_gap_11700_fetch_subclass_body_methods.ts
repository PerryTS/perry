// #11700: a `class X extends Response/Request` instance's inherited body
// methods must reach the native handle. The by-name dispatch tower found the
// intrinsic no-op-backed `Response.prototype.text`, invoked it, and that
// re-dispatched `sub.text()` by name -- recursing until the call-depth guard
// returned `{}`.
class Sub extends Response {}
class Deeper extends Sub {}
class Overrides extends Response {
  text(): Promise<string> {
    return Promise.resolve("overridden");
  }
}
class Req extends Request {}

async function main() {
  const a = new Sub("sub text", { status: 202 });
  const t = a.text();
  console.log(t instanceof Promise, await t, a.status, a.bodyUsed);

  const b: any = new Sub('{"k":[1,2]}');
  console.log(JSON.stringify(await b.json()));

  const c = new Deeper("deeper");
  console.log(await c.text(), c instanceof Sub, c instanceof Response);

  const d = new Sub("bytes!");
  const buf = await d.arrayBuffer();
  console.log(buf.byteLength, new TextDecoder().decode(buf));

  const e = new Sub("cloned");
  const e2 = e.clone();
  console.log(await e2.text(), await e.text());

  const f = new Sub("via prototype");
  const viaProto = Response.prototype.text.call(f);
  console.log(viaProto instanceof Promise, await viaProto);

  const g = new Overrides("ignored");
  console.log(await g.text());

  const r = new Req("https://example.com/x", { method: "POST", body: "req body" });
  console.log(r.method, await r.text());
}
main();
