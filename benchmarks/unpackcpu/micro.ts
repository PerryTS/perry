import { readFileSync, mkdirSync, rmSync } from "node:fs";
import { createHash } from "node:crypto";
import { gunzipSync } from "node:zlib";
import { Worker } from "node:worker_threads";
import { extractTar } from "./tar.ts";
import { files } from "./files.ts";
import { integrities } from "./integrities.ts";
const mode = process.argv[2] || "worker";
const selection = process.argv[3] || "all";
const rounds = Number(process.argv[4] || "1");
const dir = process.argv[5] || "/root/lanes/perry-unpackcpu/micro-store";
const selected = selection === "large" ? [1] : files.map((_, i) => i);
const compressed = selected.map(i => readFileSync(files[i] + ".tgz"));
const blocks = compressed.map(data => {
  const out: Uint8Array[] = [];
  for (let at = 0; at < data.length; at += 65536) out.push(Buffer.from(data.subarray(at, at + 65536)));
  return out;
});
async function* chunks(data: Uint8Array) {
  for (let at = 0; at < data.length; at += 65536) yield data.subarray(at, at + 65536);
}
async function main() {
  let bytes = 0, entries = 0;
  const output = createHash("sha256");
  if (mode === "worker" || mode === "worker-stream" || mode === "worker-noop") {
    rmSync(dir, { recursive: true, force: true }); mkdirSync(dir, { recursive: true });
    const worker = new Worker(new URL("./unpack-worker.ts", import.meta.url), { workerData: { dir } });
    let reply: ((v: any) => void) | undefined;
    const ready = new Promise<void>(resolve => { reply = resolve; });
    worker.on("message", value => { const r = reply; reply = undefined; r?.(value); });
    worker.on("error", error => { console.error(error); process.exit(1); });
    await ready;
    for (let r = 0; r < rounds && mode !== "worker-noop"; r++) {
      for (let j = 0; j < selected.length; j++) {
        const data = compressed[j];
        const pending = new Promise<any>(resolve => { reply = resolve; });
        if (mode === "worker") {
          worker.postMessage({ integrity: integrities[selected[j]], tarball: blocks[j], repair: true, parts: 1 });
        } else {
          worker.postMessage({ integrity: integrities[selected[j]], repair: true, parts: 1 });
          for (const block of blocks[j]) worker.postMessage({ block });
          worker.postMessage({ end: true });
        }
        const value = await pending;
        if (value.failed) throw new Error(value.failed.message);
        if (!value.index) throw new Error("expected assembled index");
        // Stable public worker output; the harness verifies written bytes outside timing.
        output.update(JSON.stringify(value.index));
        for (const file of value.index.files) {
          bytes += file.size; entries++;
        }
      }
    }
    await worker.terminate();
  } else if (mode === "parse") {
    const raw = selected.map(i => readFileSync(files[i] + ".tar"));
    for (let r = 0; r < rounds; r++) for (const data of raw) {
      for await (const entry of extractTar(chunks(data))) {
        output.update(entry.path + ":" + entry.mode + ":" + entry.size + "\n");
        bytes += entry.size; entries++;
      }
    }
  } else {
    for (let r = 0; r < rounds; r++) for (const data of compressed) {
      if (mode === "inflate") { const raw = gunzipSync(data); bytes += raw.length; output.update(raw.subarray(0, 512)); }
      else if (mode === "hash" || mode === "sha256" || mode === "sha1") { output.update(createHash(mode === "hash" ? "sha512" : mode).update(data).digest()); bytes += data.length; }
      else if (mode === "noop") bytes += data.length;
      else throw new Error("unknown mode");
    }
  }
  console.log(mode + " " + selection + " " + entries + " " + bytes + " " + output.digest("hex"));
}
main().catch(error => { console.error(error); process.exit(1); });
