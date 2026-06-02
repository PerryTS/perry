import { start } from "node:repl";

const input = {
  on() {},
  once() {},
  resume() {},
  pause() {},
  setEncoding() {},
  removeListener() {},
};
let output = "";
const out = {
  write(chunk: unknown) {
    output += String(chunk);
    return true;
  },
  on() {},
  once() {},
  removeListener() {},
  columns: 80,
  isTTY: false,
};

const historyPath = "/tmp/perry-repl-history-" + process.pid + ".txt";
const server = start({ input, output: out, terminal: false, prompt: "p> ", useColors: false });

server.on("exit", () => {
  console.log("exit fired");
  console.log("captured:", JSON.stringify(output));
});

server.setupHistory(historyPath, (err: unknown, replServer: unknown) => {
  console.log("history cb err null:", err === null);
  console.log("history same server:", replServer === server);
  console.log("history array:", Array.isArray(server.history));
  console.log("history size:", typeof server.historySize, server.historySize > 0);
  server.write("1 + 1\n");
  server.write(".exit\n");
});
