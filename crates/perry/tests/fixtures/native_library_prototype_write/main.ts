import { js_cxnapi_write as write } from "foreign-writer";
const base = { inherited: "base" };
const child = Object.create(base);
write(child, "inherited", "own");
console.log("ffi shadowing store", child.inherited);
console.log("reflect read", Reflect.get(child, "inherited"));
