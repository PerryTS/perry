import * as vm from "node:vm";

(globalThis as any).__scriptCount = 0;
try {
  const script = new vm.Script(
    "__scriptCount = __scriptCount + 1; __scriptCount",
  );
  console.log(
    "this repeat:",
    script.runInThisContext(),
    script.runInThisContext(),
  );
  const sandbox: any = { __scriptCount: 10 };
  const context = vm.createContext(sandbox);
  console.log(
    "context repeat:",
    script.runInContext(context),
    script.runInContext(context),
    sandbox.__scriptCount,
  );
  console.log(
    "new repeat:",
    script.runInNewContext({ __scriptCount: 20 }),
    script.runInNewContext({ __scriptCount: 30 }),
  );
} finally {
  delete (globalThis as any).__scriptCount;
}
