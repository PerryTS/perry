// DEC 250 (i): the program never adds an element to %Array.prototype%. Every
// growth path refuses it, not only the element store: push / unshift / splice
// called on Array.prototype itself and a length write that would grow its
// storage (it has none: the lock keeps it an empty array; shrinking to 0 is
// fine). Reflect.set / Reflect.defineProperty report a
// refused write as false instead of throwing (ECMA-262 28.1.12, 28.1.13).
// node prints the unlocked behaviour; the expected file is the locked one.
const AP: any = Array.prototype;
function show(name: string, f: () => unknown) {
  let r: unknown;
  try { r = f(); } catch (e) { r = "threw " + (e as Error).constructor.name + ": " + (e as Error).message; }
  console.log(name, "->", r, "| length", AP.length, "| [0]", AP[0], "| 0 in", 0 in AP);
}
show("push", () => AP.push(7));
show("push.call", () => Array.prototype.push.call(AP, 7, 8));
show("unshift.call", () => Array.prototype.unshift.call(AP, 8));
show("splice.call insert", () => Array.prototype.splice.call(AP, 0, 0, 9));
show("splice.call nothing", () => Array.prototype.splice.call(AP, 0, 0).length);
show("length = 3", () => { AP.length = 3; return AP.length; });
show("fill", () => { Array.prototype.fill.call(AP, 1); return "filled"; });
show("index store", () => { AP[1] = 5; return "stored"; });
show("length = 0", () => { AP.length = 0; return AP.length; });
show("Reflect.set index", () => Reflect.set(AP, "0", 1));
show("Reflect.set number", () => Reflect.set(AP, 2, 1));
show("Reflect.defineProperty index", () => Reflect.defineProperty(AP, "0", { value: 1 }));
show("Reflect.set join", () => Reflect.set(AP, "join", 1));
show("Reflect.defineProperty join", () => Reflect.defineProperty(AP, "join", { value: 1 }));
show("Reflect.set named add", () => Reflect.set(AP, "wplk1Named", 1) && AP.wplk1Named);
show("Reflect.set Object.prototype add", () => Reflect.set(Object.prototype, "wplk1", 1));
show("Reflect.defineProperty Function.prototype add", () => Reflect.defineProperty(Function.prototype, "wplk1", { value: 1 }));
show("Reflect.set Object.prototype index", () => Reflect.set(Object.prototype, 0, 1));
show("Reflect.set plain", () => { const o: any = {}; return Reflect.set(o, "a", 1) && o.a; });
show("Reflect.defineProperty frozen", () => Reflect.defineProperty(Object.freeze({}), "a", { value: 1 }));
const a: number[] = [];
for (let i = 0; i < 10; i++) a.push(i);
a.unshift(-1); a.splice(2, 0, 99); a.length = 15;
console.log("ordinary arrays still grow", a.length, a.slice(0, 4).join(","), [][0], ({} as any)[0]);
