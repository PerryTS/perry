// Birth identity belongs to the constructor object, through any binding.
function obtain(name: string): any { return (globalThis as any)[name]; }
const key = { key: 1 };
function exercise(name: string, C: any) {
    const args: any[] = name === "Map" || name === "WeakMap" ? [[[key, 17]]] : [[key]];
    const x: any = new C(...args);
    console.log(name, x.has(key), name === "Map" || name === "WeakMap" ? x.get(key) : "set");
    const holder = { constructorValue: C };
    const y: any = new holder.constructorValue(...args);
    console.log(name + " holder", y.has(key), Object.getPrototypeOf(y) === C.prototype);
    function NewTarget() {}
    const z: any = Reflect.construct(C, [], NewTarget);
    if (name === "Map" || name === "WeakMap") C.prototype.set.call(z, key, 17);
    else C.prototype.add.call(z, key);
    console.log(name + " reflect", Object.getPrototypeOf(z) === NewTarget.prototype,
        C.prototype.has.call(z, key));
    let calls = 0;
    function CustomTarget() {}
    CustomTarget.prototype = Object.create(C.prototype);
    const adderName = name === "Map" || name === "WeakMap" ? "set" : "add";
    Object.defineProperty(CustomTarget.prototype, adderName, {
        get: function(this: any) {
            calls++;
            console.log(name + " adder proto", Object.getPrototypeOf(this) === CustomTarget.prototype);
            return C.prototype[adderName];
        }, configurable: true
    });
    const custom: any = Reflect.construct(C, args, CustomTarget);
    console.log(name + " custom", calls, C.prototype.has.call(custom, key));
    let threw = false;
    try { Reflect.construct(C, args, NewTarget); } catch (e) { threw = e instanceof TypeError; }
    console.log(name + " missing adder", threw);
    const bound = C.bind(null);
    console.log(name + " bound", new bound(...args).has(key));
    function Wrapper(this: any) { this.wrapped = new C(...args); }
    const wrapper: any = new (Wrapper as any)();
    console.log(name + " wrapper", Object.getPrototypeOf(wrapper) === Wrapper.prototype,
        wrapper.wrapped.has(key), wrapper.has === undefined);
    const global = globalThis as any;
    global[name] = Wrapper;
    const saved: any = new C(...args);
    const replaced: any = new global[name]();
    console.log(name + " replaced", saved.has(key), replaced.wrapped.has(key));
    global[name] = C;
    Object.defineProperty(C, "name", { value: "Renamed", configurable: true });
    console.log(name + " renamed", new C(...args).has(key));
    Object.defineProperty(C, "name", { value: name, configurable: true });
}
for (const name of ["Map", "Set", "WeakMap", "WeakSet"]) exercise(name, obtain(name));
class M extends Map { marker = 1; }
class S extends Set { marker = 2; }
class WM extends WeakMap { marker = 3; }
class WS extends WeakSet { marker = 4; }
const m = new M([[key, 21]]);
const s = new S([key]);
const wm = new WM([[key, 23]]);
const ws = new WS([key]);
console.log("subclasses", m.get(key), s.has(key), wm.get(key), ws.has(key),
    Object.getPrototypeOf(m) === M.prototype, Object.getPrototypeOf(s) === S.prototype,
    Object.getPrototypeOf(wm) === WM.prototype, Object.getPrototypeOf(ws) === WS.prototype);
// The fixture has allocating loops for the protected moving-GC floor.
let sum = 0;
const W = obtain("WeakMap");
for (let i = 0; i < 300; i++) {
    const k = { i };
    const w: any = new W([[k, i]]);
    sum += w.get(k);
}
console.log("loop", sum);
