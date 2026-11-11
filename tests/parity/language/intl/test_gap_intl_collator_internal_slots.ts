// A Collator keeps its state in internal slots: no own keys; `compare` is a
// prototype getter returning one bound function; `resolvedOptions` is a
// prototype method.
function own(o: any): string {
  return Reflect.ownKeys(o).map(String).sort().join(",");
}

const c: any = new Intl.Collator("en", { sensitivity: "base", numeric: true });
console.log("own:", JSON.stringify(own(c)));
console.log("keys:", Object.keys(c).length, JSON.stringify(c));
const forIn: string[] = [];
for (const k in c) forIn.push(k);
console.log("for-in:", JSON.stringify(forIn.join(",")));
console.log("hasOwn:", c.hasOwnProperty("compare"), c.hasOwnProperty("resolvedOptions"),
  c.hasOwnProperty("__intlKind"), "__intlLocale" in c);
console.log("spread:", own({ ...c }), own(Object.assign({}, c)));
console.log("desc:", Object.getOwnPropertyDescriptor(c, "compare") === undefined);

console.log("typeof:", typeof c.compare, c.compare.length, JSON.stringify(c.compare.name));
console.log("stable:", c.compare === c.compare);
const getter = Object.getOwnPropertyDescriptor(Intl.Collator.prototype, "compare")!.get!;
console.log("getter:", getter.call(c) === c.compare);
console.log("proto method:", c.resolvedOptions === Intl.Collator.prototype.resolvedOptions);
console.log("compare:", c.compare("a", "A"), c.compare("a2", "a10"), ["b", "a", "C"].sort(c.compare).join(""));
const r = c.resolvedOptions();
console.log("resolved:", r.locale, r.sensitivity, r.numeric, r.usage);

// `c.compare = f` sets nothing: the inherited accessor has no setter.
c.compare = () => 42;
console.log("store:", c.compare("a", "b"), own(c));
try {
  "use strict";
  (function () { "use strict"; c.compare = () => 42; })();
  console.log("strict store: no throw");
} catch (e: any) {
  console.log("strict store:", e.name);
}

class MyCollator extends Intl.Collator {}
const m: any = new MyCollator("en");
console.log("subclass:", m instanceof Intl.Collator, m.compare("a", "b"), own(m),
  typeof m.resolvedOptions().locale);
try {
  getter.call({});
} catch (e: any) {
  console.log("foreign:", e.name);
}
