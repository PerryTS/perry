import * as fs from "node:fs";

const ROOT = "/tmp/perry_node_suite_fs_stream_pause_resume";
try { fs.rmSync(ROOT, { recursive: true, force: true }); } catch (_e) {}
fs.mkdirSync(ROOT, { recursive: true });
const p = ROOT + "/pause.txt";
fs.writeFileSync(p, "abcdef");

await new Promise<void>((resolve) => {
  const chunks: string[] = [];
  let pausedOnce = false;
  const rs = fs.createReadStream(p, { encoding: "utf8", highWaterMark: 2 });
  rs.on("data", (chunk) => {
    chunks.push(chunk);
    if (!pausedOnce) {
      pausedOnce = true;
      rs.pause();
      console.log("read pause immediate:", chunks.join("|"), rs.isPaused());
      setTimeout(() => rs.resume(), 5);
    }
  });
  rs.on("end", () => {
    console.log("read pause chunks:", chunks.join("|"));
    resolve();
  });
});
