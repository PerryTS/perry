{
function push1(label: string, a: any, n: number): void {
  try { if (n) a.push(99); else a.push(); console.log(label, "no-throw", a.length); }
  catch (e: any) { console.log(label, e.constructor.name + ": " + e.message); }
}
const f1: number[] = [1, 2]; Object.freeze(f1); push1("freeze1", f1, 1); push1("freeze0", f1, 0);
const f2: any = [1, 2]; Object.freeze(f2);
try { Array.prototype.push.call(f2, 7); } catch (e: any) { console.log("call", e.message); }
try { f2.push(5, 6); } catch (e: any) { console.log("two", e.message); }
}
{
const e1: number[] = []; Object.defineProperty(e1, "length", { writable: false }); Object.preventExtensions(e1);
console.log("E1", Object.isSealed(e1), Object.isFrozen(e1), Reflect.ownKeys(e1).join());
const e2: any = []; Object.defineProperty(e2, "foo", { value: 1 }); Object.preventExtensions(e2);
console.log("E2", Object.isSealed(e2), Object.isFrozen(e2));
const e3: number[] = []; Object.preventExtensions(e3); Object.defineProperty(e3, "length", { writable: false });
console.log("E3", Object.isSealed(e3), Object.isFrozen(e3));
const e4: number[] = [1]; Object.seal(e4); Object.defineProperty(e4, "length", { writable: false });
console.log("E4", Object.isSealed(e4), Object.isFrozen(e4));
const e5: number[] = [1]; Object.defineProperty(e5, "length", { writable: false }); Object.seal(e5);
console.log("E5", Object.isSealed(e5), Object.isFrozen(e5));
}
function outcome(f: () => any): string { try { f(); return "ok"; } catch (e: any) { return e.constructor.name + ": " + e.message; } }
console.log("sealed push", outcome(() => Object.seal([1, 2]).push(0)), outcome(() => Object.seal([1, 2]).push()));
console.log("nonext push", outcome(() => Object.preventExtensions([1, 2]).push(0)), outcome(() => Object.preventExtensions([1, 2]).push()));
console.log("call push", outcome(() => Array.prototype.push.call(Object.freeze([1]), 1, 2)), outcome(() => Array.prototype.push.call(Object.freeze([1]))));
