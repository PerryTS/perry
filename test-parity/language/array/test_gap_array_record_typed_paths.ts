function collect(a: number[]): string {
  let s: any = "";
  for (const x of a) s += String(x) + ",";
  return s;
}
const own: number[] = [1,2,3];
(own as any)[Symbol.iterator] = function() {
  let i = 0; return { next: () => i++ < 2 ? { value: i * 10, done: false } : { done: true } };
};
console.log("own", collect(own));
const proto: any = Array.prototype;
const values = proto[Symbol.iterator];
try { proto[Symbol.iterator] = function(this: number[]) {
  let i = 0; return { next: () => i < this.length ? { value: this[i++] * 3, done: false } : { done: true } };
}; } catch (e) { console.log("DEC 250: " + (e as Error).message); }
console.log("prototype", collect([2,4]));
try { proto[Symbol.iterator] = values; } catch (e) { console.log("DEC 250: " + (e as Error).message); }
const itp: any = Object.getPrototypeOf(values.call([0]));
const next = itp.next;
try { itp.next = function(this: any) { const r = next.call(this); if (!r.done) r.value += 20; return r; }; } catch (e) { console.log("DEC 250: " + (e as Error).message); }
console.log("next", collect([1,2]));
try { itp.next = next; } catch (e) { console.log("DEC 250: " + (e as Error).message); }
const changing: number[] = [1,2];
let result = "";
for (const x of changing) {
  result += x + ",";
  try { if (x === 1) { changing.push(3); itp.next = () => ({done:true}); } } catch (e) { console.log("DEC 250: " + (e as Error).message); }
}
try { itp.next = next; } catch (e) { console.log("DEC 250: " + (e as Error).message); }
console.log("mutation", result);
const holey: number[] = [1,,3];
console.log("holes", collect(holey));
