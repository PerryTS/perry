// A spread argument to a JS built-in static or global function must expand
// into separate arguments. perry folded the spread operand in as ONE argument
// holding the whole array, so `Object.assign({}, ...sources)` copied the
// array's indices instead of each source's properties. prettier's option
// table is built exactly that way, and `prettier.format` threw
// `Unexpected type undefined`.

const plugins: any[] = [{ name: "a" }, { name: "b", options: { x: { type: "int" } } }];
const core = { y: { type: "string" } };
const merged = Object.assign({}, ...plugins.map(({ options: o }) => o), core);
console.log("prettier-shape", JSON.stringify(merged));
for (const [name, info] of Object.entries(merged)) {
  console.log("option", name, (info as any).type);
}

const srcs: any[] = [undefined, { x: 1 }];
const one: any[] = [{ k: 1, j: 2 }];
const nums = [3, 9, 4];
console.log("assign-mid", JSON.stringify(Object.assign({}, ...srcs, { y: 2 })));
console.log("assign-tail", JSON.stringify(Object.assign({}, ...srcs)));
console.log("assign-all", JSON.stringify(Object.assign(...([{ t: 1 }, { u: 2 }] as [object, object]))));
const target = { z: 0 };
console.log("assign-identity", Object.assign(target, ...srcs) === target, JSON.stringify(target));
console.log("keys", JSON.stringify(Object.keys(...(one as [object]))));
console.log("values", JSON.stringify(Object.values(...(one as [object]))));
console.log("entries", JSON.stringify(Object.entries(...(one as [object]))));
console.log("fromEntries", JSON.stringify(Object.fromEntries(...([[["a", 1]]] as [any]))));
console.log("is", Object.is(...([1, 1] as [number, number])));
console.log("hasOwn", Object.hasOwn(...([{ a: 1 }, "a"] as [object, string])));
console.log("create", Object.getPrototypeOf(Object.create(...([null] as [null]))));
console.log("math", Math.max(...nums), Math.min(...nums), Math.hypot(...([3, 4] as [number, number])));
console.log("fromCharCode", String.fromCharCode(...[72, 105]));
console.log("array", JSON.stringify(Array.of(...nums)), JSON.stringify(Array.from(...([[1, 2]] as [number[]]))));
console.log("isArray", Array.isArray(...([[1]] as [unknown])));
console.log("json", JSON.stringify(...([{ a: 1 }] as [unknown])), JSON.parse(...(["[1]"] as [string])).length);
console.log("number", Number.isInteger(...([2] as [number])), Number.parseInt(...(["12", 10] as [string, number])));
console.log("globals", parseInt(...(["ff", 16] as [string, number])), Boolean(...([0] as [number])));
console.log("reflect", JSON.stringify(Reflect.ownKeys(...(one as [object]))), Reflect.has(...([{ a: 1 }, "a"] as [object, string])));
