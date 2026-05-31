(process as any).emitWarning = () => {};
import * as fsp from "node:fs/promises";

const ROOT = "/tmp/perry_node_suite_fsp_filehandle_webstream";
try { await fsp.rm(ROOT, { recursive: true, force: true }); } catch (_e) {}
await fsp.mkdir(ROOT, { recursive: true });
const p = ROOT + "/file.txt";
await fsp.writeFile(p, "abcdef");

async function drain(stream: any): Promise<{ text: string; chunks: number; uint8: boolean }> {
  const reader = stream.getReader();
  let text = "";
  let chunks = 0;
  let uint8 = true;
  while (true) {
    const result = await reader.read();
    if (result.done) break;
    chunks++;
    uint8 = uint8 && result.value instanceof Uint8Array;
    text += Buffer.from(result.value).toString("utf8");
  }
  return { text, chunks, uint8 };
}

async function captureCode(label: string, fn: () => any) {
  try {
    await fn();
    console.log(label, "none");
  } catch (e) {
    console.log(label, (e as any).code || (e as any).name);
  }
}

function captureSyncCode(label: string, fn: () => any) {
  try {
    fn();
    console.log(label, "none");
  } catch (e) {
    console.log(label, (e as any).code || (e as any).name);
  }
}

const shape = await fsp.open(p, "r");
console.log("fh webstream method:", typeof (shape as any).readableWebStream);
console.log("fh asyncDispose method:", typeof (shape as any)[Symbol.asyncDispose]);
await shape.close();

const iterHandle = await fsp.open(p, "r");
const iterStream = iterHandle.readableWebStream({ autoClose: true } as any);
let iterText = "";
for await (const chunk of iterStream as any) {
  iterText += Buffer.from(chunk).toString("utf8");
}
console.log("fh webstream for await:", iterText);
console.log("fh webstream for await fd:", iterHandle.fd);

const auto = await fsp.open(p, "r");
const autoResult = await drain((auto as any).readableWebStream({ autoClose: true }));
console.log("fh webstream auto data:", autoResult.text);
console.log("fh webstream auto uint8:", autoResult.uint8);
console.log("fh webstream auto chunks positive:", autoResult.chunks > 0);
console.log("fh webstream auto fd:", auto.fd);
await captureCode("fh webstream auto stat code:", () => auto.stat());

const keep = await fsp.open(p, "r");
const keepResult = await drain((keep as any).readableWebStream());
console.log("fh webstream keep data:", keepResult.text);
console.log("fh webstream keep fd alive:", keep.fd >= 0);
console.log("fh webstream keep stat size:", (await keep.stat()).size);
await keep.close();

const positioned = await fsp.open(p, "r");
const prefix = Buffer.alloc(2);
await positioned.read(prefix, 0, 2, null as any);
const positionedResult = await drain((positioned as any).readableWebStream({ autoClose: true }));
console.log("fh webstream current prefix:", prefix.toString("utf8"));
console.log("fh webstream current data:", positionedResult.text);

const locked = await fsp.open(p, "r");
(locked as any).readableWebStream();
captureSyncCode("fh webstream second code:", () => (locked as any).readableWebStream());
await locked.close();

const closed = await fsp.open(p, "r");
await closed.close();
captureSyncCode("fh webstream closed code:", () => (closed as any).readableWebStream());

const cancelAuto = await fsp.open(p, "r");
const cancelAutoReader = (cancelAuto as any).readableWebStream({ autoClose: true }).getReader();
await cancelAutoReader.cancel();
console.log("fh webstream cancel auto fd:", cancelAuto.fd);
await captureCode("fh webstream cancel auto stat code:", () => cancelAuto.stat());

const cancelKeep = await fsp.open(p, "r");
const cancelKeepReader = (cancelKeep as any).readableWebStream({ autoClose: false }).getReader();
await cancelKeepReader.cancel();
console.log("fh webstream cancel keep fd alive:", cancelKeep.fd >= 0);
await cancelKeep.close();

const disposed = await fsp.open(p, "r");
await (disposed as any)[Symbol.asyncDispose]();
console.log("fh asyncDispose fd:", disposed.fd);
await captureCode("fh asyncDispose stat code:", () => disposed.stat());

const badOptions = await fsp.open(p, "r");
captureSyncCode("fh webstream null options code:", () => (badOptions as any).readableWebStream(null));
await badOptions.close();

const badAutoClose = await fsp.open(p, "r");
captureSyncCode("fh webstream autoClose type code:", () =>
  (badAutoClose as any).readableWebStream({ autoClose: "yes" })
);
await badAutoClose.close();
