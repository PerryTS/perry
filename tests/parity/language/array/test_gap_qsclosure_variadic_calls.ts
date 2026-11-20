const push: any = Array.prototype.push;
const unshift: any = Array.prototype.unshift;
const splice: any = Array.prototype.splice;
console.log(push.length, unshift.length, splice.length);
const arr: any[] = [2, 3];
console.log(Reflect.apply(push, arr, []), Reflect.apply(unshift, arr, [0, 1]), arr.join(","));
const values = Array.from({ length: 32 }, (_, i) => ({ n: i }));
Reflect.apply(push, arr, values);
console.log(arr.length, arr[35].n);
console.log(Reflect.apply(splice, arr, [0, 2, "a", "b"]).join(","), arr[0], arr[1]);
const a = [1, 2, 3], b = [1, 2, 3];
console.log(Reflect.apply(splice, a, [1]).join(","), a.join(","));
console.log(Reflect.apply(splice, b, [1, undefined]).join(","), b.join(","));
const obj: any = { 0: "x", length: 1 };
console.log(Reflect.apply(push, obj, ["y", "z"]), obj[1], obj[2]);
console.log(Reflect.apply(unshift, obj, ["w"]), obj[0], obj[3]);
const log: string[] = [];
const proxy = new Proxy({ length: 0 } as any, {
  get(t, k) { log.push("get:" + String(k)); return Reflect.get(t, k); },
  set(t, k, v) { log.push("set:" + String(k)); return Reflect.set(t, k, v); },
});
console.log(Reflect.apply(push, proxy, ["p", "q"]), log.join(","));
for (const fn of [push, unshift, splice]) {
  try { Reflect.apply(fn, Object.freeze([1, 2]), [0, 1]); }
  catch (e) { console.log(e instanceof TypeError); }
}
function forward(fn: any) { return function(this: any) { return fn.apply(this, arguments); }; }
console.log(Reflect.apply(forward(push), [], [1, 2, 3]));
