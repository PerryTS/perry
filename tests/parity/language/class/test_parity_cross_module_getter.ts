import { Box } from "../../../fixtures/programs/fixtures/parity_5891/cross_module_getter/lib.ts";
const box = new Box();
console.log("field=" + box.field);
console.log("method=" + box.method());
console.log("prop=" + box.prop);
