import { mock } from "node:test";

const target = {
  value: 10,
  read() {
    return this.value;
  },
};

const first = mock.method(target, "read", () => 20);
const second = mock.method(target, "read", () => 30);
console.log("nested:", target.read(), first === second);
second.mock.restore();
console.log("restore second:", target.read(), target.read === first);
first.mock.restore();
console.log("restore first:", target.read(), (target.read as any).mock === undefined);
