import { mock } from "node:test";

const target: any = {};
Object.defineProperty(target, "method", {
  configurable: false,
  value() {
    return "original";
  },
});

try {
  mock.method(target, "method", () => "mocked");
  console.log("non-configurable: NO_THROW", target.method());
} catch (error) {
  console.log("non-configurable:", (error as any).code ?? (error as Error).name, target.method());
}
