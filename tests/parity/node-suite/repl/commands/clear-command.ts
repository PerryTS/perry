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
const server = start({
  input,
  output,
  terminal: false,
  prompt: "p> ",
  useColors: false,
});
server.context.marker = 42;
let resetContext: unknown;
server.once("reset", (context: unknown) => {
  resetContext = context;
});
captured = "";
server.write(".clear\n");
console.log(server.context.marker);
console.log(resetContext === server.context);
console.log(JSON.stringify(captured));
