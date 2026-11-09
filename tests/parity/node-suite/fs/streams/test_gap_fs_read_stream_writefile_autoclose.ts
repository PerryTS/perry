import * as fs from "node:fs";
import * as os from "node:os";
import * as path from "node:path";

const dir = fs.mkdtempSync(path.join(os.tmpdir(), "perry-writefile-autoclose-"));
const input = path.join(dir, "input.txt");
fs.writeFileSync(input, "caller-owned bytes");

async function check(autoClose: boolean, idle = false) {
  const fd = fs.openSync(input, "r");
  const source = fs.createReadStream("", { fd, autoClose, highWaterMark: 4 });
  const events: string[] = [];
  let ends = 0;
  let closes = 0;
  source.on("end", () => { ends++; events.push("end"); });
  source.on("close", () => { closes++; events.push("close"); });
  const output = path.join(dir, autoClose ? "owned.txt" : "borrowed.txt");
  if (idle) await new Promise<void>((resolve) => setImmediate(resolve));
  await fs.promises.writeFile(output, source);
  if (ends !== 1) throw new Error("writeFile completed before the source ended");
  await new Promise<void>((resolve) => setTimeout(resolve, 20));
  let alive = false;
  try { alive = fs.fstatSync(fd).isFile(); } catch (_) {}
  console.log("autoClose", autoClose, "bytes", fs.readFileSync(output, "utf8"));
  console.log("lifecycle", ends, closes, events.join(">"));
  console.log("fd alive", alive);
  if (fs.readFileSync(input, "utf8") !== "caller-owned bytes") throw new Error("Input changed");
  if (fs.readFileSync(output, "utf8") !== "caller-owned bytes") throw new Error("Output lost bytes");
  if (events.join(">") !== (autoClose ? "end>close" : "end") || alive === autoClose) throw new Error("Ownership or lifecycle changed");
  if (alive) fs.closeSync(fd);
}

async function main() {
  await check(false);
  await check(true);
  console.log("idle consumption");
  await check(false, true);
  await check(true, true);
  fs.rmSync(dir, { recursive: true, force: true });
}
main();
