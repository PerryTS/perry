// A `function Q() {}` constructor whose prototype defines methods that share a
// name with an Array builtin (`push`/`pop`/`shift`/`unshift`). The array fast
// paths must not hijack them: the instance is a plain object, not an array.
// This is the shape denque uses (mysql2's command queue).

function Q(this: any) {
  this.n = 0;
}
Q.prototype.push = function (this: any, x: any) {
  this.n++;
  return "Q.push:" + x;
};
Q.prototype.pop = function (this: any) {
  return "Q.pop";
};
Q.prototype.shift = function (this: any) {
  return "Q.shift";
};
Q.prototype.unshift = function (this: any, x: any) {
  return "Q.unshift:" + x;
};

// receiver: local
const q: any = new (Q as any)();
console.log("local push   :", q.push("a"));
console.log("local pop    :", q.pop());
console.log("local shift  :", q.shift());
console.log("local unshift:", q.unshift("b"));
console.log("local field n:", q.n);

// receiver: property (`this._commands.push(cmd)` — mysql2's shape)
function Holder(this: any) {
  this.q = new (Q as any)();
}
Holder.prototype.add = function (this: any, x: any) {
  return this.q.push(x);
};
const h: any = new (Holder as any)();
console.log("field via method:", h.add("c"));
console.log("field direct    :", h.q.push("d"));
console.log("field n         :", h.q.n);

// receiver: parameter (type is `any`)
function useIt(dq: any) {
  return dq.push("e");
}
console.log("param        :", useIt(q));
console.log("param n      :", q.n);

// A denque-shaped ring buffer: push must actually enqueue so shift can dequeue.
function Deq(this: any) {
  this._head = 0;
  this._tail = 0;
  this._capacityMask = 3;
  this._list = [, , , ,];
}
Deq.prototype.size = function (this: any) {
  return this._head === this._tail
    ? 0
    : this._head < this._tail
      ? this._tail - this._head
      : this._capacityMask + 1 - (this._head - this._tail);
};
Deq.prototype.push = function (this: any, item: any) {
  const t = this._tail;
  this._list[t] = item;
  this._tail = (t + 1) & this._capacityMask;
  return this.size();
};
Deq.prototype.shift = function (this: any) {
  const head = this._head;
  if (head === this._tail) return undefined;
  const item = this._list[head];
  this._list[head] = undefined;
  this._head = (head + 1) & this._capacityMask;
  return item;
};

const dq: any = new (Deq as any)();
console.log("deq push ret :", dq.push("cmd1"));
console.log("deq size     :", dq.size());
console.log("deq shift    :", String(dq.shift()));
console.log("deq empty    :", String(dq.shift()));

// Real arrays must keep working (the fast path is preserved for them).
const arr: number[] = [1, 2];
arr.push(3);
console.log("array        :", arr.join(","), arr.length);

class WithField {
  items: number[] = [];
  add(v: number) {
    this.items.push(v);
    return this.items.length;
  }
}
const wf = new WithField();
wf.add(7);
console.log("typed field  :", wf.add(8), wf.items.join(","));
