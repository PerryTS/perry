const key = Symbol("key");
const globalKey = Symbol.for("shared-key");
const makeSymbol = Symbol;
const aliasKey = makeSymbol("alias-key");
const value: any = { other: "string" };
value[key] = 7;
value[globalKey] = 9;
value[aliasKey] = 11;
const inherited = Object.create(value);
function read(object: any, index: any) { return object[index]; }
console.log(value[key], value[globalKey], value[aliasKey]);
console.log(inherited[key], read(value, key), read(value, "other"));
Object.defineProperty(value, key, { get() { return 13; }, configurable: true });
console.log(inherited[key], value[key]);
const result = { inherited: inherited[globalKey], plain: value.other };
console.log(result.inherited, result.plain);
