// A program that creates a vm context before it observes a lazy family keeps its
// own family: the vm's birth never becomes the main realm's (node prints the same).
import * as vm from "node:vm";

const ctx = vm.createContext({});
const vmTypeErrorProto = vm.runInContext("new TypeError('x'); TypeError.prototype", ctx);
const vmDateProto = vm.runInContext("Date.prototype", ctx);
console.log("TypeError.prototype main", TypeError.prototype !== vmTypeErrorProto);
console.log("instanceof main", new TypeError("y") instanceof TypeError);
console.log("vm error not main", !(vm.runInContext("new TypeError('z')", ctx) instanceof TypeError));
console.log("Date.prototype main", Date.prototype !== vmDateProto);
console.log("date instanceof", new Date(0) instanceof Date);
console.log("proto of main error", Object.getPrototypeOf(new TypeError("w")) === TypeError.prototype);
