import { mock } from "node:test";

const target = {
  read() {
    return "original";
  },
};
const method = mock.method(target, "read", () => "mocked");
console.log("mocked:", target.read());
method.mock.restore();
console.log("first restore:", target.read());
method.mock.restore();
console.log("second restore:", target.read());
