// Issue #2089 — Date is a reference type: setter mutations must propagate
// through aliasing / function / closure boundaries (this is what made effect's
// DateTime.add a no-op). Pre-fix Perry modeled Date as a value-type f64, so
// each binding held an independent copy and mutations didn't propagate.

// Alias — `const b = a` is the same object in JS.
const a = new Date(1704067200000);
const b = a;
b.setUTCDate(b.getUTCDate() + 1);
console.log(a.getTime() - 1704067200000); // 86400000
console.log(b.getTime() - 1704067200000); // 86400000

// Mutation through a function parameter.
function bump(d: Date) {
  d.setUTCDate(d.getUTCDate() + 1);
}
const c = new Date(1704067200000);
bump(c);
console.log(c.getTime() - 1704067200000); // 86400000

// Mutation through a closure parameter (the effect mutate(self, f) shape).
function mutate(self: Date, f: (d: Date) => void): Date {
  f(self);
  return self;
}
const e = new Date(1704067200000);
mutate(e, (d) => { d.setUTCDate(d.getUTCDate() + 1); });
console.log(e.getTime() - 1704067200000); // 86400000

// Function-local Date passed to a mutating fn.
function addDay(): number {
  const local = new Date(1704067200000);
  bump(local);
  return local.getTime() - 1704067200000;
}
console.log(addDay()); // 86400000

// effect DateTime.add shape: clone, mutate the clone via a closure, read back;
// the original stays untouched.
const addDuration = (self: Date, days: number): Date =>
  mutate(new Date(self.getTime()), (date) => {
    date.setUTCDate(date.getUTCDate() + days);
  });
const base = new Date(1704067200000);
const next = addDuration(base, 1);
console.log(next.getTime() - base.getTime()); // 86400000
console.log(base.getTime() - 1704067200000);  // 0

// Reference identity: distinct objects with the same time are NOT ===.
console.log(new Date(0) === new Date(0)); // false
const x = new Date(0);
console.log(x === x);                      // true

// The rest of the Date surface stays intact.
const d = new Date(1704067200000);
console.log(typeof d);            // object
console.log(d instanceof Date);   // true
console.log(+d);                  // 1704067200000
console.log(d < next);            // true
console.log(JSON.stringify(d));   // "2024-01-01T00:00:00.000Z"
console.log(JSON.stringify([d])); // ["2024-01-01T00:00:00.000Z"]
const inv = new Date(NaN);
console.log(inv instanceof Date); // true
console.log(inv.toString());      // Invalid Date
console.log(JSON.stringify(inv)); // null
