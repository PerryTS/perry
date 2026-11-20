import { writer } from "node:repl";

console.log(typeof writer.options);
console.log(writer.options.showProxy);
console.log(typeof writer.options.depth, typeof writer.options.colors);
console.log(writer({ value: 42 }));
