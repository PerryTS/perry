function instance(x: any, c: any) { return x instanceof c; }
function show(name: string, fn: any) {
  try { console.log(name + "=" + fn()); }
  catch (e: any) { console.log(name + "=throw:" + e.name); }
}
function Unmaterialized() {}
show("unmaterialized", () => instance({}, Unmaterialized));
function Terminal() {}
(Terminal as any).prototype = {};
const terminal = {};
show("terminal-before", () => instance(terminal, Terminal));
(Terminal as any).prototype = Object.prototype;
show("terminal-after", () => instance(terminal, Terminal));
function F() {}
const old = Object.create(F.prototype);
show("initial", () => instance(old, F));
(F as any).prototype = {};
show("replaced", () => instance(old, F));
const fresh = Object.create(F.prototype);
show("fresh", () => instance(fresh, F));
Object.setPrototypeOf(fresh, null);
show("instance-relink", () => instance(fresh, F));
function Base() {}
const deep = Object.create(F.prototype);
show("parent-before", () => instance(deep, Base));
Object.setPrototypeOf(F.prototype, Base.prototype);
show("parent-after", () => instance(deep, Base));
const bound = (F as any).bind(null);
show("bound", () => instance(deep, bound));
show("primitive", () => instance(1, F));
show("noncallable", () => instance({}, {}));
(F as any).prototype = 1;
show("bad-prototype", () => instance(old, F));
show("bad-prototype-primitive", () => instance(1, F));
function Later() {}
show("hook-before", () => instance(42, Later));
Object.defineProperty(Later, Symbol.hasInstance, { value: function(x: any) { return this === Later && x === 42; }, configurable: true });
show("hook-after", () => instance(42, Later));
let sum = 0;
for (let i = 0; i < 200; i++) {
  const holder: any = { i, object: Object.create(Base.prototype), padding: [i, i + 1] };
  if (instance(holder.object, Base)) sum++;
}
console.log("poll-loop=" + sum);
