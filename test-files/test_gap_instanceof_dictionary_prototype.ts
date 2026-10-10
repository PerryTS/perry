// A function's own data prototype remains readable after unrelated edits make
// its function shape dictionary. Hooks, accessors and bound targets still win.
const mapCtor: any = Map;
const objectProto = Object.prototype;
const values: any[] = [{}, Object.create(null), Object.create(Map.prototype), new Map()];
console.log(values.map(v => v instanceof mapCtor).join(","));
function C() {}
const ctor: any = C;
ctor.prototype = objectProto;
Object.defineProperty(ctor, "unrelated", { get() { throw new Error("unrelated getter ran"); } });
console.log({} instanceof ctor);
const custom = {};
ctor.prototype = custom;
console.log({} instanceof ctor, Object.create(custom) instanceof ctor);
ctor.prototype = objectProto;
const sym = Symbol("extra");
ctor[sym] = 1;
console.log({} instanceof ctor);
const bound: any = ctor.bind(null);
bound.prototype = custom;
console.log({} instanceof bound, Object.create(custom) instanceof bound);
let reads = 0;
const arrow: any = () => {};
Object.defineProperty(arrow, "prototype", { configurable: true, get() { reads++; return objectProto; } });
console.log({} instanceof arrow, Object.create(null) instanceof arrow, reads);
Object.defineProperty(arrow, "prototype", { configurable: true, value: custom });
console.log({} instanceof arrow, Object.create(custom) instanceof arrow, reads);
Object.defineProperty(ctor, Symbol.hasInstance, { configurable: true, value(v: any) { return v === 42; } });
console.log(42 instanceof ctor, {} instanceof ctor);
delete ctor[Symbol.hasInstance];
console.log({} instanceof ctor);
let threw = false;
try { console.log({} instanceof ({} as any)); } catch { threw = true; }
console.log(threw);
