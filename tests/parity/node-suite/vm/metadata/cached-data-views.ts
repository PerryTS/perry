import * as vm from "node:vm";

const scriptSource = "value + 1";
const scriptCache = new vm.Script(scriptSource).createCachedData();
const scriptBytes = new Uint8Array(
  scriptCache.buffer,
  scriptCache.byteOffset,
  scriptCache.byteLength,
);
const scriptView = new DataView(
  scriptCache.buffer,
  scriptCache.byteOffset,
  scriptCache.byteLength,
);
console.log(
  "Script views:",
  new vm.Script(scriptSource, { cachedData: scriptBytes }).cachedDataRejected,
  new vm.Script(scriptSource, { cachedData: scriptView }).cachedDataRejected,
);

const functionSource = "return value + 1";
const produced: any = vm.compileFunction(functionSource, ["value"], {
  produceCachedData: true,
});
const functionBytes = new Uint8Array(
  produced.cachedData.buffer,
  produced.cachedData.byteOffset,
  produced.cachedData.byteLength,
);
const functionView = new DataView(
  produced.cachedData.buffer,
  produced.cachedData.byteOffset,
  produced.cachedData.byteLength,
);
console.log(
  "function views:",
  vm.compileFunction(functionSource, ["value"], { cachedData: functionBytes })
    .cachedDataRejected,
  vm.compileFunction(functionSource, ["value"], { cachedData: functionView })
    .cachedDataRejected,
);
