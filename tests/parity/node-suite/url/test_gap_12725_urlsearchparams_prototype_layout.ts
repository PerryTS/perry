// Gap test for #12725: URLSearchParams.prototype has node's own-key layout:
// the same names and keys in the same order (constructor, size, the WebIDL
// operations, then the symbols), node's descriptor on every key (operations
// and `size` enumerable, `constructor` and the symbols not), and
// `Symbol.iterator` the same function as `entries`. for-in over an instance
// lists the enumerable members in that order.
// `Symbol(nodejs.util.inspect.custom)` is left out of the comparison: perry's
// util.inspect does not render a URLSearchParams through that member yet.
const P: any = URLSearchParams.prototype;
const INSPECT_CUSTOM = Symbol.for("nodejs.util.inspect.custom");
const ownKeys = Reflect.ownKeys(P).filter((k) => k !== INSPECT_CUSTOM);

function desc(k: PropertyKey): string {
  const d: any = Object.getOwnPropertyDescriptor(P, k);
  if (!d) return "none";
  const kind = "value" in d ? typeof d.value : (d.get ? "get" : "") + (d.set ? "set" : "");
  return kind + ":" + (d.enumerable ? "e" : "-") + (d.configurable ? "c" : "-") +
    ("value" in d ? (d.writable ? "w" : "-") : "");
}

console.log("names", JSON.stringify(Object.getOwnPropertyNames(P)));
console.log("ownKeys", JSON.stringify(ownKeys.map(String)));
for (const k of ownKeys) console.log(String(k), desc(k));
console.log("iterator===entries", P[Symbol.iterator] === P.entries);
console.log("tag", P[Symbol.toStringTag], Object.prototype.toString.call(new URLSearchParams()));

const p = new URLSearchParams("a=1&b=2");
const forIn: string[] = [];
for (const k in p) forIn.push(k);
console.log("for-in", JSON.stringify(forIn));
console.log("keys", JSON.stringify(Object.keys(P)));
console.log("enumerable size", P.propertyIsEnumerable("size"), "constructor", P.propertyIsEnumerable("constructor"));
