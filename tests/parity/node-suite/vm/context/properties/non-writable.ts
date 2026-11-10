import * as vm from "node:vm";

const sandbox: any = {};
Object.defineProperty(sandbox, "locked", {
  value: 11,
  writable: false,
  configurable: true,
});
const context = vm.createContext(sandbox);

try {
  console.log("sloppy:", vm.runInContext("locked = 12; locked", context));
} catch (error: any) {
  console.log("sloppy:", error.name, error.code || "-");
}
console.log("outer:", sandbox.locked);
try {
  vm.runInContext("'use strict'; locked = 13", context);
  console.log("strict: ok");
} catch (error: any) {
  console.log("strict:", error.name, error.code || "-");
}
console.log("unchanged:", sandbox.locked);
