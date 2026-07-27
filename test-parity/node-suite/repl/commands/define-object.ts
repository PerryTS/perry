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
const command = { help: "show help", action() {} };
console.log(server.defineCommand("hello", command));
console.log(server.commands.hello === command);
console.log(server.commands.hello.help);
