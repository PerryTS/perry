// parity-node-argv: --experimental-stream-iter
(process as any).emitWarning = () => {};
import * as fs from "node:fs";
import * as fsp from "node:fs/promises";

const ROOT = "/tmp/perry_node_suite_fsp_filehandle_stream_iter_tail";
try { await fsp.rm(ROOT, { recursive: true, force: true }); } catch (_e) {}
await fsp.mkdir(ROOT, { recursive: true });
const input = ROOT + "/input.txt";
await fsp.writeFile(input, "abcdef");

function batchText(batch: any): string {
  let text = "";
  for (const chunk of batch || []) {
    text += Buffer.from(chunk).toString("utf8");
  }
  return text;
}

async function asyncText(iterable: any): Promise<{ text: string; batches: number; first: string; uint8: boolean }> {
  const iterator = iterable[Symbol.asyncIterator]();
  let text = "";
  let batches = 0;
  let first = "";
  let uint8 = true;
  while (true) {
    const step = await iterator.next();
    if (step.done) break;
    batches++;
    if (batches === 1) first = batchText(step.value);
    for (const chunk of step.value) {
      uint8 = uint8 && chunk instanceof Uint8Array;
    }
    text += batchText(step.value);
  }
  return { text, batches, first, uint8 };
}

function syncText(iterable: any): { text: string; batches: number; first: string; uint8: boolean } {
  const iterator = iterable[Symbol.iterator]();
  let text = "";
  let batches = 0;
  let first = "";
  let uint8 = true;
  while (true) {
    const step = iterator.next();
    if (step.done) break;
    batches++;
    if (batches === 1) first = batchText(step.value);
    for (const chunk of step.value) {
      uint8 = uint8 && chunk instanceof Uint8Array;
    }
    text += batchText(step.value);
  }
  return { text, batches, first, uint8 };
}

function syncCode(label: string, fn: () => any) {
  try {
    fn();
    console.log(label, "none");
  } catch (err) {
    console.log(label, (err as any).code || (err as any).name);
  }
}

const shape = await fsp.open(input, "r");
console.log("fh pull typeof:", typeof (shape as any).pull, (shape as any).pull?.length);
console.log("fh pullSync typeof:", typeof (shape as any).pullSync, (shape as any).pullSync?.length);
console.log("fh writer typeof:", typeof (shape as any).writer, (shape as any).writer?.length);
await shape.close();

const pullKeep = await fsp.open(input, "r");
const pullKeepResult = await asyncText((pullKeep as any).pull({ autoClose: false, chunkSize: 2 }));
console.log("fh pull data:", pullKeepResult.text);
console.log("fh pull first:", pullKeepResult.first);
console.log("fh pull batches positive:", pullKeepResult.batches > 0);
console.log("fh pull uint8:", pullKeepResult.uint8);
console.log("fh pull keep fd:", pullKeep.fd >= 0);
await pullKeep.close();

const pullAuto = await fsp.open(input, "r");
const pullAutoResult = await asyncText((pullAuto as any).pull({ start: 2, limit: 3, autoClose: true, chunkSize: 2 }));
console.log("fh pull start limit:", pullAutoResult.text);
console.log("fh pull auto fd:", pullAuto.fd);

const positioned = await fsp.open(input, "r");
const prefix = Buffer.alloc(2);
await positioned.read(prefix, 0, 2, null as any);
const positionedResult = await asyncText((positioned as any).pull({ autoClose: true }));
console.log("fh pull current prefix:", prefix.toString("utf8"));
console.log("fh pull current data:", positionedResult.text);

const pullSyncKeep = await fsp.open(input, "r");
const pullSyncKeepResult = syncText((pullSyncKeep as any).pullSync({ autoClose: false, chunkSize: 2 }));
console.log("fh pullSync data:", pullSyncKeepResult.text);
console.log("fh pullSync first:", pullSyncKeepResult.first);
console.log("fh pullSync batches positive:", pullSyncKeepResult.batches > 0);
console.log("fh pullSync uint8:", pullSyncKeepResult.uint8);
console.log("fh pullSync keep fd:", pullSyncKeep.fd >= 0);
await pullSyncKeep.close();

const pullSyncAuto = await fsp.open(input, "r");
const pullSyncAutoResult = syncText((pullSyncAuto as any).pullSync({ start: 1, limit: 4, chunkSize: 2, autoClose: true }));
console.log("fh pullSync start limit:", pullSyncAutoResult.text);
console.log("fh pullSync auto fd:", pullSyncAuto.fd);

const closed = await fsp.open(input, "r");
await closed.close();
syncCode("fh pull closed code:", () => (closed as any).pull());
syncCode("fh pullSync closed code:", () => (closed as any).pullSync());
syncCode("fh writer closed code:", () => (closed as any).writer());

const invalidPullOptions = await fsp.open(input, "r");
syncCode("fh pull autoClose code:", () => (invalidPullOptions as any).pull({ autoClose: "yes" }));
syncCode("fh pull chunkSize code:", () => (invalidPullOptions as any).pull({ chunkSize: 0 }));
await invalidPullOptions.close();

const invalidWriterOptions = await fsp.open(ROOT + "/writer-invalid.txt", "w+");
syncCode("fh writer autoClose code:", () => (invalidWriterOptions as any).writer({ autoClose: "yes" }));
await invalidWriterOptions.close();

const writerSyncHandle = await fsp.open(ROOT + "/writer-sync.txt", "w+");
const writerSync = (writerSyncHandle as any).writer({ autoClose: false });
console.log("fh writer keys:", Object.keys(writerSync).join(","));
console.log("fh writer method types:", typeof writerSync.write, typeof writerSync.writev, typeof writerSync.writeSync, typeof writerSync.writevSync, typeof writerSync.end, typeof writerSync.endSync, typeof writerSync.fail);
console.log("fh writer writeSync:", writerSync.writeSync(Buffer.from("ab")));
console.log("fh writer writevSync:", writerSync.writevSync([Buffer.from("cd"), Buffer.from("ef")]));
console.log("fh writer endSync:", writerSync.endSync());
console.log("fh writer sync fd alive:", writerSyncHandle.fd >= 0);
await writerSyncHandle.close();
console.log("fh writer sync data:", fs.readFileSync(ROOT + "/writer-sync.txt", "utf8"));

const writerAsyncHandle = await fsp.open(ROOT + "/writer-async.txt", "w+");
const writerAsync = (writerAsyncHandle as any).writer({ autoClose: true });
console.log("fh writer write await:", await writerAsync.write(Buffer.from("xy")) === undefined);
console.log("fh writer writev await:", await writerAsync.writev([Buffer.from("z")]) === undefined);
console.log("fh writer end await:", await writerAsync.end());
console.log("fh writer async fd:", writerAsyncHandle.fd);
console.log("fh writer async data:", fs.readFileSync(ROOT + "/writer-async.txt", "utf8"));

const writerLimitHandle = await fsp.open(ROOT + "/writer-limit.txt", "w+");
const writerLimit = (writerLimitHandle as any).writer({ limit: 3, autoClose: false });
console.log("fh writer limit first:", writerLimit.writeSync(Buffer.from("12")));
console.log("fh writer limit overflow:", writerLimit.writeSync(Buffer.from("34")));
console.log("fh writer limit async:", await writerLimit.write(Buffer.from("3")) === undefined);
console.log("fh writer limit end:", writerLimit.endSync());
await writerLimitHandle.close();
console.log("fh writer limit data:", fs.readFileSync(ROOT + "/writer-limit.txt", "utf8"));
