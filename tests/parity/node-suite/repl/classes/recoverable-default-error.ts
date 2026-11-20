import { Recoverable } from "node:repl";

const error = new Recoverable();
console.log(error instanceof Recoverable, error instanceof SyntaxError);
console.log(error.err, error.name, error.message);
