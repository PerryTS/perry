// #13645: with a `return` on %ArrayIteratorPrototype% the array arm still
// closes on break/return/throw, with the cursor one past the last Get, and
// a throwing Get (a hole getter) completes the record without closing.
const ArrayIteratorPrototype = Object.getPrototypeOf([][Symbol.iterator]());
const log: string[] = [];
ArrayIteratorPrototype.return = function (this: any) {
  const rest = this.next();
  log.push("close next=" + JSON.stringify(rest.value) + " done=" + rest.done);
  return {};
};
function probe(n: number): number { return n + 1; }
const a: number[] = [10, 20, 30, 40];
for (const x of a) { if (x === 20) break; probe(x); }
function ret(): number { for (const x of a) { if (probe(x) > 30) return x; } return 0; }
log.push("ret " + ret());
try { for (const x of a) { if (x === 10) throw new Error("t" + probe(x)); } } catch (e) { log.push((e as Error).message); }
for (const x of a) { probe(x); }
log.push("exhausted");
const holey: any[] = [1, , 3];
Object.defineProperty(Array.prototype, "1", { get() { throw new Error("getter"); }, configurable: true });
try { for (const x of holey) { probe(x); } } catch (e) { log.push("get " + (e as Error).message); }
delete (Array.prototype as any)[1];
console.log(log.join("\n"));
