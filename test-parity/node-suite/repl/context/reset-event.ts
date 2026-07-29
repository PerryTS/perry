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
const server = start({ input, output, terminal: false, useGlobal: false });
const previous = server.context;
server.on("reset", function (context: any) {
  console.log(this === server);
  console.log(context === server.context, context !== previous);
});
server.resetContext();
