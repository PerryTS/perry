// #3719: node:test current named exports — `assert` (object) + `expectFailure` (function).
import * as t from "node:test";
import { assert, expectFailure, test as namedTest } from "node:test";

console.log("expectFailure:", typeof expectFailure);
console.log("expectFailure namespace:", typeof t.expectFailure);
console.log("assert:", typeof assert);
console.log("assert namespace:", typeof t.assert);
console.log("assert.register:", typeof assert.register);
console.log("assert keys:", Object.keys(assert).sort().join(","));
console.log("namedTest:", typeof namedTest);
