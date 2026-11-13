// LAZYPROTO P2 batch B3 (Event, DOMException born on first observation): first-touch variant c.
// The first observation differs per variant; the dump after it must be node's in every one.
const SKIP: string[] = [];

const G: any = globalThis as any;
const fam: string[] = ["Event", "DOMException"];
function kind(d: any): string {
  return "value" in d ? "data" : "accessor(" + (d.get ? "get " + d.get.name : "-") + "," + (d.set ? "set" : "-") + ")";
}
function dump(name: string): string[] {
  const out: string[] = [];
  const add = (fact: string, v: string) => { if (!SKIP.includes(name + " " + fact)) out.push(name + " " + fact + ": " + v); };
  const C = G[name];
  add("typeof", typeof C);
  add("ctor keys", JSON.stringify(Reflect.ownKeys(C).map(String)));
  add("length,name", C.length + "," + C.name);
  const statics: string[] = [];
  for (const k of Reflect.ownKeys(C)) {
    const d: any = Object.getOwnPropertyDescriptor(C, k)!;
    statics.push(String(k) + ":" + kind(d) + "," + d.enumerable + "," + d.configurable + (("writable" in d) ? "," + d.writable : "") + (typeof d.value === "number" ? "=" + d.value : ""));
  }
  add("ctor members", statics.join(" "));
  const pd: any = Object.getOwnPropertyDescriptor(C, "prototype");
  const P = C.prototype;
  add("prototype desc", [pd && pd.value === P, pd && pd.writable, pd && pd.enumerable, pd && pd.configurable].join(","));
  add("proto keys", JSON.stringify(Reflect.ownKeys(P).map(String)));
  const kinds: string[] = [];
  for (const k of Reflect.ownKeys(P)) {
    const d: any = Object.getOwnPropertyDescriptor(P, k)!;
    kinds.push(String(k) + ":" + kind(d) + "," + d.enumerable + "," + d.configurable + (("writable" in d) ? "," + d.writable : ""));
  }
  add("proto members", kinds.join(" "));
  add("constructor link", String(P.constructor === C));
  const pp = Object.getPrototypeOf(P);
  add("proto parent", pp === Object.prototype ? "Object.prototype" : pp === null ? "null" : String(pp.constructor && pp.constructor.name));
  const cp = Object.getPrototypeOf(C);
  add("ctor parent", cp === Function.prototype ? "Function.prototype" : String(cp && cp.name));
  add("toStringTag", String(P[Symbol.toStringTag]));
  add("for-in ctor", (() => { const ks: string[] = []; for (const k in C) ks.push(k); return ks.join(","); })());
  return out;
}
function all(): void {
  const lines: string[] = [];
  for (const n of fam) for (const l of dump(n)) lines.push(l);
  console.log(lines.join("\n"));
}

// First touch: children, subclasses and runtime-made instances.
const t: string[] = [];
const ce = new CustomEvent("x", { detail: 5 });
t.push(String(ce instanceof Event), String(Object.getPrototypeOf(CustomEvent.prototype) === Event.prototype), String(Object.getPrototypeOf(CustomEvent) === Event));
class MyEvent extends Event { extra = 7; constructor() { super("mine"); } }
const me = new MyEvent();
t.push(me.type, String(me.extra), String(me instanceof Event), String(Object.getPrototypeOf(MyEvent) === Event), String(MyEvent.BUBBLING_PHASE));
class MyDom extends DOMException { constructor() { super("m", "SyntaxError"); } }
const md = new MyDom();
t.push(md.name, String(md.code), String(md instanceof DOMException), String(md instanceof Error), String(MyDom.SYNTAX_ERR));
let cloned = "none";
try { structuredClone(() => 1); } catch (e: any) { cloned = e.name + "," + (e instanceof DOMException) + "," + e.code + "," + (e.constructor === DOMException); }
t.push(cloned);
const reason: any = AbortSignal.abort().reason;
t.push(reason.name, String(reason instanceof DOMException), String(reason.code === DOMException.ABORT_ERR));
const target = new EventTarget();
let seen = "";
target.addEventListener("go", (e: any) => { seen = e.type + "," + (e instanceof Event) + "," + (e.eventPhase === Event.AT_TARGET); });
target.dispatchEvent(new Event("go"));
t.push(seen);
console.log(t.join(" "));
all();
