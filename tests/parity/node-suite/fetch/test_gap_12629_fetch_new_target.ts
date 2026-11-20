// Refs #12629: a Fetch base constructed with a custom new.target inherits
// from newTarget.prototype and keeps its native state, the same as a
// `class X extends <base>` instance does through `super()`.
function NT() {}
NT.prototype.extra = function () { return "nt"; };
const bases: [string, any, any[]][] = [
  ["Headers", Headers, [{ a: "1" }]],
  ["Request", Request, ["http://example.test/p", { method: "POST" }]],
  ["Response", Response, ["body", { status: 201 }]],
  ["Blob", Blob, [["ab"]]],
  ["File", File, [["ab"], "f.txt"]],
];
for (const [name, C, args] of bases) {
  const o: any = Reflect.construct(C, args, NT);
  console.log(name, Object.getPrototypeOf(o) === NT.prototype,
    o instanceof (NT as any), o.extra());
  const same: any = Reflect.construct(C, args, C);
  console.log(name + " same", Object.getPrototypeOf(same) === C.prototype);
}
const h: any = Reflect.construct(Headers, [{ a: "1" }], NT);
console.log("Headers get", Headers.prototype.get.call(h, "a"));
class H extends Headers { both() { return this.get("a") + "," + this.get("b"); } }
const sub: any = Reflect.construct(Headers, [{ a: "1" }], H);
sub.set("b", "2");
console.log("H", Object.getPrototypeOf(sub) === H.prototype, sub instanceof H,
  sub instanceof Headers, sub.both(), Object.prototype.toString.call(sub));
const rq: any = Reflect.construct(Request, ["http://example.test/q", { method: "PUT" }], NT);
console.log("Request props", Object.getOwnPropertyDescriptor(Request.prototype, "method")!.get!.call(rq),
  Object.getOwnPropertyDescriptor(Request.prototype, "url")!.get!.call(rq));
// File extends Blob; their attributes are prototype getters, which is how a
// subclass instance (its native handle in its backing) reads them.
console.log("File extends Blob", Object.getPrototypeOf(File.prototype) === Blob.prototype,
  Object.getPrototypeOf(File) === Blob);
for (const [C, keys] of [[Blob, ["type", "size"]], [File, ["name", "lastModified"]]] as [any, string[]][])
  for (const k of keys)
    console.log(C.name, k, typeof Object.getOwnPropertyDescriptor(C.prototype, k)!.get);
class B extends Blob { kind() { return "B"; } }
class F extends File { kind() { return "F"; } }
const b = new B(["abc"], { type: "text/x" });
const f = new F(["abcd"], "a.txt", { type: "text/y", lastModified: 5 });
console.log("B", Object.getPrototypeOf(b) === B.prototype, b instanceof Blob, b.size, b.type, b.kind());
console.log("F", Object.getPrototypeOf(f) === F.prototype, f instanceof File, f instanceof Blob,
  f.name, f.size, f.type, f.lastModified, f.kind(), Object.prototype.toString.call(f));
const plain = new File(["xy"], "p.txt");
console.log("plain File", plain.name, plain.size, plain.type === "", plain instanceof Blob);
console.log("Blob parts", new Blob([b, f]).size);
(async () => {
  const rs: any = Reflect.construct(Response, ["text body"], NT);
  console.log("Response text", await Response.prototype.text.call(rs));
  console.log("F text", await f.text(), await b.text());
  console.log("parts text", await new Blob([f, b]).text());
})();
