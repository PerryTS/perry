import { Recoverable } from "node:repl";

const cause = new SyntaxError("incomplete");
const error = new Recoverable(cause);
console.log(error instanceof Recoverable);
console.log(error instanceof SyntaxError);
console.log(error.err === cause);
console.log(error.name, error.message);
console.log(Object.keys(error).join(","));
