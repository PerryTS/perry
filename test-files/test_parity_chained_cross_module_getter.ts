import { VM } from "./fixtures/parity_5891/chained_cross_module_getter/lib.ts";
const vm = new VM();
console.log("chain=" + vm.viewport.scroll.scrollTop);
const scroll = vm.viewport.scroll;
console.log("local=" + scroll.scrollTop);
