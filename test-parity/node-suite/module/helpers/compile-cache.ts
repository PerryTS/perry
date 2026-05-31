import * as Module from "node:module";

const status = Module.constants.compileCacheStatus;

console.log("status keys:", Object.keys(status).sort().join(","));
console.log(
  "status values:",
  Object.keys(status)
    .sort()
    .map((key) => `${key}=${status[key]}`)
    .join(","),
);
console.log("cache before:", String(Module.getCompileCacheDir()));

const first = Module.enableCompileCache("/tmp/perry-node-module-cache-probe");
console.log("first keys:", Object.keys(first).sort().join(","));
console.log("first enabled:", first.status === status.ENABLED);
console.log(
  "first directory contains:",
  String(first.directory).includes("perry-node-module-cache-probe"),
);
console.log(
  "cache after contains:",
  String(Module.getCompileCacheDir()).includes("perry-node-module-cache-probe"),
);

const capturedGetCompileCacheDir = Module.getCompileCacheDir;
console.log(
  "captured cache contains:",
  String(capturedGetCompileCacheDir()).includes("perry-node-module-cache-probe"),
);

const second = Module.enableCompileCache("/tmp/perry-node-module-cache-probe-2");
console.log("second already:", second.status === status.ALREADY_ENABLED);
console.log("flush:", String(Module.flushCompileCache()));
