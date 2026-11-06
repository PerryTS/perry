import * as vm from "node:vm";

(globalThis as any).__vmValue = 4;
try {
  console.log("read:", vm.runInThisContext("__vmValue"));
  console.log(
    "write result:",
    vm.runInThisContext("__vmValue = __vmValue + 3; __vmValue"),
  );
  console.log("write global:", (globalThis as any).__vmValue);
  console.log(
    "global identity:",
    vm.runInThisContext("this === globalThis") === true,
  );
  console.log("process visibility:", vm.runInThisContext("typeof process"));
} finally {
  delete (globalThis as any).__vmValue;
}
