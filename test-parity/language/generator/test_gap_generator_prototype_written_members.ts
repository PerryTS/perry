// A generator object's [[Prototype]] is its generator function's
// `prototype`, an ordinary object: members the program writes there are
// what for-of and `next()` call, before %GeneratorPrototype%'s protocol.
class P {
  x: number;
  constructor(x: number) { this.x = x; }
}
const N = process.argv.length + 1;

function fakeNext(): () => any {
  let calls = 0;
  return function () {
    return calls++ < 2 ? { value: { v: "str", k: "b" }, done: false } : { value: undefined, done: true };
  };
}

function viaNext(it: any): string {
  let s = "";
  for (let st = it.next(); !st.done; st = it.next()) s += String(st.value.v.x);
  return s;
}
function viaForOf(it: any): string {
  let s = "";
  for (const e of it) s += String(e.v.x);
  return s;
}
function viaDestructure(it: any): string {
  let s = "";
  for (let st = it.next(); !st.done; st = it.next()) {
    const { v, k } = st.value;
    s += String(v.x) + k.length;
  }
  return s;
}

// for-of calls a `next` written on the generator's prototype.
function* g1(n: number) { for (let i = 0; i < n; i++) yield { v: new P(i) }; }
(g1.prototype as any).next = fakeNext();
console.log("forof next", viaForOf(g1(N)));

// for-of starts at a `[Symbol.iterator]` written there.
function* g2(n: number) { for (let i = 0; i < n; i++) yield { v: new P(i) }; }
const n2 = fakeNext();
(g2.prototype as any)[Symbol.iterator] = function () { return { next: n2 }; };
console.log("forof iterator", viaForOf(g2(N)));

// A `next()` loop, plain and destructuring.
function* g3(n: number) { for (let i = 0; i < n; i++) yield { v: new P(i), k: "a" + i }; }
(g3.prototype as any).next = fakeNext();
console.log("next loop", viaNext(g3(N)));
(g3.prototype as any).next = fakeNext();
console.log("next destructure", viaDestructure(g3(N)));

// One read through a written next, then break.
function* g4() { yield { v: { x: 1 } }; }
(g4.prototype as any).next = function () { return { value: { v: 7 }, done: false }; };
for (const e of g4() as any) { console.log("break", e.v.x); break; }

// Control: an untouched generator runs its own body.
function* g5(n: number) { for (let i = 0; i < n; i++) yield { v: new P(i), k: "a" + i }; }
console.log("own", viaNext(g5(N)), viaForOf(g5(N)), viaDestructure(g5(N)));
console.log(Object.getPrototypeOf(g5(1)) === g5.prototype);
