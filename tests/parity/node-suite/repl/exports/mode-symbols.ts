import { REPL_MODE_SLOPPY, REPL_MODE_STRICT } from "node:repl";

console.log(
  typeof REPL_MODE_SLOPPY,
  REPL_MODE_SLOPPY.description,
  Symbol.keyFor(REPL_MODE_SLOPPY),
);
console.log(
  typeof REPL_MODE_STRICT,
  REPL_MODE_STRICT.description,
  Symbol.keyFor(REPL_MODE_STRICT),
);
console.log(REPL_MODE_SLOPPY === REPL_MODE_STRICT);
