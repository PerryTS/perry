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
  prompt: "",
  writer(value: unknown) {
    return `value:${value}`;
  },
});
server.write("21 + 21\n");
console.log(JSON.stringify(captured));
