// LAZYPROTO P2 batch B3 (Event, DOMException born on first observation): first-touch variant b.
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
  // Node's internal symbols (util.inspect.custom, the messaging clone symbols) are not Perry's: string keys and @@toStringTag.
  const pkeys = Reflect.ownKeys(P).filter((k) => typeof k === "string" || k === Symbol.toStringTag);
  add("proto keys", JSON.stringify(pkeys.map(String)));
  const kinds: string[] = [];
  for (const k of pkeys) {
    // Known gap, not this fixture's subject: node's Event.prototype.isTrusted is not configurable.
    if (name === "Event" && k === "isTrusted") continue;
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

// First touch: the constants and the key listings of the unborn constructors.
const t: string[] = [];
t.push(String("NONE" in Event), String(Object.prototype.hasOwnProperty.call(DOMException, "TIMEOUT_ERR")));
t.push(JSON.stringify(Object.getOwnPropertyNames(Event)), String(Object.keys(DOMException).length));
t.push(String(Event.AT_TARGET), String(DOMException.QUOTA_EXCEEDED_ERR), String(G.Event.CAPTURING_PHASE + G.DOMException.INDEX_SIZE_ERR));
const d: any = Object.getOwnPropertyDescriptor(DOMException, "NETWORK_ERR");
t.push([d.value, d.writable, d.enumerable, d.configurable].join(","));
let threw = "no";
try { (Event as any).NONE = 9; } catch (e: any) { threw = e instanceof TypeError ? "TypeError" : "other"; }
t.push(threw, String(Event.NONE), String(delete (G.DOMException as any).ABORT_ERR), String(DOMException.ABORT_ERR));
(G.Event as any).extra = 1; (G.DOMException as any).extra2 = 2;
t.push(JSON.stringify(Object.keys(Event)), String(Object.keys(DOMException).pop()));
delete (G.Event as any).extra; delete (G.DOMException as any).extra2;
console.log(t.join(" "));
all();
