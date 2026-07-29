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
captured = "";
server.write("function unfinished() {\n");
console.log(JSON.stringify(captured));
captured = "";
console.log(server.clearBufferedCommand());
server.write("1 + 1\n");
console.log(JSON.stringify(captured));
