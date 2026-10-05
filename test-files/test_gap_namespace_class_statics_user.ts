import * as ns from "./nsstatics/classes.ts";

console.log(ns.Counter.add(2), ns.Derived.add(2), ns.Alias.add(3), ns.default.add(4));
console.log(ns.Counter.value, ns.Counter.value, ns.Counter.reads);
console.log(ns.Derived.value, ns.Derived.reads, ns.Counter.reads);
console.log(ns.Counter.factory(5), ns.Counter.reads);
const method = ns.Counter.plain;
console.log(method(7), method === ns.Counter.plain);
const add = ns.Counter.add;
console.log(add.call(ns.Derived, 3));
console.log(ns.Counter["plain"](8), ns["Counter"].plain(9));
console.log(ns.Counter.plain(...[10]));
(ns.Counter as any).plain = (n: number) => n + 100;
console.log(ns.Counter.plain(11));
