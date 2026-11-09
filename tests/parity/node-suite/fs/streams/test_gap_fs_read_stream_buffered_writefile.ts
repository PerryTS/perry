import * as fs from "node:fs";
import * as os from "node:os";
import * as path from "node:path";

const dir = fs.mkdtempSync(path.join(os.tmpdir(), "perry-buffered-writefile-"));
const input = path.join(dir, "input.txt");
const original = "ABCDEFGHIJKLMNOPQRSTUVWXYZ012345";

async function check(autoClose: boolean, text = original) {
  fs.writeFileSync(input, text);
  const fd = fs.openSync(input, "r");
  const source = fs.createReadStream("", { fd, autoClose, highWaterMark: 16 });
  const events: string[] = [];
  let ends = 0;
  let closes = 0;
  source.on("end", () => { ends++; events.push("end"); });
  source.on("close", () => { closes++; events.push("close"); });
  await new Promise<void>((resolve) => source.once("readable", () => resolve()));
  const prefix = String(source.read(1));
  console.log("prefix", prefix, "buffered", source.readableLength);
  const output = path.join(dir, autoClose ? "owned.txt" : "borrowed.txt");
  await fs.promises.writeFile(output, source);
  if (ends !== 1) throw new Error("writeFile completed before the source ended");
  await new Promise<void>((resolve) => setTimeout(resolve, 20));
  const remainder = fs.readFileSync(output, "utf8");
  let alive = false;
  try { alive = fs.fstatSync(fd).isFile(); } catch (_) {}
  console.log("autoClose", autoClose, "output", remainder);
  console.log("accounted", prefix.length + remainder.length, "expected", text.length);
  console.log("equal", prefix + remainder === text, "input unchanged", fs.readFileSync(input, "utf8") === text);
  console.log("lifecycle", ends, closes, events.join(">"), "fd alive", alive);
  if (prefix + remainder !== text || fs.readFileSync(input, "utf8") !== text) throw new Error("Data accounting failed");
  if (events.join(">") !== (autoClose ? "end>close" : "end") || alive === autoClose) throw new Error("Ownership or lifecycle changed");
  if (alive) fs.closeSync(fd);
}

async function main() {
  await check(false);
  await check(true);
  console.log("beyond prefetch window");
  const large = "ABCDEFGHIJKLMNOPQRSTUVWXYZ012345abcdefghijklmnopqrstuvwxyz6789+-";
  await check(false, large);
  await check(true, large);
  fs.rmSync(dir, { recursive: true, force: true });
}
main();
