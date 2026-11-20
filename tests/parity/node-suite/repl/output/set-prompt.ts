import { start } from "node:repl";

const input = {
  on() {},
  once() {},
  resume() {},
  pause() {},
  setEncoding() {},
  removeListener() {},
};
let captured = "";
const output = {
  write(chunk: unknown) {
    captured += String(chunk);
    return true;
  },
  on() {},
  once() {},
  removeListener() {},
  isTTY: false,
};
const server = start({ input, output, terminal: false, prompt: "first> " });
console.log(server.getPrompt());
console.log(server.setPrompt("next> "));
console.log(server.getPrompt());
captured = "";
server.displayPrompt();
console.log(JSON.stringify(captured));
