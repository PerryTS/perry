const t = Object.freeze(new EventTarget());
let hits = 0;
try {
  t.addEventListener("x", () => { hits++; });
  t.dispatchEvent(new Event("x"));
  console.log("frozen target ok", hits);
} catch (e: any) { console.log("frozen target threw", e?.constructor?.name); }
const s = Object.seal(new EventTarget());
try { s.addEventListener("y", () => { hits++; }); s.dispatchEvent(new Event("y")); console.log("sealed ok", hits); } catch (e: any) { console.log("sealed threw", e?.constructor?.name); }
const ac = new AbortController();
Object.freeze(ac);
try { ac.abort("r"); console.log("frozen ctrl abort", ac.signal.aborted, ac.signal.reason); } catch (e: any) { console.log("frozen ctrl threw", e?.constructor?.name); }
const et2 = new EventTarget();
console.log("syms", Object.getOwnPropertySymbols(et2).map(String).join(","));
console.log("keys", JSON.stringify(Object.keys(et2)), JSON.stringify(et2), String(et2));
const ev = new Event("e");
console.log("evsyms", Object.getOwnPropertySymbols(ev).map(String).join(","), Object.keys(ev).join(","));
console.log(Object.getOwnPropertyNames(ev).join(","));
class Bus extends EventTarget { n = 1; }
const b = new Bus();
b.addEventListener("z", (e) => console.log("bus got", e.type, e.target === b));
b.dispatchEvent(new Event("z"));
console.log("bus keys", Object.keys(b).join(","), Object.getOwnPropertySymbols(b).length === Object.getOwnPropertySymbols(et2).length);
const o = Object.setPrototypeOf({}, EventTarget.prototype);
try { o.addEventListener("q", () => {}); console.log("borrowed ok"); } catch (e: any) { console.log("borrowed threw", e?.constructor?.name); }
console.log(new DOMException("m", "AbortError").name, new DOMException("m", "AbortError").code, new DOMException("m") instanceof Error);
const sig = AbortSignal.abort("why");
console.log("static abort", sig.aborted, sig.reason);
const m = new Map([[new EventTarget(), 1], [new EventTarget(), 2]]);
console.log("map", m.size);
