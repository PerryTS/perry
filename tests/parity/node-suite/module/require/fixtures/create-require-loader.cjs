"use strict";
// Required through `createRequire(import.meta.url)`: its exported functions
// run (create-require-runs-module*.ts).
const dep = require("./create-require-dep.cjs");
exports.load = function (o) {
  return "load " + o.k;
};
exports.dep = function (n) {
  return dep.twice(n);
};
