import { start } from "node:repl";

const input = {
  on() {},
  once() {},
  resume() {},
  pause() {},
  setEncoding() {},
  removeListener() {},
};
const output = {
  write() {
    return true;
  },
  on() {},
  once() {},
  removeListener() {},
  isTTY: false,
};
const server = start({ input, output, terminal: false });
for (const name of ["_", "_error"]) {
  const descriptor = Object.getOwnPropertyDescriptor(server.context, name)!;
  console.log(
    name,
    descriptor.enumerable,
    descriptor.configurable,
    typeof descriptor.get,
    typeof descriptor.set,
  );
}
