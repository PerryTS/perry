import * as vm from "node:vm";

const source = "value = value + 1; value";
const script: any = new vm.Script(source);
const before = script.createCachedData();
const sandbox: any = { value: 1 };

console.log("run:", script.runInNewContext(sandbox), sandbox.value);
const after = script.createCachedData();
console.log(
  "cache shape:",
  Buffer.isBuffer(before),
  before.length > 0,
  Buffer.isBuffer(after),
  after.length > 0,
);
console.log(
  "accepted:",
  new vm.Script(source, { cachedData: before }).cachedDataRejected,
  new vm.Script(source, { cachedData: after }).cachedDataRejected,
);
