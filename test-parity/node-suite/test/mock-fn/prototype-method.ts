import { mock } from "node:test";

class Counter {
  constructor(public value: number) {}
  read() {
    return this.value;
  }
}

const instance = new Counter(7);
const method = mock.method(Counter.prototype, "read");
console.log("prototype result:", instance.read());
console.log("prototype record:", method.mock.callCount(), method.mock.calls[0].this === instance);
method.mock.restore();
console.log("prototype restore:", instance.read());
