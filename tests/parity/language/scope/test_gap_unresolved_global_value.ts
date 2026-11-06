// Rule G: an unresolved identifier reads what the program stored on the
// global object under that name; nothing stored = ReferenceError.
class V3 {
  x: number; y: number;
  constructor(x: number, y: number) { this.x = x; this.y = y; }
  add(v: V3) { this.x += v.x; this.y += v.y; return this; }
}
let made = 0;
class Obj {
  position = new V3(0, 0);
  constructor() {
    made++;
    if (typeof DEVTOOLS !== "undefined") { DEVTOOLS.observe(this); }
    if (typeof window !== "undefined") { window.__REV__ = "1"; }
  }
}
function run(tag: string) {
  const o = new Obj();
  const a = new V3(1, 2), b = new V3(4, 5);
  for (let i = 0; i < 3; i++) a.add(b);
  console.log(tag, a.x, a.y, o.position.x, made, typeof DEVTOOLS, typeof window);
}
run("none");
try { console.log(DEVTOOLS); } catch (e: any) { console.log("read", e.name); }
let seen: any[] = [];
(globalThis as any).DEVTOOLS = { observe(o: any) { seen.push(o.position.x); } };
run("named");
(globalThis as any)["DEV" + "TOOLS".slice(0)] = { observe(o: any) { seen.push("c" + o.position.y); } };
run("computed");
Object.defineProperty(globalThis, "DEVTOOLS", { configurable: true, get() { return { observe(o: any) { o.position.constructor.prototype.add = function (this: any) { delete this.x; this.q = 1; return this; }; } }; } });
run("getter");
console.log(seen.join(","));
