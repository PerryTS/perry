import * as types from "node:util/types";

console.log("map:", types.isMap(new Map()));
console.log("array-buffer:", types.isArrayBuffer(new ArrayBuffer(1)));
console.log("date false:", types.isDate("2020-01-01"));
