const a: any = [1,2,3,4];
const original = Array.prototype[Symbol.iterator];
function drive() { let s = 0; for (const x of a) s += x; return s; }
function factory(this: any) { console.log("factory", this === a); return original.call(this); }
function check(label: string) { try { console.log(label, drive()); } catch(e: any) { console.log(label, e instanceof TypeError); } }
for (const target of [a, Array.prototype]) {
    try { Object.defineProperty(target, Symbol.iterator, { configurable: true, get() { console.log("getter", this === a); return factory; } }); } catch (e) { console.log("DEC 250: " + (e as Error).message); }
    check("getter-call");
    try { Object.defineProperty(target, Symbol.iterator, { configurable: true, get() { console.log("missing", this === a); return undefined; } }); } catch (e) { console.log("DEC 250: " + (e as Error).message); }
    check("getter-absent");
    try { Object.defineProperty(target, Symbol.iterator, { configurable: true, get: undefined }); } catch (e) { console.log("DEC 250: " + (e as Error).message); }
    check("no-getter");
    try { Object.defineProperty(target, Symbol.iterator, { configurable: true, writable: true, value: 17 }); } catch (e) { console.log("DEC 250: " + (e as Error).message); }
    check("noncallable");
    try { target[Symbol.iterator] = function() { return 42; }; } catch (e) { console.log("DEC 250: " + (e as Error).message); }
    check("nonobject");
    try { const [] = a; console.log("nonobject-empty", false); } catch(e: any) { console.log("nonobject-empty", e instanceof TypeError); }
    try { target[Symbol.iterator] = function() { return [7,8]; }; } catch (e) { console.log("DEC 250: " + (e as Error).message); }
    check("array-result");
    try { delete target[Symbol.iterator]; } catch (e) { console.log("DEC 250: " + (e as Error).message); }
}
try { Object.defineProperty(Array.prototype, Symbol.iterator, { value: original, configurable: true, writable: true }); } catch (e) { console.log("DEC 250: " + (e as Error).message); }
const custom: any = Object.create(Array.prototype);
Object.defineProperty(custom, Symbol.iterator, {get() { console.log("custom-get", this === a); return factory; }});
Object.setPrototypeOf(a, custom);
check("custom-prototype");
