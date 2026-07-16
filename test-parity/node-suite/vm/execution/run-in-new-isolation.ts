import * as vm from "node:vm";

(globalThis as any).__outside = 9;
try {
  const sandbox: any = { seed: 2 };
  const result = vm.runInNewContext(
    "seed = seed + 5; created = seed + 1; typeof process + ':' + typeof __outside",
    sandbox,
  );
  console.log("result:", result);
  console.log("sandbox:", sandbox.seed, sandbox.created);
  console.log("outer created:", typeof (globalThis as any).created);
  console.log(
    "receiver:",
    vm.runInNewContext("this === globalThis", sandbox) === true,
  );
  console.log(
    "sandbox identity:",
    vm.runInNewContext("this", sandbox) === sandbox,
  );
} finally {
  delete (globalThis as any).__outside;
}
