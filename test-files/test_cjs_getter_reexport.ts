import { transform, value, late, update, reads } from "./cjs_getter_reexport/index.cjs";

console.log(reads());
console.log(typeof transform);
console.log(transform(1));
console.log(value(2));
console.log(typeof late);
update();
console.log(transform(1));
console.log(value(2));
console.log(late(3));
console.log(reads());
