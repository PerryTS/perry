import repl from "node:repl";

for (const name of ["builtinModules", "_builtinLibs"]) {
  const descriptor = Object.getOwnPropertyDescriptor(repl, name)!;
  console.log(
    name,
    descriptor.enumerable,
    descriptor.configurable,
    typeof descriptor.get,
    typeof descriptor.set,
  );
}
