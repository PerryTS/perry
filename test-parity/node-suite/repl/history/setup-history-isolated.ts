import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { start } from "node:repl";

const directory = mkdtempSync(join(tmpdir(), "perry-repl-"));
const historyPath = join(directory, "history");
writeFileSync(historyPath, "2 + 2\n1 + 1\n");
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
const server = start({ input, output, terminal: false });
server.setupHistory(historyPath, (error: unknown, value: unknown) => {
  try {
    console.log(error === null, value === server);
    console.log(server.history.length, server.history[0], server.history[1]);
    console.log(server.historySize);
  } finally {
    if (typeof server.close === "function") server.close();
    rmSync(directory, { recursive: true, force: true });
  }
});
