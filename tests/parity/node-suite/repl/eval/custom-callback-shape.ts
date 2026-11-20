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
let server: any;
server = start({
  input,
  output,
  terminal: false,
  prompt: "",
  eval(code: string, context: unknown, filename: string, callback: Function) {
    console.log(this === server);
    console.log(JSON.stringify(code), context === server.context, filename);
    callback(null, 42);
  },
});
server.write("21 * 2\n");
