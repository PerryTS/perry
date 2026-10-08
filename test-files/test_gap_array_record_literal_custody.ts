const log: string[] = [];
function value(n: number): number { log.push("value:" + n); return n; }
const [a, b, c] = [value(1), value(2), value(3)];
console.log("plain", a, b, c, log.join(","));
const proto: any = Array.prototype;
const original = proto[Symbol.iterator];
proto[Symbol.iterator] = function(this: number[]) {
  log.push("open:" + this.join(","));
  let i = 0;
  return { next: () => i < this.length ? { value: this[i++] * 10, done: false } : { done: true },
    return: () => { log.push("close"); return { done: true }; } };
};
const [x, y] = [value(4), value(5), value(6)];
proto[Symbol.iterator] = original;
console.log("override", x, y, log.join(","));
const itp: any = Object.getPrototypeOf([1][Symbol.iterator]());
const savedNext = itp.next;
itp.next = function(this: any) { const r = savedNext.call(this); if (!r.done) r.value += 100; return r; };
const [m, n] = [7, 8];
itp.next = savedNext;
console.log("next", m, n);
let receiver: any;
itp.return = function(this: any) { receiver = this; return { done: true }; };
const [first] = [11, 12, 13];
delete itp.return;
console.log("close-cursor", first, receiver.next().value, receiver.next().value, receiver.next().done);
const [p = (itp.return = function(this: any) { receiver = this; return { done: true }; }, 42)] = [undefined, 15];
delete itp.return;
console.log("late-close", p, receiver.next().value);
