const prototype = { inherited: undefined };
const target: Record<string | symbol, any> = Object.create(prototype);
const events: string[] = [];

Object.defineProperty(target, "locked", {
  value: 1,
  writable: false,
  enumerable: true,
  configurable: true,
});
Object.defineProperty(target, "getOnly", {
  get() { return 2; },
  enumerable: true,
  configurable: true,
});
Object.defineProperty(target, "withSetter", {
  set(value) { events.push("set:" + value); },
  enumerable: true,
  configurable: true,
});
target.s = 3;
target["κey"] = 4;

for (let i = 0; i < 65532; i++) {
  target["field_" + i] = i === 42 ? undefined : i;
}

for (const key of ["locked", "s", "κey", "field_42", "field_65531"]) {
  console.log(key, target[key], key in target, target.hasOwnProperty(key));
}
console.log("missing", target.missing, "missing" in target, target.hasOwnProperty("missing"));
console.log("inherited", target.inherited, "inherited" in target, target.hasOwnProperty("inherited"));

delete target.field_32768;
console.log("deleted", "field_32768" in target, "field_32769" in target);
target.field_32768 = 32768;
console.log("reinserted", target.field_32768, target.hasOwnProperty("field_32768"));

try {
  Object.assign(target, { locked: 9 });
  console.log("locked no throw");
} catch (error) {
  console.log("locked", error instanceof TypeError, target.locked);
}
try {
  Object.assign(target, { getOnly: 9 });
  console.log("getOnly no throw");
} catch (error) {
  console.log("getOnly", error instanceof TypeError, target.getOnly);
}

const source = Object.defineProperty({}, "withSetter", {
  get() { events.push("get"); return 7; },
  enumerable: true,
});
Object.assign(target, source);
console.log("accessors", events.join(","));

const symbol = Symbol("wide");
Object.assign(target, { [symbol]: 11 });
console.log("symbol", target[symbol], Object.getOwnPropertySymbols(target).length);

Object.preventExtensions(target);
try {
  Object.assign(target, { newKey: 12 });
  console.log("nonextensible no throw");
} catch (error) {
  console.log("nonextensible", error instanceof TypeError, "newKey" in target);
}
