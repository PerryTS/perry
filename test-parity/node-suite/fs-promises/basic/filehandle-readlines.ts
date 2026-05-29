import * as fsp from "node:fs/promises";

(process as any).emitWarning = () => {};

const ROOT = "/tmp/perry_node_suite_fs_promises_filehandle_readlines";
try { await fsp.rm(ROOT, { recursive: true, force: true }); } catch (_e) {}
await fsp.mkdir(ROOT, { recursive: true });

async function collect(path: string, options?: { encoding?: BufferEncoding; start?: number; end?: number }) {
  const fh = await fsp.open(path, "r");
  try {
    const lines = fh.readLines(options);
    console.log("fh readLines typeof:", typeof fh.readLines);
    console.log("fh readLines next:", typeof (lines as any).next);
    console.log("fh readLines close:", typeof (lines as any).close);
    console.log("fh readLines asyncIterator:", typeof (lines as any)[Symbol.asyncIterator]);
    const out: string[] = [];
    for await (const line of lines) out.push(line);
    return out;
  } finally {
    await fh.close();
  }
}

const multi = ROOT + "/multi.txt";
await fsp.writeFile(multi, "alpha\nbeta\n", "utf8");
console.log("fh readLines multi:", JSON.stringify(await collect(multi, { encoding: "utf8" })));

const empty = ROOT + "/empty.txt";
await fsp.writeFile(empty, "", "utf8");
console.log("fh readLines empty:", JSON.stringify(await collect(empty)));

const final = ROOT + "/final.txt";
await fsp.writeFile(final, "last-line", "utf8");
console.log("fh readLines final:", JSON.stringify(await collect(final)));

const range = ROOT + "/range.txt";
await fsp.writeFile(range, "zero\none\ntwo\nthree", "utf8");
console.log("fh readLines range:", JSON.stringify(await collect(range, { encoding: "utf8", start: 5, end: 11 })));

const early = await fsp.open(multi, "r");
const earlyLines: string[] = [];
for await (const line of early.readLines()) {
  earlyLines.push(line);
  break;
}
console.log("fh readLines early break:", JSON.stringify(earlyLines), early.fd >= 0 ? "open" : "closed");
await early.close();

const closed = await fsp.open(multi, "r");
await closed.close();
try {
  closed.readLines();
  console.log("fh readLines closed:", "none");
} catch (e) {
  const err = e as NodeJS.ErrnoException;
  console.log("fh readLines closed:", `${err.name}:${err.code}:${err.message.split("\n")[0]}`);
}
