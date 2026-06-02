import repl, { Recoverable, REPLServer, REPL_MODE_SLOPPY, REPL_MODE_STRICT } from "node:repl";

const inner = new SyntaxError("more");
const recoverable = new Recoverable(inner);
const plainSyntax: any = new SyntaxError("plain");
plainSyntax.err = inner;

function hasModule(name: string): boolean {
  for (const builtin of repl.builtinModules) {
    if (builtin === name) {
      return true;
    }
  }
  return false;
}

console.log("builtinModules array:", Array.isArray(repl.builtinModules));
console.log("builtinModules has fs:", hasModule("fs"));
console.log("builtinModules has node:fs:", hasModule("node:fs"));
console.log(
  "modes symbols:",
  typeof REPL_MODE_SLOPPY,
  typeof REPL_MODE_STRICT,
  REPL_MODE_SLOPPY === REPL_MODE_STRICT,
);
console.log("recoverable instanceof Recoverable:", recoverable instanceof Recoverable);
console.log("recoverable instanceof SyntaxError:", recoverable instanceof SyntaxError);
console.log("recoverable err identity:", recoverable.err === inner);
console.log("recoverable err syntax:", recoverable.err instanceof SyntaxError);
console.log("recoverable keys:", Object.keys(recoverable).join(","));
console.log("recoverable constructor name:", Recoverable.name, Recoverable.length);
console.log("plain syntax recoverable:", plainSyntax instanceof Recoverable);
console.log("plain object repl server:", ({ context: {} } instanceof REPLServer));
