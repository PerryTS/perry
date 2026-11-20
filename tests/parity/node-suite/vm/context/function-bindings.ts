import * as vm from "node:vm";

const sandbox: any = {};
const context = vm.createContext(sandbox);
console.log(
  "declare:",
  vm.runInContext(
    "function declared() { return 4; } var expression = function () { return 5; }; let lexical = () => 6; declared() + expression() + lexical()",
    context,
  ),
);
console.log(
  "properties:",
  typeof sandbox.declared,
  typeof sandbox.expression,
  typeof sandbox.lexical,
);
console.log("repeat:", vm.runInContext("declared() + expression()", context));
