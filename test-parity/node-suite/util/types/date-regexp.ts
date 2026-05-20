import * as util from "node:util";

console.log("date:", util.types.isDate(new Date(0)));
console.log("date false:", util.types.isDate("1970-01-01"));
console.log("regexp:", util.types.isRegExp(/abc/));
console.log("regexp false:", util.types.isRegExp("abc"));
