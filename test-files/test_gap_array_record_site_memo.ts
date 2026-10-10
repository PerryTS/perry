// Each entry site memoizes the intrinsic owners' validated ShapeIds. Every
// mutation below lands after the site is warm; the next entry must observe it.
function sum(a: number[]): any {
  let t: any = 0;
  for (const x of a) t += x;
  return t;
}
function sumAny(a: any): any {
  let t: any = 0;
  for (const x of a) t += x;
  return t;
}
function firstTwo(a: any): any {
  const [p, q] = a;
  return String(p) + "/" + String(q);
}
function literal(): any {
  let t: any = "";
  for (const x of [5, 6]) t += x + ";";
  return t;
}
function attempt(label: string, f: () => any) {
  try {
    console.log(label, f());
  } catch (e) {
    console.log(label, "threw", (e as Error).constructor.name);
  }
}
function warm() {
  for (let i = 0; i < 3; i++) {
    sum([1, 2]);
    sumAny([1, 2]);
    firstTwo([1, 2]);
    literal();
  }
}
function report(label: string) {
  attempt(label + " typed", () => sum([1, 2]));
  attempt(label + " erased", () => sumAny([1, 2]));
  attempt(label + " destructure", () => firstTwo([1, 2]));
  attempt(label + " literal", () => literal());
}
const AP: any = Array.prototype;
const values = AP[Symbol.iterator];
const aip: any = Object.getPrototypeOf(values.call([]));
const next = aip.next;
const replacement = function (this: any) {
  let i = 0;
  const self = this;
  return { next: () => (i < self.length ? { value: self[i++] * 100, done: false } : { value: undefined, done: true }) };
};

warm(); report("pristine");

warm(); AP[Symbol.iterator] = replacement; report("store @@iterator");
AP[Symbol.iterator] = values; warm(); report("restored @@iterator");

warm();
Object.defineProperty(AP, Symbol.iterator, { get() { return replacement; }, configurable: true });
report("accessor @@iterator");
Object.defineProperty(AP, Symbol.iterator, { value: values, writable: true, configurable: true, enumerable: false });
warm(); report("redefined @@iterator");

warm(); delete AP[Symbol.iterator]; report("deleted @@iterator");
AP[Symbol.iterator] = values; warm(); report("re-added @@iterator");

warm(); Object.setPrototypeOf(AP, null); report("Array.prototype proto null");
Object.setPrototypeOf(AP, Object.prototype); warm(); report("Array.prototype proto restored");

warm();
aip.next = function (this: any) { const r = next.call(this); if (!r.done) r.value = -r.value; return r; };
report("store next");
aip.next = next; warm(); report("restored next");

warm();
Object.defineProperty(aip, "next", { get() { return function (this: any) { const r = next.call(this); if (!r.done) r.value += 1000; return r; }; }, configurable: true });
report("accessor next");
Object.defineProperty(aip, "next", { value: next, writable: true, configurable: true, enumerable: false });
warm(); report("redefined next");

warm(); delete aip.next; report("deleted next");
aip.next = next; warm(); report("re-added next");

warm(); const iterProto = Object.getPrototypeOf(aip); Object.setPrototypeOf(aip, null); report("aip proto null");
Object.setPrototypeOf(aip, iterProto); warm(); report("aip proto restored");

warm(); (AP as any).extra = 1; report("unrelated Array.prototype key");
delete (AP as any).extra; warm(); report("unrelated key removed");
