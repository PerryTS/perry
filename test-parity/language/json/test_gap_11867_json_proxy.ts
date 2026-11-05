// JSON.stringify must use a Proxy's internal operations, including nested values.
const target = { a: 1, b: [2, 3] };
const p = new Proxy(target, {});
console.log(JSON.stringify(p));
console.log(JSON.stringify({ wrap: p }));
console.log(JSON.stringify([p, p]));
console.log(JSON.stringify(new Proxy(p, {})));
console.log(JSON.stringify(new Proxy([1, { x: 2 }], {})));
console.log(JSON.stringify(p, null, 2));
console.log(JSON.stringify(p, (key, value) => typeof value === "number" ? value * 10 : value));
console.log(JSON.stringify(p, ["b", "a"]));

const events: string[] = [];
const sym = Symbol("hidden");
const trapped = new Proxy({ a: 1, b: 2, hidden: 3, [sym]: 4 }, {
  ownKeys(t) {
    events.push("keys");
    return ["b", "hidden", "a", sym];
  },
  getOwnPropertyDescriptor(t, k) {
    events.push("desc:" + String(k));
    return { value: Reflect.get(t, k), enumerable: k !== "hidden", configurable: true };
  },
  get(t, k, receiver) {
    events.push("get:" + String(k));
    return k === "a" ? 10 : Reflect.get(t, k, receiver);
  }
});
console.log(JSON.stringify(trapped));
console.log(events.join(","));
events.length = 0;
console.log(JSON.stringify(trapped, ["hidden", "a"]));
console.log(events.join(","));

const arrayEvents: string[] = [];
const arrayProxy = new Proxy([1, 2, 3], {
  get(t, k, receiver) {
    arrayEvents.push(String(k));
    if (k === "length") return 2;
    if (k === "1") return 9;
    return Reflect.get(t, k, receiver);
  },
  ownKeys() { throw new Error("array serialization must not enumerate keys"); }
});
console.log(JSON.stringify(arrayProxy));
console.log(arrayEvents.join(","));

const withJSON = new Proxy({ n: 7, toJSON(key: string) { return key + ":" + this.n; } }, {});
console.log(JSON.stringify(withJSON));
console.log(JSON.stringify({ wrap: withJSON }));
console.log(JSON.stringify([withJSON]));
const omitted = new Proxy({ toJSON() { return undefined; } }, {});
console.log(JSON.stringify(omitted));
console.log(JSON.stringify({ omitted }));
console.log(JSON.stringify([omitted]));

const repeated = new Proxy({ n: 1 }, {});
console.log(JSON.stringify({ first: repeated, second: repeated }));
const cyclic: any = {};
const cycleProxy = new Proxy(cyclic, {});
cyclic.self = cycleProxy;
try { JSON.stringify(cycleProxy); } catch (e) { console.log("cycle", e instanceof TypeError); }
const revocable = Proxy.revocable({ a: 1 }, {});
revocable.revoke();
try { JSON.stringify(revocable.proxy); } catch (e) { console.log("revoked", e instanceof TypeError); }
const throwing = new Proxy({}, { ownKeys() { throw new Error("ownKeys failed"); } });
try { JSON.stringify(throwing); } catch (e) { console.log(e.message); }
console.log(JSON.stringify(p));

console.log(JSON.stringify(new Proxy({}, {})));
console.log(JSON.stringify(new Proxy([], {})));
let calls = 0;
const returnsSelf = new Proxy({ a: 4, toJSON() { calls++; return this; } }, {});
console.log(JSON.stringify(returnsSelf), calls);
const returnedProxy = new Proxy({ a: 5, toJSON() { throw new Error("unexpected second toJSON"); } }, {});
const returnsProxy = new Proxy({ toJSON() { return returnedProxy; } }, {});
console.log(JSON.stringify(returnsProxy));
console.log(JSON.stringify({ item: returnsProxy }));
console.log(JSON.stringify(0, (key, value) => key === "" ? returnedProxy : value));
console.log(JSON.stringify(p, function (key, value) {
  if (key === "a") console.log("holder", this === p);
  return value;
}, 2));
const descriptorThrows = new Proxy({ a: 1 }, {
  getOwnPropertyDescriptor() { throw new Error("descriptor failed"); }
});
try { JSON.stringify(descriptorThrows); } catch (e) { console.log(e.message); }
const getThrows = new Proxy({ a: 1 }, {
  get(t, k) { if (k === "a") throw new Error("get failed"); return Reflect.get(t, k); }
});
try { JSON.stringify(getThrows); } catch (e) { console.log(e.message); }
