import { constants } from "node:perf_hooks";
console.log(Object.keys(constants).sort().join(","));
console.log(
  Object.entries(constants).sort(([a], [b]) => a.localeCompare(b)).map((
    [key, value],
  ) => `${key}=${value}`).join(","),
);
