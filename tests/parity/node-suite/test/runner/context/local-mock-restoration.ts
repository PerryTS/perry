import test from "node:test";

const target = {
  value: 1,
  read() {
    return this.value;
  },
};

test("installs local mock", (t) => {
  t.mock.method(target, "read", () => 99);
  console.log("local-mock:during", target.read());
});

test("local mock restored", () => {
  console.log("local-mock:after", target.read(), (target.read as any).mock === undefined);
});
