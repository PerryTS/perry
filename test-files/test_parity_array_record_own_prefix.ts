// Array-record own-prefix guards must be refreshed after each user body.
declare function gc(): void;
function consume(a: any[], mutate: (a: any[], step: number) => void): string {
  let step = 0;
  let out = "";
  for (const value of a) {
    out += String(value) + ",";
    mutate(a, step++);
  }
  return out;
}
function consumeLabeled(a: any[], mutate: (a: any[], step: number) => void): string {
  let step = 0;
  let out = "";
  outer: for (const value of a) {
    out += String(value) + ",";
    mutate(a, step++);
    if (step === 3) break outer;
  }
  return out;
}
function nothing(a: any[], step: number): void {}
console.log("dense", consume([1, 2, 3], nothing));
// This must run before any intrinsic prototype mutation: the global
// protector is still clean, but this array's own chain answers the hole.
const customProto = Object.create(Array.prototype);
Object.defineProperty(customProto, "1", { get() { return 31; } });
function retarget(a: any[], step: number): void {
  if (step === 0) {
    Object.setPrototypeOf(a, customProto);
    delete a[1];
  }
}
function namedArray(): any[] {
  const a: any = [1, 2, 3];
  a.marker = 1; // Reserve named storage before admission/retargeting.
  return a;
}
console.log("custom-proto", consume(namedArray(), retarget));
console.log("custom-proto-labeled", consumeLabeled(namedArray(), retarget));
console.log("delete", consume([1, 2, 3], (a, i) => { if (i === 0) delete a[1]; }));
console.log("truncate", consume([1, 2, 3], (a, i) => { if (i === 0) a.length = 1; }));
console.log("grow", consume([1], (a, i) => { if (i === 0) { a.push(2); a.push(3); } }));
console.log("accessor", consume([1, 2, 3], (a, i) => {
  if (i === 0) Object.defineProperty(a, "1", { configurable: true, get() { return 21; } });
}));
// A prototype getter cannot intercept own data, but must answer a hole.
let inheritedReads = 0;
let inherited = consume([1, 2, 3], (a, i) => {
  if (i === 0) {
    Object.defineProperty(Array.prototype, "1", {
      configurable: true,
      get() { inheritedReads++; return 41; },
      set(v) { Object.defineProperty(this, "1", { value: v, writable: true, configurable: true }); }
    });
    delete a[1];
  }
});
let own = consume([1, 2, 3], nothing);
delete (Array.prototype as any)[1];
console.log("inherited", inherited, own, inheritedReads);
// An inherited getter throwing during IteratorStep does not close; a body
// exception does. The captured built-in next still observes a changed return.
let closes = 0;
const iteratorProto = Object.getPrototypeOf([][Symbol.iterator]());
Object.defineProperty(iteratorProto, "return", { configurable: true, value() {
  closes++; return { done: true };
} });
try {
  consume([1, 2, 3], (a, i) => {
    if (i === 0) Object.defineProperty(a, "1", { get() { throw "get"; } });
  });
} catch (e) { console.log("throw-get", e, closes); }
try { consume([1, 2, 3], () => { throw "body"; }); }
catch (e) { console.log("throw-body", e, closes); }
for (const x of [1, 2, 3]) { break; }
console.log("break-close", closes);
delete iteratorProto.return;
// Entry still honors a custom iterator; next remains captured during iteration.
const custom: any = [1, 2, 3];
custom[Symbol.iterator] = function() {
  let i = 0;
  return { next() { return { value: 10 + i++, done: i > 2 }; } };
};
console.log("custom", consume(custom, nothing));
const captured = [1, 2, 3];
console.log("captured", consume(captured, (a, i) => {
  if (i === 0) (a as any)[Symbol.iterator] = () => { throw "new-iterator"; };
}));
// Reallocation and a moving collection between reads must reload the payload.
const moving: any[] = [1, 2, 3];
let visits = 0;
let sum = 0;
for (const value of moving) {
  sum += value;
  if (visits++ === 0) {
    for (let i = 4; i <= 256; i++) moving.push(i);
    if (typeof gc === "function") gc();
  }
}
console.log("move", visits, sum);
let unbacked = 0;
for (const value of new Array(1000001)) {
  if (value !== undefined) throw "unexpected-slot";
  if (++unbacked === 32) break;
}
console.log("unbacked", unbacked);
