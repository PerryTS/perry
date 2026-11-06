import * as vm from "node:vm";

const script = new vm.Script(
  "counter = typeof counter === 'undefined' ? 1 : counter + 1; counter",
);

console.log("fresh:", script.runInNewContext(), script.runInNewContext());
const sandbox: any = {};
console.log(
  "reused object:",
  script.runInNewContext(sandbox),
  script.runInNewContext(sandbox),
  sandbox.counter,
);
