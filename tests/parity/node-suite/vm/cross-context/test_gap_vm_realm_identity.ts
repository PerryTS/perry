// Each vm context is its own realm: its builtins are not the main realm's and
// not another context's (node prints the same lines).
import * as vm from "node:vm";

const a = vm.createContext({ MainArray: Array, MainError: Error });
const b = vm.createContext({});
console.log("array own", vm.runInContext("Array", a) !== Array);
console.log("two realms", vm.runInContext("Array", a) !== vm.runInContext("Array", b));
console.log("proto two realms", vm.runInContext("Array.prototype", a) !== vm.runInContext("Array.prototype", b));
console.log("instanceof main", vm.runInContext("[] instanceof MainArray", a));
console.log("isArray main", vm.runInContext("MainArray.isArray([])", a));
console.log("error chain", vm.runInContext("Object.getPrototypeOf(TypeError.prototype) === Error.prototype", a));
console.log("error not main", vm.runInContext("Object.getPrototypeOf(TypeError.prototype) === MainError.prototype", a));
console.log("new ctx each", vm.runInNewContext("Object") !== vm.runInNewContext("Object"));
