import { mock } from "node:test";

const prototype = {
  read() {
    return "prototype";
  },
};
const target = Object.create(prototype);
const method = mock.method(target, "read", () => "mocked");
console.log("inherited mocked:", target.read(), Object.hasOwn(target, "read"));
method.mock.restore();
console.log("inherited restored:", target.read(), Object.hasOwn(target, "read"));
