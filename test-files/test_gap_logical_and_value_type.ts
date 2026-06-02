// #3527: `var x = <boolean-expr> && <object/any-value>` must keep `x` usable as
// the right-hand value, not be mistyped as Boolean. Mirrors object-inspect's
// `var hasMap = typeof Map === "function" && Map.prototype; hasMap && d && d.get`.
const hasMap = typeof Map === "function" && Map.prototype;
console.log("hasMap typeof:", typeof hasMap);

const d: any = null;
const mapSize = hasMap && d && typeof d.get === "function" ? d.get : "none";
console.log("mapSize:", mapSize);

// && returns the right operand's value when left is truthy
const cfg = true && { port: 3000, host: "localhost" };
console.log("cfg.port:", (cfg as any).port, "host:", (cfg as any).host);

// || returns the right value when left is falsy
const fallback = (undefined as any) || { name: "default" };
console.log("fallback.name:", fallback.name);

// chained && guarding a property access short-circuits correctly
const obj: any = { inner: { val: 42 } };
const got = obj && obj.inner && obj.inner.val;
console.log("got:", got);
const missing: any = { inner: null };
const safe = missing && missing.inner && missing.inner.val;
console.log("safe:", safe);
