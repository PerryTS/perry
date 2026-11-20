import * as vm from "node:vm";

const moduleOnly = 7;
console.log(
  "module local:",
  moduleOnly,
  vm.runInThisContext("typeof moduleOnly"),
);
try {
  console.log(
    "explicit global:",
    vm.runInThisContext("globalThis.vmScoped = 3; vmScoped"),
    (globalThis as any).vmScoped,
  );
} finally {
  delete (globalThis as any).vmScoped;
}
