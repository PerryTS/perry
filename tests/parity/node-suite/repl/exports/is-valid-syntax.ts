import { isValidSyntax } from "node:repl";

console.log(isValidSyntax("1 + 1"));
console.log(isValidSyntax("const value ="));
console.log(isValidSyntax("function value() {"));
console.log(isValidSyntax("await Promise.resolve(1)"));
console.log(isValidSyntax("return 1"));
