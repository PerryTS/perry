import { getter } from "./_helpers/cross_module_method_wrapper.ts";
console.log(getter.get({ a: 1 }, "a"));
