import { METHODS } from "node:http";

console.log("array:", Array.isArray(METHODS));
console.log("count:", METHODS.length);
console.log("sorted:", METHODS.join("|") === [...METHODS].sort().join("|"));
console.log(
  "core:",
  ["GET", "POST", "PUT", "DELETE", "PATCH"].every((method) =>
    METHODS.includes(method)
  ),
);
console.log(
  "extended:",
  ["CONNECT", "M-SEARCH", "PURGE", "REPORT", "UNSUBSCRIBE"].every((method) =>
    METHODS.includes(method)
  ),
);
console.log("unique:", new Set(METHODS).size === METHODS.length);
