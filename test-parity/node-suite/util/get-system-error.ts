// util.getSystemErrorName / getSystemErrorMessage / getSystemErrorMap (#2514).
// Codes are libuv-style negatives; messages are libuv's (not libc strerror).
import util from "node:util";

console.log("name -2:", util.getSystemErrorName(-2));
console.log("name -13:", util.getSystemErrorName(-13));
console.log("name -4095:", util.getSystemErrorName(-4095));
console.log("name unmapped:", util.getSystemErrorName(-999999));
console.log("msg -2:", util.getSystemErrorMessage(-2));
console.log("msg -17:", util.getSystemErrorMessage(-17));
console.log("msg -21:", util.getSystemErrorMessage(-21));
console.log("msg -3008:", util.getSystemErrorMessage(-3008));
const m = util.getSystemErrorMap();
console.log("map isMap:", m instanceof Map);
console.log("map size:", m.size);
console.log("map -2:", JSON.stringify(m.get(-2)));
console.log("map -9:", JSON.stringify(m.get(-9)));
console.log("map -4094:", JSON.stringify(m.get(-4094)));

import { getSystemErrorName } from "node:util";
console.log("named -28:", getSystemErrorName(-28));
