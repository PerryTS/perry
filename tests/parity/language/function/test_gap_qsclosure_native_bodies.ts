const own: any = Object.prototype.hasOwnProperty;
const join: any = Array.prototype.join;
const obj = { x: 1, undefined: 2 };
console.log(own.call(obj, "x"), own.call(obj, "missing"), own.call(obj));
console.log(own.apply(obj, ["x"]));
console.log(join.call([1, 2, 3], ":"));
const keys: any = [];
Object.defineProperty(keys, "0", { get() { console.log("key-get"); return "x"; } });
keys.length = 1;
console.log(own.apply(obj, keys));
const construct: any = Array;
console.log(construct.call(null, 3).length, construct.apply(null, [1, 2]).join(","));
class C {}
try { Reflect.apply(C, null, []); } catch (e) { console.log(e instanceof TypeError); }
try { own.call(null, "x"); } catch (e) { console.log(e instanceof TypeError); }
// Keep native-call receivers and coercion arguments live across moving GC.
let hits = 0;
for (let i = 0; i < 32; i++) {
  const receiver = { x: i };
  const key = { toString() {
    const garbage: any[] = [];
    for (let j = 0; j < 40; j++) garbage.push({ tag: j });
    if (garbage.length !== 40) throw new Error("churn");
    return "x";
  } };
  hits += own.call(receiver, key) ? 1 : 0;
}
console.log(hits);
