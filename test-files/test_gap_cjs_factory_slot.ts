// The CommonJS wrapper keeps each module's factory in the record's internal
// slot, never a property: user code cannot see or stringify it, and a reload
// after `delete require.cache[...]` still runs the module again. Functions a
// CommonJS module defines still stringify exactly as Node prints them.
import { createRequire } from "node:module";

const require = createRequire(import.meta.url);
const modPath = require.resolve("./fixtures/cjs_factory_slot/mod.cjs");
const m = require("./fixtures/cjs_factory_slot/mod.cjs");

console.log("fn:", m.add.toString());
console.log("arrow:", String(m.arrow));
console.log("class:", `${m.Klass}`);
console.log("method:", Function.prototype.toString.call(m.Klass.prototype.method));
console.log("inside:", m.selfText);
console.log("named by key:", String(m[["ad", "d"].join("")]) === m.add.toString());

const rec = require.cache[modPath]!;
const hidden = (o: object) =>
  !Object.keys(o).some((k) => k.includes("factory")) &&
  !Object.getOwnPropertyNames(o).some((k) => k.includes("factory")) &&
  !("__perry_cjs_factory" in o);
console.log("record hides factory:", hidden(rec), rec === m.self);
console.log("module view inside:", m.keysSeeFactory, m.factoryType);
console.log("exports reassigned:", rec.exports === m, m.loads);

const a = require("./fixtures/cjs_factory_slot/cycle-a.cjs");
console.log("cycle:", a.fromB, a.a2);

delete require.cache[modPath];
const m2 = require("./fixtures/cjs_factory_slot/mod.cjs");
console.log("reload:", m2 !== m, m2.loads, require.cache[modPath]!.exports === m2);
console.log("reload hides factory:", hidden(require.cache[modPath]!));
console.log("reload fn:", m2.add.toString() === m.add.toString());
delete require.cache[modPath];
delete (globalThis as any).__cfsLoads;
