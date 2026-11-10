// `arr[i](...)` is a method call: the callee is a property reference whose
// base, the array, is `this` (EvaluateCall). Typed arrays of functions, any
// arrays, constant and run-time indices, and a non-escaping literal array.
const fns: any[] = [
  function (this: any) { return this; },
  function (this: any, a: number) { return this.length + a; },
];
console.log(fns[0]() === fns);
console.log(fns[1](10));
let i = 0;
console.log(fns[i]() === fns);
for (let k = 0; k < 2; k++) console.log(typeof fns[k](1));

const typed: Array<(this: any) => any> = [function () { return this; }];
console.log(typed[0]() === typed);

// A literal array used only as a call base must still exist as `this`.
const local = [function (this: any) { return Array.isArray(this) && this.length; }];
console.log(local[0]());

// Arrow elements ignore `this`.
const arrows = [() => 7, (x: number) => x * 2];
console.log(arrows[0](), arrows[1](4));

// Holes and non-functions throw TypeError.
const holes: any[] = [1];
try { holes[0](); } catch (e) { console.log(e instanceof TypeError); }
try { holes[5](); } catch (e) { console.log(e instanceof TypeError); }

// Element replaced at run time; class methods stored in an array.
class Box { v = 3; get(this: any) { return this === arr ? "array" : typeof this; } }
const arr: any[] = [new Box().get];
console.log(arr[0]());
arr[0] = function (this: any) { return this[1]; };
arr[1] = "second";
console.log(arr[0]());

async function viaAsync() {
  const xs = [function (this: any) { return Array.isArray(this); }];
  for (let k = 0; k < 1; k++) console.log(xs[k]());
}
viaAsync();
