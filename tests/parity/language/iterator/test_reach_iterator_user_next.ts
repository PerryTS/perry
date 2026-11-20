// Built-in iterator methods call the program's own iterator methods: the
// helpers read `next` off their receiver (ECMA-262 27.1.2 GetIteratorDirect)
// and %IteratorPrototype%[@@dispose] calls the receiver's `return`.
const IteratorProto = Object.getPrototypeOf(Object.getPrototypeOf([][Symbol.iterator]()));

const counted: any = Object.create(IteratorProto);
let n = 0;
counted.next = function () {
  n++;
  return { done: n > 3, value: n };
};
console.log([...counted.map((x: number) => x * 2)].join(","));

class Countdown extends Iterator<number> {
  left = 3;
  next() {
    return this.left > 0 ? { done: false, value: this.left-- } : { done: true, value: undefined };
  }
}
console.log(new Countdown().toArray().join(","));

const disposable: any = Object.create(IteratorProto);
disposable.return = function () {
  console.log("return called");
  return { done: true };
};
disposable[Symbol.dispose]();
