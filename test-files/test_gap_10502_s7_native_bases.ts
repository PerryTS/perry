// Refs #10502 (S7): a native-base subclass instance reads an inherited
// method through its holder chain (`Sub.prototype` -> `Response.prototype`)
// and must reach the native payload or handle behind the instance, whether
// the call is `sub.text()`, a value read, or `Response.prototype.text.call`.
import { Readable, Transform } from "stream";
import { EventEmitter } from "events";
class R extends Response {}
class Q extends Request {}
class U extends URL {}
class USP extends URLSearchParams {}
class M extends Map<string, number> {}
class S extends Set<number> {}
class E extends Error {}
class P<T> extends Promise<T> {}
class A extends Array<number> {}
class Em extends EventEmitter {}
class AC extends AbortController {}
class Rd extends Readable { _read() {} }
class Tr extends Transform {
  _transform(chunk: any, _e: string, cb: (e: any, d?: any) => void) { cb(null, String(chunk).toUpperCase()); }
}
async function t(f: () => Promise<void>) { try { await f(); } catch (e: any) { console.log("ERR", e && e.message); } }
async function main() {
  await t(async () => {
    const r = new R("resp");
    console.log("R", await r.text(), r.bodyUsed, r.status, r instanceof Response);
  });
  await t(async () => {
    const rj = new R('{"a":1}');
    console.log("Rj", JSON.stringify(await rj.json()));
  });
  await t(async () => {
    const rb = new R("buf");
    console.log("Rb", (await rb.arrayBuffer()).byteLength);
  });
  await t(async () => {
    const q = new Q("https://e.com/p?x=1", { method: "PUT", body: "qb" });
    console.log("Q", q.method, q.url, await q.text(), q.headers.get("content-type"));
  });
  await t(async () => {
    const u = new U("https://x.org:8080/a/b?q=1#h");
    console.log("U", u.host, u.pathname, u.searchParams.get("q"), u.toString(), u.toJSON(), String(u));
  });
  await t(async () => {
    const usp = new USP("a=1&b=2");
    usp.append("c", "3");
    console.log("USP", usp.get("b"), usp.toString(), usp.has("c"));
  });
  await t(async () => {
    const m = new M(); m.set("k", 1);
    console.log("M", m.get("k"), m.has("k"), m.size, [...m.keys()].join());
  });
  await t(async () => {
    const s = new S([1, 2]); s.add(3);
    console.log("S", s.has(3), s.size, [...s].join());
  });
  await t(async () => {
    const e = new E("boom");
    console.log("E", e.message, e.toString(), e instanceof Error, typeof e.stack);
  });
  await t(async () => {
    const p = new P<number>((res) => res(5));
    console.log("P", await p.then((v) => v + 1), p instanceof Promise);
  });
  await t(async () => {
    const a = new A(); a.push(1, 2);
    console.log("A", a.length, a.map((x) => x * 2).join());
  });
  await t(async () => {
    const em = new Em(); let got = 0; em.on("x", (v: number) => { got = v; }); em.emit("x", 9);
    console.log("Em", got, em.listenerCount("x"));
  });
  await t(async () => {
    const ac = new AC(); ac.abort();
    console.log("AC", ac.signal.aborted);
  });
  await t(async () => {
    const rd = new Rd(); rd.push("r1"); rd.push(null);
    let rs = ""; for await (const c of rd) rs += c;
    console.log("Rd", rs);
  });
  await t(async () => {
    const tr = new Tr(); let ts = ""; tr.on("data", (d: any) => { ts += d; });
    tr.write("ab"); tr.end();
    await new Promise((res) => tr.on("end", res));
    console.log("Tr", ts);
  });
  await t(async () => {
    const viaProto = Response.prototype.text.call(new R("vp"));
    console.log("viaProto", await viaProto);
  });
  await t(async () => {
    console.log("ident", (new R("i") as any).text === Response.prototype.text, Object.getPrototypeOf(R.prototype) === Response.prototype);
  });
}
main();
