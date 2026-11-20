import * as fs from "node:fs";
import { Readable } from "node:stream";

const ROOT = "/tmp/perry_node_suite_fs_callback_writefile_inputs";
try { fs.rmSync(ROOT, { recursive: true, force: true }); } catch (_e) {}
fs.mkdirSync(ROOT, { recursive: true });

function errText(err: any): string {
  return err ? `${err.name}:${err.code}` : "ok";
}

function writeFileP(label: string, path: string, data: any, options?: any): Promise<void> {
  return new Promise((resolve) => {
    const cb = (err: any) => {
      console.log(label + " err:", errText(err));
      resolve();
    };
    try {
      if (options === undefined) fs.writeFile(path, data, cb);
      else fs.writeFile(path, data, options, cb);
    } catch (err: any) {
      console.log(label + " threw:", errText(err));
      resolve();
    }
  });
}

await writeFileP("callback iterable", ROOT + "/iterable.txt", ["41", Buffer.from("B"), new Uint8Array([67])], { encoding: "hex" });
console.log("callback iterable content:", fs.readFileSync(ROOT + "/iterable.txt", "utf8"));

async function* asyncChunks() {
  yield "D";
  yield Buffer.from("E");
}
await writeFileP("callback async iterable", ROOT + "/async-iterable.txt", asyncChunks());
console.log("callback async iterable content:", fs.readFileSync(ROOT + "/async-iterable.txt", "utf8"));

await writeFileP("callback Readable.from", ROOT + "/readable-from.txt", Readable.from(["F", Buffer.from("G")]));
console.log("callback Readable.from content:", fs.readFileSync(ROOT + "/readable-from.txt", "utf8"));

fs.writeFileSync(ROOT + "/source.txt", "HI");
await writeFileP("callback createReadStream", ROOT + "/readstream.txt", fs.createReadStream(ROOT + "/source.txt"));
console.log("callback createReadStream content:", fs.readFileSync(ROOT + "/readstream.txt", "utf8"));

await writeFileP("callback invalid chunk", ROOT + "/invalid-chunk.txt", ["ok", 7 as any]);

const pre = new AbortController();
pre.abort();
await writeFileP("callback pre abort", ROOT + "/pre-abort.txt", ["x"], { signal: pre.signal });
console.log("callback pre abort exists:", fs.existsSync(ROOT + "/pre-abort.txt"));

const mid = new AbortController();
function* abortingChunks() {
  yield "A";
  mid.abort();
  yield "B";
}
await writeFileP("callback mid abort", ROOT + "/mid-abort.txt", abortingChunks(), { signal: mid.signal });
console.log("callback mid abort content:", fs.existsSync(ROOT + "/mid-abort.txt") ? fs.readFileSync(ROOT + "/mid-abort.txt", "utf8") : "missing");
