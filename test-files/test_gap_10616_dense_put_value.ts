// Dense Array PutValue guards also serve class-member reference cells.
function factory(start: any) {
  var count = start;
  class Counter {
    constructor() { count++; }
    static next() { return count++; }
    static down() { return --count; }
  }
  return { Counter, read: () => count };
}
const first = factory(0);
const second = factory(20);
new first.Counter();
new second.Counter();
console.log(first.Counter.next(), first.Counter.down(), first.read(), second.read());
const big = factory(3n);
new big.Counter();
console.log(String(big.Counter.next()), String(big.Counter.down()), String(big.read()));
const text = factory("8");
new text.Counter();
console.log(text.Counter.next(), text.read());
let conversions = 0;
const converted = factory({ valueOf() { conversions++; return 40; } });
new converted.Counter();
console.log(converted.Counter.next(), converted.read(), conversions);

let shared = "shared";
const changed: any = [1];
changed[0] = shared;
shared += "!";
console.log(changed[0], shared);

function update(a: any, k: any) { return a[k]++; }
const a: any = [4, 8];
console.log(update(a, -0), update(a, 1), a[0], a[1]);
Object.seal(a);
console.log(update(a, 0), a[0]); // Existing elements remain writable.
Object.freeze(a);
try { update(a, 0); } catch (e) { console.log(e instanceof TypeError); }
console.log(a[0]);

const described: any = [10];
let value = 30;
Object.defineProperty(described, "0", {
  get() { return value; }, set(v) { value = v; }, configurable: true,
});
console.log(update(described, 0), value);
const target: any = [7];
const receiver: any = [40];
console.log(Reflect.set(target, "0", 99, receiver), target[0], receiver[0]);
let coercions = 0;
const key = { toString() { coercions++; return "0"; } };
target[key as any] = 12;
console.log(coercions, target[0]);
target[0.5] = 13;
target[-1] = 14;
console.log(target[0], target[0.5], target[-1]);

// A hole can be intercepted by an inherited setter.
const hole: any = new Array(1);
let seen = 0;
Object.defineProperty(Array.prototype, "0", {
  get() { return 50; }, set(v) { seen = v; }, configurable: true,
});
const old = update(hole, 0);
delete (Array.prototype as any)[0];
console.log(old, seen, Object.hasOwn(hole, "0"));
