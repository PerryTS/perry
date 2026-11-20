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
const commands = server.commands ?? {};
console.log(Object.keys(commands).sort().join(","));
for (const name of ["break", "clear", "exit", "help", "load", "save"]) {
  console.log(
    name,
    typeof commands[name]?.action,
    typeof commands[name]?.help,
  );
}
