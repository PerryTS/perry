import { error } from "./error.ts";
console.log("m1");
await Promise.resolve();
function fail() { console.log("finish"); throw error; }
export class C { static value = fail(); }
console.log("unexpected m2");
