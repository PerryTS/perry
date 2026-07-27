import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { start } from "node:repl";

const directory = mkdtempSync(join(tmpdir(), "perry-repl-"));
const historyPath = join(directory, "history");
writeFileSync(historyPath, "3\n2\n1\n");
const cleanup = () => rmSync(directory, { recursive: true, force: true });
process.once("exit", cleanup);
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
const result = server.setupHistory({
  filePath: historyPath,
  size: 2,
  removeHistoryDuplicates: false,
}, (error: unknown, value: unknown) => {
  try {
    console.log(error === null, value === server);
    console.log(server.history.length, server.history.join(","));
    console.log(server.historySize);
  } finally {
    if (typeof server.close === "function") server.close();
    process.removeListener("exit", cleanup);
    cleanup();
  }
});
console.log(result);
