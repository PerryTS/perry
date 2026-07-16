// node:vm Script metadata and cached-data observable shape.
import * as vm from "node:vm";

function errorShape(label: string, fn: () => void) {
  try {
    fn();
    console.log(label + ":", "ok");
  } catch (error: any) {
    console.log(label + ":", error.name, error.code || "-");
  }
}

const source = "const local = 1;\nlocal + 2;\n//# sourceMappingURL=script.map";
const script: any = new vm.Script(source);
const cachedData = script.createCachedData();

console.log("source map:", script.sourceMapURL);
console.log(
  "script cache shape:",
  Buffer.isBuffer(cachedData),
  cachedData.length > 0,
);
console.log(
  "script no cache rejected:",
  script.cachedDataRejected === undefined,
);

const accepted: any = new vm.Script(source, { cachedData });
console.log("script cache accepted:", accepted.cachedDataRejected);

const rejected: any = new vm.Script("1", { cachedData: Buffer.from([0]) });
console.log("script cache rejected:", rejected.cachedDataRejected);

const produced: any = new vm.Script("1 + 1", { produceCachedData: true });
console.log(
  "script produce shape:",
  Buffer.isBuffer(produced.cachedData),
  produced.cachedData.length > 0,
  produced.cachedDataProduced,
);

errorShape("script cachedData validation", () => {
  new vm.Script("1", { cachedData: "nope" as any });
});

errorShape("script produce validation", () => {
  new vm.Script("1", { produceCachedData: "yes" as any });
});

const fn: any = vm.compileFunction("return a + 1", ["a"], {
  produceCachedData: true,
});
console.log(
  "compile produce shape:",
  fn(4),
  Buffer.isBuffer(fn.cachedData),
  fn.cachedData.length > 0,
  fn.cachedDataProduced,
);

const fnAccepted: any = vm.compileFunction("return a + 1", ["a"], {
  cachedData: fn.cachedData,
});
console.log("compile cache accepted:", fnAccepted.cachedDataRejected);

const fnRejected: any = vm.compileFunction("return 1", [], {
  cachedData: Buffer.from([0]),
});
console.log("compile cache rejected:", fnRejected.cachedDataRejected);

errorShape("compile cachedData validation", () => {
  vm.compileFunction("return 1", [], { cachedData: "nope" as any });
});

errorShape("compile produce validation", () => {
  vm.compileFunction("return 1", [], { produceCachedData: "yes" as any });
});
