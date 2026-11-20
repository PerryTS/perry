import { format } from "node:util";

console.log(format("string decimal d:%d i:%i f:%f", "12.5", "12.5", "12.5"));
console.log(format("number decimal d:%d i:%i f:%f", 12.5, 12.5, 12.5));
console.log(format("empty d:%d", ""));
console.log(format("negative fractional d:%d", -0.5));
