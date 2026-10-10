function scan(a: any): string {
  let out = "";
  for (const x of a) {
    if (x === 1) { a.push(4); continue; }
    if (x === 3) continue;
    out += String(x);
  }
  return out;
}
console.log("continue-growth", scan([1,2,3]));
let steps = 0;
let closes = 0;
const own: any = [1,2,3];
own[Symbol.iterator] = function() {
  let index = 0;
  return {
    next() { steps++; return index < 3 ? {value: ++index, done: false} : {done: true}; },
    return() { closes++; return {done: true}; }
  };
};
let text = "";
for (const x of own) {
  if (x === 1) continue;
  text += String(x);
}
console.log("continue-protocol", text, steps, closes);
for (const x of own) { if (x === 1) continue; break; }
console.log("continue-close", steps, closes);
const family: any = Object.getPrototypeOf([].values());
const next = family.next;
let reads = 0;
Object.defineProperty(Array.prototype, "1", {
  configurable: true,
  get() { reads++; throw 17; }
});
let caught = 0;
const hole: any = [1,,3];
try { for (const x of hole) { continue; } } catch (e) { caught = e as number; }
delete (Array.prototype as any)[1];
console.log("continue-getter", reads, caught);
const captured: any = [1,2,3];
let result = "";
for (const x of captured) {
  if (x === 1) { family.next = function() { return {done: true}; }; continue; }
  result += String(x);
}
family.next = next;
console.log("continue-captured-next", result);
