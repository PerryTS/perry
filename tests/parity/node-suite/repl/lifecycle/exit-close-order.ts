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
const server = start({ input, output, terminal: false, prompt: "" });
const events: string[] = [];
server.on("exit", () => events.push("exit"));
server.on("close", () => {
  events.push("close");
  console.log(events.join(","));
});
server.write(".exit\n");
