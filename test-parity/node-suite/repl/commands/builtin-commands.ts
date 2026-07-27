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
console.log(Object.keys(server.commands).sort().join(","));
for (const name of ["break", "clear", "exit", "help", "load", "save"]) {
  console.log(
    name,
    typeof server.commands[name].action,
    typeof server.commands[name].help,
  );
}
