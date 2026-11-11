import * as vm from "node:vm";

const first: any = vm.createContext({});
const second: any = vm.createContext({});

console.log(
  "declare:",
  vm.runInContext("let lexical = 3; const fixed = 4; lexical + fixed", first),
);
console.log(
  "repeat:",
  vm.runInContext("lexical = lexical + 2; lexical + fixed", first),
);
console.log(
  "not property:",
  Object.prototype.hasOwnProperty.call(first, "lexical"),
);
console.log("other context:", vm.runInContext("typeof lexical", second));
