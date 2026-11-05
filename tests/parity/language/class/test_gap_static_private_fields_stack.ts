// Static private storage must survive writes through the lexical class name.
// The construction gate is the shape used by lru-cache's Stack.
class Stack {
  static #constructing = false;
  heap: any[];
  static create(max: number) {
    Stack.#constructing = true;
    const stack = new Stack(max);
    Stack.#constructing = false;
    return stack;
  }
  constructor(max: number) {
    if (!Stack.#constructing) {
      throw new TypeError("instantiate Stack using Stack.create(n)");
    }
    this.heap = new Array(max);
  }
}
try { console.log("ok", Stack.create(4).heap.length); }
catch (e) { console.log("THROW", e.message); }
try { new Stack(1); }
catch (e) { console.log("direct", e instanceof TypeError); }

class Counter {
  static #n = 0;
  static #flag = false;
  static inc() { Counter.#n++; return Counter.#n; }
  static set(v: boolean) { Counter.#flag = v; }
  static read() { return Counter.#n; }
  static flag() { return Counter.#flag; }
  static has(value: any) { return #n in value; }
  static readFrom(value: any) { return value.#n; }
  static writeTo(value: any) { value.#n = 9; }
  static #method() { return Counter.#n + 10; }
  static get #value() { return Counter.#n; }
  static set #value(v: number) { Counter.#n = v; }
  static methodFrom(value: any) { return value.#method(); }
  static accessorFrom(value: any) { return value.#value; }
  static accessorTo(value: any, v: number) { value.#value = v; }
}
Counter.inc(); Counter.inc(); Counter.set(true);
console.log("counter", Counter.read());
console.log("flag", Counter.flag());
console.log("method", Counter.methodFrom(Counter));
Counter.accessorTo(Counter, 5);
console.log("accessor", Counter.accessorFrom(Counter), Counter.readFrom(Counter));
class Sub extends Counter {}
console.log("brand", Counter.has(Counter), Counter.has(Sub), Counter.has({}));
try { Counter.readFrom(Sub); } catch (e) { console.log("sub read", e instanceof TypeError); }
try { Counter.writeTo(Sub); } catch (e) { console.log("sub write", e instanceof TypeError); }
try { Counter.methodFrom(Sub); } catch (e) { console.log("sub method", e instanceof TypeError); }
try { Counter.accessorFrom(Sub); } catch (e) { console.log("sub getter", e instanceof TypeError); }
try { Counter.accessorTo(Sub, 7); } catch (e) { console.log("sub setter", e instanceof TypeError); }
// Public strings, including the compiler's diagnostic spelling, have their
// own namespace and cannot expose, overwrite, or delete a private field.
let hidden = true;
if (Counter["#n"] !== undefined) hidden = false;
Counter["#n"] = 100;
if (Counter["#n"] !== 100 || Counter.read() !== 5) hidden = false;
delete Counter["#n"];
for (let i = 1; i <= 12; i++) {
  const key = `#<perry:private-value:${i}:#n>`;
  if (Counter[key] !== undefined) hidden = false;
  Counter[key] = 99;
  if (Counter.read() !== 5) hidden = false;
  delete Counter[key];
  if (Counter.read() !== 5) hidden = false;
}
console.log("namespace", hidden);
console.log("reflection", Object.keys(Counter).length, Reflect.ownKeys(Counter).includes("#n"));
Object.freeze(Counter);
Counter.inc();
console.log("frozen private", Counter.read(), Counter.has(Counter));

const make = (initial: number): any => class Named {
  static #n = initial;
  static #method() { return Named.#n + 1; }
  static get #value() { return Named.#n; }
  static set #value(v: number) { Named.#n = v; }
  static read() { return Named.#n; }
  static inc() { Named.#n++; }
  static has(value: any) { return #n in value; }
  static method() { return Named.#method(); }
  static access(v: number) { Named.#value = v; return Named.#value; }
  static readFrom(value: any) { return value.#n; }
  static hasMethod(value: any) { return #method in value; }
  static hasAccessor(value: any) { return #value in value; }
  static methodFrom(value: any) { return value.#method(); }
  static accessorFrom(value: any) { return value.#value; }
};
const A = make(20), B = make(30);
A.inc();
console.log("expressions", A.read(), B.read(), A.method(), A.access(24));
console.log("expression brands", A.has(A), A.has(B), B.has(A), B.has(B));
try { A.readFrom(B); } catch (e) { console.log("other evaluation", e instanceof TypeError); }
const instance = new A();
console.log("static instance brands", A.has(instance), A.hasMethod(instance), A.hasAccessor(instance));
try { A.methodFrom(instance); } catch (e) { console.log("instance method", e instanceof TypeError); }
try { A.accessorFrom(instance); } catch (e) { console.log("instance getter", e instanceof TypeError); }
class ExprSub extends A {}
console.log("expression subclass", A.has(ExprSub));
try { A.readFrom(ExprSub); } catch (e) { console.log("expression sub read", e instanceof TypeError); }

class Order {
  static has(value: any) { return #n in value; }
  static read() { return Order.#n; }
  static {
    console.log("before field", Order.has(Order));
    try { Order.read(); } catch (e) { console.log("uninitialized", e instanceof TypeError); }
  }
  static #n = 4;
}
console.log("after field", Order.has(Order), Order.read());

class Values {
  static #é: any;
  static has(value: any) { return #é in value; }
  static read() { return Values.#é; }
  static write(value: any) { Values.#é = value; }
}
console.log("undefined field", Values.has(Values), Values.read() === undefined);
const stored = { answer: 42 };
Values.write(stored);
console.log("reference field", Values.read() === stored, Values.read().answer);
