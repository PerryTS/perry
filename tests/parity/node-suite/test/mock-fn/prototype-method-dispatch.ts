import { mock } from "node:test";

class Counter {
  value: number;
  constructor(value: number) {
    this.value = value;
  }
  read() {
    return this.value;
  }
}

function propertyName(name: string): string {
  return name;
}

const instance = new Counter(7);
const method = mock.method(Counter.prototype, propertyName("read"), function (this: Counter) {
  return this.value + 10;
});
console.log("dynamic result:", instance.read());
console.log("dynamic count:", method.mock.callCount());
const call = method.mock.calls[0];
console.log("dynamic record:", call.this === instance, JSON.stringify(call.arguments), call.result);
method.mock.restore();
console.log("dynamic restore:", instance.read());
