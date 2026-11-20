import { isMap, isSet, isDate } from "node:util/types";

console.log("map:", isMap(new Map()));
console.log("set:", isSet(new Set()));
console.log("date:", isDate(new Date(0)));
