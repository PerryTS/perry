import { start } from "node:repl";

let captured = "";
const stream = {
  write(chunk: unknown) {
    captured += String(chunk);
    return true;
  },
  on() {},
  once() {},
  resume() {},
  pause() {},
  setEncoding() {},
  removeListener() {},
  isTTY: false,
};
const server = start("legacy> ", stream as any);
console.log(server.input === stream, server.output === stream);
console.log(server.getPrompt());
console.log(JSON.stringify(captured));
