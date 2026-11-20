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
function action() {}
console.log(server.defineCommand("hello", action));
const helloCommand = server.commands?.hello;
console.log(helloCommand?.action === action);
console.log(Object.keys(helloCommand ?? {}).join(","));
