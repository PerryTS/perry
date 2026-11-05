// Each site is warmed proven, then warmed refused, then entered restored.
function typed(a: number[]): number {
  let sum = 0; for (const x of a) sum += x; return sum;
}
function erased(a: any): number {
  let sum = 0; for (const x of a) sum += x; return sum;
}
function binding(a: any): string {
  const [a0, a1] = a; return String(a0) + "/" + String(a1);
}
function literal(): number {
  let sum = 0; for (const x of [2, 3]) sum += x; return sum;
}
function warm(label: string) {
  for (let i = 0; i < 4; i++)
    console.log(label, typed([2, 3]), erased([2, 3]), binding([2, 3]), literal());
}
const ap: any = Array.prototype;
const values = ap[Symbol.iterator];
const aip: any = Object.getPrototypeOf(values.call([]));
const next = aip.next;
warm("proven");
try { ap[Symbol.iterator] = function(this: any) {
  let i = 0; const a = this;
  return { next() { return i < a.length ? { done: false, value: 10 * a[i++] } : { done: true }; } };
}; } catch (e) { console.log("DEC 250: " + (e as Error).message); }
warm("refused iterator");
try { ap[Symbol.iterator] = values; } catch (e) { console.log("DEC 250: " + (e as Error).message); }
warm("restored iterator");
try { aip.next = function(this: any) {
  const r = next.call(this); if (!r.done) r.value *= -1; return r;
}; } catch (e) { console.log("DEC 250: " + (e as Error).message); }
warm("refused next");
try { aip.next = next; } catch (e) { console.log("DEC 250: " + (e as Error).message); }
warm("restored next");
