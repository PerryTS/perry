import { constants } from "node:module";

const status = constants.compileCacheStatus;
console.log("constant keys:", Object.keys(constants).sort().join(","));
console.log("status keys:", Object.keys(status).sort().join(","));
console.log(
  "status values:",
  Object.entries(status).sort().map(([key, value]) => `${key}:${value}`).join(
    "|",
  ),
);
console.log(
  "objects frozen:",
  Object.isFrozen(constants),
  Object.isFrozen(status),
);
console.log(
  "descriptors:",
  JSON.stringify(Object.getOwnPropertyDescriptors(status)),
);
