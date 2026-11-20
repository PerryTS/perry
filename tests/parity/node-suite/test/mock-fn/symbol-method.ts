import { mock } from "node:test";

const key = Symbol("method");
const target = {
  [key](value: number) {
    return value + 1;
  },
};

try {
  const method = mock.method(target, key, (value: number) => value * 2);
  console.log("symbol method:", target[key](3), method.mock.callCount());
  method.mock.restore();
  console.log("symbol restore:", target[key](3));
} catch (error) {
  console.log("symbol error:", (error as any).code ?? (error as Error).name);
}
