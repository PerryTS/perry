// #10544: opening the next fs stream from the previous stream's 'end'
// ('finish') handler stopped silently after exactly 512 streams — no more
// events, the loop went idle and the process exited 0 with the work skipped.
import fs from "node:fs";
import os from "node:os";
import path from "node:path";

const dir = fs.mkdtempSync(path.join(os.tmpdir(), "perry-10544-"));
const file = path.join(dir, "in.txt");
fs.writeFileSync(file, "x".repeat(100));
const N = 1500;

function chainRead(): Promise<[number, number]> {
  return new Promise((resolve) => {
    let done = 0;
    let bytes = 0;
    const next = () => {
      if (done === N) return resolve([done, bytes]);
      const s = fs.createReadStream(file);
      s.on("data", (c: any) => (bytes += c.length));
      s.on("end", () => {
        done++;
        next();
      });
      s.on("error", (e: any) => console.log("read error", e.message));
    };
    next();
  });
}

function chainWrite(): Promise<number> {
  return new Promise((resolve) => {
    let done = 0;
    const next = () => {
      if (done === N) return resolve(done);
      const w = fs.createWriteStream(path.join(dir, "out.txt"));
      w.on("finish", () => {
        done++;
        next();
      });
      w.on("error", (e: any) => console.log("write error", e.message));
      w.end("z");
    };
    next();
  });
}

let finished = false;
process.on("exit", (code) => console.log("exit", code, "finished", finished));
(async () => {
  const [reads, bytes] = await chainRead();
  console.log("chained read streams:", reads, "bytes:", bytes);
  console.log("chained write streams:", await chainWrite());
  fs.rmSync(dir, { recursive: true, force: true });
  finished = true;
})();
