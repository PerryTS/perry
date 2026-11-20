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
  completer(line: string, callback: Function) {
    console.log(this === server, line);
    callback(null, [["hello"], line]);
  },
});
server.complete("he", (error: unknown, result: [string[], string]) => {
  console.log(error === null, result[0].join(","), result[1]);
});
