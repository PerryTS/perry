// A typed array's push honours a user prototype override.
// An array that adopts a user prototype (`Object.create(Array.prototype)`)
// reads `push` through that prototype, so an override there is what
// `a.push(v)` calls, whatever the receiver's declared type. map and indexOf
// always did; the number[]-typed push appended through the builtin.
const ap: any = Object.create(Array.prototype);
ap.push = function (this: any, v: number) { return 701 + v + this.length * 1000; };
const a: number[] = Object.setPrototypeOf([1, 2], ap);
const b: any = Object.setPrototypeOf([1, 2], ap);
function viaParam(x: number[]): number { return x.push(3); }
console.log(a.push(3), b.push(3), viaParam(Object.setPrototypeOf([1], ap)), a.length, b.length);

// Statement position (the result is discarded): still the override.
const c: number[] = Object.setPrototypeOf([5], ap);
for (let i = 0; i < 3; i++) c.push(i);
console.log(c.length, c.join(","));

// An accessor on the user prototype: its getter selects the method.
let gets = 0;
const gp: any = Object.create(Array.prototype);
Object.defineProperty(gp, "push", {
  get() { gets++; return function (this: number[], v: number) { return -v; }; },
});
const d: number[] = Object.setPrototypeOf([], gp);
console.log(d.push(9), d.length, gets);

// A user prototype that keeps the builtin push: the push appends.
const keep: any = Object.create(Array.prototype);
const e: number[] = Object.setPrototypeOf([1], keep);
console.log(e.push(2), e.push(3), e.length, e.join(","));

// The override is read on every call: replacing it later is seen.
ap.push = function (v: number) { return -1000 - v; };
console.log(a.push(4), a.length);

// An own push (on the array itself, no prototype change) is the method the
// call takes, on a typed receiver and on an any-typed one alike.
const ownTyped: number[] = [1, 2];
(ownTyped as any).push = function (x: number) { return "own.push:" + x; };
const ownAny: any = [1, 2];
ownAny.push = function (x: any) { return "own.push:" + x; };
console.log(ownTyped.push(3), ownTyped.length, ownAny.push(3), ownAny.length);
