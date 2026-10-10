const a: any = [1,2,3,4];
const original = Array.prototype[Symbol.iterator];
function drive() { let s = 0; for (const x of a) s += x; return s; }
function factory(this: any) { console.log("factory", this === a); return original.call(this); }
function check(label: string) { try { console.log(label, drive()); } catch(e: any) { console.log(label, e instanceof TypeError); } }
for (const target of [a, Array.prototype]) {
    Object.defineProperty(target, Symbol.iterator, { configurable: true, get() { console.log("getter", this === a); return factory; } });
    check("getter-call");
    Object.defineProperty(target, Symbol.iterator, { configurable: true, get() { console.log("missing", this === a); return undefined; } });
    check("getter-absent");
    Object.defineProperty(target, Symbol.iterator, { configurable: true, get: undefined });
    check("no-getter");
    Object.defineProperty(target, Symbol.iterator, { configurable: true, writable: true, value: 17 });
    check("noncallable");
    target[Symbol.iterator] = function() { return 42; };
    check("nonobject");
    try { const [] = a; console.log("nonobject-empty", false); } catch(e: any) { console.log("nonobject-empty", e instanceof TypeError); }
    target[Symbol.iterator] = function() { return [7,8]; };
    check("array-result");
    delete target[Symbol.iterator];
}
Object.defineProperty(Array.prototype, Symbol.iterator, { value: original, configurable: true, writable: true });
const custom: any = Object.create(Array.prototype);
Object.defineProperty(custom, Symbol.iterator, {get() { console.log("custom-get", this === a); return factory; }});
Object.setPrototypeOf(a, custom);
check("custom-prototype");
