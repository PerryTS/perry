import repl, * as namespace from "node:repl";
import {
  Recoverable,
  REPL_MODE_SLOPPY,
  REPL_MODE_STRICT,
  REPLServer,
  start,
} from "node:repl";

console.log(repl === namespace.default);
console.log(repl.start === start);
console.log(repl.REPLServer === REPLServer);
console.log(repl.Recoverable === Recoverable);
console.log(repl.REPL_MODE_SLOPPY === REPL_MODE_SLOPPY);
console.log(repl.REPL_MODE_STRICT === REPL_MODE_STRICT);
