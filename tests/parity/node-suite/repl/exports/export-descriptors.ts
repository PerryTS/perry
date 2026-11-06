import repl from "node:repl";

for (
  const name of [
    "start",
    "writer",
    "REPLServer",
    "REPL_MODE_SLOPPY",
    "REPL_MODE_STRICT",
    "Recoverable",
    "isValidSyntax",
  ]
) {
  const descriptor = Object.getOwnPropertyDescriptor(repl, name)!;
  console.log(
    name,
    descriptor.enumerable,
    descriptor.configurable,
    descriptor.writable,
    typeof descriptor.value,
  );
}
