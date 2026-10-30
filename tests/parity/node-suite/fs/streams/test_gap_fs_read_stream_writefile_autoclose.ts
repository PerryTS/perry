import * as fs from "node:fs";
import * as os from "node:os";
import * as path from "node:path";

const dir = fs.mkdtempSync(path.join(os.tmpdir(), "perry-writefile-autoclose-"));
const input = path.join(dir, "input.txt");
fs.writeFileSync(input, "caller-owned bytes");

async function check(autoClose: boolean) {
  const fd = fs.openSync(input, "r");
  const source = fs.createReadStream("", { fd, autoClose, highWaterMark: 4 });
  const events: string[] = [];
  let ends = 0;
  let closes = 0;
  source.on("end", () => { ends++; events.push("end"); });
  source.on("close", () => { closes++; events.push("close"); });
  const output = path.join(dir, autoClose ? "owned.txt" : "borrowed.txt");
  await fs.promises.writeFile(output, source);
  await new Promise<void>((resolve) => setTimeout(resolve, 20));
  let alive = false;
  try { alive = fs.fstatSync(fd).isFile(); } catch (_) {}
  console.log("autoClose", autoClose, "bytes", fs.readFileSync(output, "utf8"));
  console.log("lifecycle", ends, closes, events.join(">"));
  console.log("fd alive", alive);
  if (alive) fs.closeSync(fd);
}

async function main() {
  await check(false);
  await check(true);
  fs.rmSync(dir, { recursive: true, force: true });
}
main();
